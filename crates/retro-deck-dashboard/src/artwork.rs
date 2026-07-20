//! Optional persistent cover cache with bounded PNG decoding.

use std::error::Error;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use png::{BitDepth, ColorType, Decoder, Limits, Transformations};
use retro_deck_config::CatalogEntry;
use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};
use retro_deck_ui::rgb888_to_rgb565;

use crate::{ArtworkProvider, Cover, MAXIMUM_DASHBOARD_ENTRIES};

const MAXIMUM_COMPRESSED_BYTES: usize = 4 * 1_024 * 1_024;
const MAXIMUM_SOURCE_DIMENSION: usize = 2_048;
const MAXIMUM_DECODED_BYTES: usize = MAXIMUM_SOURCE_DIMENSION * MAXIMUM_SOURCE_DIMENSION * 4;
const MAXIMUM_DECODER_BYTES: usize = 32 * 1_024 * 1_024;
const MAXIMUM_COVER_WIDTH: usize = 600;
const MAXIMUM_COVER_HEIGHT: usize = 378;
const MAXIMUM_COVER_PIXELS: usize =
    MAXIMUM_DASHBOARD_ENTRIES * MAXIMUM_COVER_WIDTH * MAXIMUM_COVER_HEIGHT;

/// Covers decoded from the persistent cache and indexed by catalog identity.
#[derive(Debug, Default)]
pub struct ArtworkStore {
    covers: Vec<StoredCover>,
    issues: Vec<ArtworkIssue>,
    missing: usize,
    capacity_skipped: usize,
}

impl ArtworkStore {
    /// Load every matching `<identifier>.png` from one absolute cache path.
    ///
    /// A missing file is ordinary cache state. Invalid files are skipped and
    /// retained as bounded diagnostics. The store never fetches over a network.
    ///
    /// # Errors
    ///
    /// Returns [`ArtworkStoreError`] only when the cache directory path is not
    /// absolute. Individual file failures do not prevent construction.
    pub fn load(
        directory: impl AsRef<Path>,
        entries: &[CatalogEntry],
    ) -> Result<Self, ArtworkStoreError> {
        let directory = directory.as_ref();
        if !directory.is_absolute() {
            return Err(ArtworkStoreError(directory.to_path_buf()));
        }

        let mut store = Self {
            covers: Vec::with_capacity(entries.len().min(MAXIMUM_DASHBOARD_ENTRIES)),
            issues: Vec::new(),
            missing: 0,
            capacity_skipped: 0,
        };
        let mut stored_pixels = 0_usize;
        for entry in entries.iter().take(MAXIMUM_DASHBOARD_ENTRIES) {
            let path = directory.join(format!("{}.png", entry.identifier()));
            match load_png(&path, entry.color().components()) {
                Ok(cover) => {
                    let requested = stored_pixels.saturating_add(cover.pixels.len());
                    if requested > MAXIMUM_COVER_PIXELS {
                        store.capacity_skipped = store.capacity_skipped.saturating_add(1);
                        continue;
                    }
                    stored_pixels = requested;
                    store.covers.push(StoredCover {
                        identifier: Box::from(entry.identifier()),
                        width: cover.width,
                        height: cover.height,
                        pixels: cover.pixels,
                    });
                }
                Err(ArtworkError::Read(BoundedReadError::Open { source, .. }))
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    store.missing = store.missing.saturating_add(1);
                }
                Err(error) => store.issues.push(ArtworkIssue {
                    identifier: Box::from(entry.identifier()),
                    error,
                }),
            }
        }
        Ok(store)
    }

    /// Summarize optional cache loading without exposing allocations.
    #[must_use]
    pub fn report(&self) -> ArtworkReport {
        ArtworkReport {
            loaded: self.covers.len(),
            missing: self.missing,
            invalid: self.issues.len(),
            capacity_skipped: self.capacity_skipped,
        }
    }

    /// Invalid cache entries retained for startup diagnostics.
    #[must_use]
    pub fn issues(&self) -> impl ExactSizeIterator<Item = &ArtworkIssue> {
        self.issues.iter()
    }
}

impl ArtworkProvider for ArtworkStore {
    fn cover(&self, identifier: &str) -> Option<Cover<'_>> {
        let stored = self
            .covers
            .iter()
            .find(|cover| cover.identifier.as_ref() == identifier)?;
        Cover::new(stored.width, stored.height, &stored.pixels).ok()
    }
}

#[derive(Debug)]
struct StoredCover {
    identifier: Box<str>,
    width: usize,
    height: usize,
    pixels: Box<[u16]>,
}

#[derive(Debug)]
struct DecodedCover {
    width: usize,
    height: usize,
    pixels: Box<[u16]>,
}

/// Bounded optional-artwork loading counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArtworkReport {
    /// Covers available to the renderer.
    pub loaded: usize,
    /// Catalog entries without a cached PNG.
    pub missing: usize,
    /// Cached PNGs rejected during validation or decoding.
    pub invalid: usize,
    /// Valid covers omitted after the global decoded-pixel budget was full.
    pub capacity_skipped: usize,
}

/// One invalid optional cache entry.
#[derive(Debug)]
pub struct ArtworkIssue {
    identifier: Box<str>,
    error: ArtworkError,
}

impl ArtworkIssue {
    /// Catalog identifier whose cached cover was rejected.
    #[must_use]
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Validation or decoding failure for this cache entry.
    #[must_use]
    pub const fn error(&self) -> &ArtworkError {
        &self.error
    }
}

impl fmt::Display for ArtworkIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cover {} is unavailable: {}",
            self.identifier, self.error
        )
    }
}

impl Error for ArtworkIssue {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

/// Cache directory violates the fixed absolute-path contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtworkStoreError(PathBuf);

impl fmt::Display for ArtworkStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cover cache path is not absolute: {}",
            self.0.display()
        )
    }
}

impl Error for ArtworkStoreError {}

/// One cached PNG failed bounded validation or decoding.
#[derive(Debug)]
pub enum ArtworkError {
    /// Compressed input was unavailable, unsafe, or outside its byte bound.
    Read(BoundedReadError),
    /// PNG structure or compressed pixels were invalid.
    Decode(png::DecodingError),
    /// Source geometry exceeded the decoder contract.
    Dimensions { width: u32, height: u32 },
    /// Animated covers are not accepted as static dashboard assets.
    Animated,
    /// Decoded storage or color layout violated the normalized contract.
    Layout,
    /// A bounded output allocation could not be reserved.
    Allocate(std::collections::TryReserveError),
}

impl fmt::Display for ArtworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(formatter),
            Self::Decode(error) => write!(formatter, "PNG decode failed: {error}"),
            Self::Dimensions { width, height } => write!(
                formatter,
                "PNG dimensions {width}x{height} are outside 1 through {MAXIMUM_SOURCE_DIMENSION}"
            ),
            Self::Animated => formatter.write_str("animated PNG covers are unsupported"),
            Self::Layout => formatter.write_str("decoded PNG has an invalid pixel layout"),
            Self::Allocate(error) => write!(formatter, "cannot allocate decoded cover: {error}"),
        }
    }
}

impl Error for ArtworkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::Allocate(error) => Some(error),
            Self::Dimensions { .. } | Self::Animated | Self::Layout => None,
        }
    }
}

fn load_png(path: &Path, background: [u8; 3]) -> Result<DecodedCover, ArtworkError> {
    let bytes = read_regular_bounded(path, MAXIMUM_COMPRESSED_BYTES).map_err(ArtworkError::Read)?;
    let mut png_decoder = Decoder::new_with_limits(
        Cursor::new(bytes),
        Limits {
            bytes: MAXIMUM_DECODER_BYTES,
        },
    );
    png_decoder.set_transformations(Transformations::ALPHA | Transformations::STRIP_16);
    let mut reader = png_decoder.read_info().map_err(ArtworkError::Decode)?;
    let source_width_value = reader.info().width;
    let source_height_value = reader.info().height;
    let source_width = usize::try_from(source_width_value).ok();
    let source_height = usize::try_from(source_height_value).ok();
    let (Some(source_width), Some(source_height)) = (source_width, source_height) else {
        return Err(ArtworkError::Dimensions {
            width: source_width_value,
            height: source_height_value,
        });
    };
    if source_width == 0
        || source_width > MAXIMUM_SOURCE_DIMENSION
        || source_height == 0
        || source_height > MAXIMUM_SOURCE_DIMENSION
    {
        return Err(ArtworkError::Dimensions {
            width: source_width_value,
            height: source_height_value,
        });
    }
    if reader.info().animation_control.is_some() {
        return Err(ArtworkError::Animated);
    }
    let decoded_size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAXIMUM_DECODED_BYTES)
        .ok_or(ArtworkError::Layout)?;
    let mut source_pixels = Vec::new();
    source_pixels
        .try_reserve_exact(decoded_size)
        .map_err(ArtworkError::Allocate)?;
    source_pixels.resize(decoded_size, 0);
    let output = reader
        .next_frame(&mut source_pixels)
        .map_err(ArtworkError::Decode)?;
    if output.bit_depth != BitDepth::Eight
        || usize::try_from(output.width).ok() != Some(source_width)
        || usize::try_from(output.height).ok() != Some(source_height)
        || output.buffer_size() > source_pixels.len()
    {
        return Err(ArtworkError::Layout);
    }
    let source_pixels = source_pixels
        .get(..output.buffer_size())
        .ok_or(ArtworkError::Layout)?;
    let (width, height) = target_dimensions(source_width, source_height);
    let pixel_count = width.checked_mul(height).ok_or(ArtworkError::Layout)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(ArtworkError::Allocate)?;
    for y in 0..height {
        let source_y = y.saturating_mul(source_height) / height;
        for x in 0..width {
            let source_x = x.saturating_mul(source_width) / width;
            let rgba = source_pixel(
                source_pixels,
                output.line_size,
                output.color_type,
                source_x,
                source_y,
            )
            .ok_or(ArtworkError::Layout)?;
            pixels.push(quantized_pixel(composite(rgba, background)));
        }
    }
    Ok(DecodedCover {
        width,
        height,
        pixels: pixels.into_boxed_slice(),
    })
}

const fn target_dimensions(width: usize, height: usize) -> (usize, usize) {
    if width <= MAXIMUM_COVER_WIDTH && height <= MAXIMUM_COVER_HEIGHT {
        return (width, height);
    }
    if width.saturating_mul(MAXIMUM_COVER_HEIGHT) > height.saturating_mul(MAXIMUM_COVER_WIDTH) {
        let scaled_height = height.saturating_mul(MAXIMUM_COVER_WIDTH) / width;
        (
            MAXIMUM_COVER_WIDTH,
            if scaled_height == 0 { 1 } else { scaled_height },
        )
    } else {
        let scaled_width = width.saturating_mul(MAXIMUM_COVER_HEIGHT) / height;
        (
            if scaled_width == 0 { 1 } else { scaled_width },
            MAXIMUM_COVER_HEIGHT,
        )
    }
}

fn source_pixel(
    bytes: &[u8],
    stride: usize,
    color: ColorType,
    x: usize,
    y: usize,
) -> Option<[u8; 4]> {
    let samples = color.samples();
    let offset = y
        .checked_mul(stride)?
        .checked_add(x.checked_mul(samples)?)?;
    let values = bytes.get(offset..offset.checked_add(samples)?)?;
    match (color, values) {
        (ColorType::Grayscale, [gray]) => Some([*gray, *gray, *gray, 255]),
        (ColorType::Rgb, [red, green, blue]) => Some([*red, *green, *blue, 255]),
        (ColorType::GrayscaleAlpha, [gray, alpha]) => Some([*gray, *gray, *gray, *alpha]),
        (ColorType::Rgba, [red, green, blue, alpha]) => Some([*red, *green, *blue, *alpha]),
        _ => None,
    }
}

fn composite([red, green, blue, alpha]: [u8; 4], background: [u8; 3]) -> [u8; 3] {
    [
        composite_channel(red, background[0], alpha),
        composite_channel(green, background[1], alpha),
        composite_channel(blue, background[2], alpha),
    ]
}

fn composite_channel(foreground: u8, background: u8, alpha: u8) -> u8 {
    let alpha = u32::from(alpha);
    let inverse = 255_u32.saturating_sub(alpha);
    let value = (u32::from(foreground) * alpha + u32::from(background) * inverse + 127) / 255;
    u8::try_from(value).unwrap_or_default()
}

fn quantized_pixel([red, green, blue]: [u8; 3]) -> u16 {
    let table = xterm_quantization_table();
    let index =
        (usize::from(red >> 3) << 10) | (usize::from(green >> 3) << 5) | usize::from(blue >> 3);
    table.get(index).copied().unwrap_or_default()
}

fn xterm_quantization_table() -> &'static [u16] {
    static TABLE: OnceLock<Box<[u16]>> = OnceLock::new();
    TABLE.get_or_init(build_xterm_quantization_table)
}

fn build_xterm_quantization_table() -> Box<[u16]> {
    let mut table = Vec::with_capacity(32 * 32 * 32);
    for red5 in 0_u8..32 {
        let red = (red5 << 3) | (red5 >> 2);
        for green5 in 0_u8..32 {
            let green = (green5 << 3) | (green5 >> 2);
            for blue5 in 0_u8..32 {
                let blue = (blue5 << 3) | (blue5 >> 2);
                table.push(nearest_xterm_pixel([red, green, blue]));
            }
        }
    }
    table.into_boxed_slice()
}

fn nearest_xterm_pixel(color: [u8; 3]) -> u16 {
    let mut best_distance = u32::MAX;
    let mut best = [0_u8; 3];
    for index in 0_u16..=255 {
        let candidate = xterm_color(index);
        let red = i32::from(color[0]) - i32::from(candidate[0]);
        let green = i32::from(color[1]) - i32::from(candidate[1]);
        let blue = i32::from(color[2]) - i32::from(candidate[2]);
        let distance = u32::try_from(red * red + green * green + blue * blue).unwrap_or(u32::MAX);
        if distance < best_distance {
            best_distance = distance;
            best = candidate;
        }
    }
    rgb888_to_rgb565((u32::from(best[0]) << 16) | (u32::from(best[1]) << 8) | u32::from(best[2]))
}

fn xterm_color(index: u16) -> [u8; 3] {
    if index < 16 {
        match index {
            0 => [0, 0, 0],
            1 => [128, 0, 0],
            2 => [0, 128, 0],
            3 => [128, 128, 0],
            4 => [0, 0, 128],
            5 => [128, 0, 128],
            6 => [0, 128, 128],
            7 => [192, 192, 192],
            8 => [128, 128, 128],
            9 => [255, 0, 0],
            10 => [0, 255, 0],
            11 => [255, 255, 0],
            12 => [0, 0, 255],
            13 => [255, 0, 255],
            14 => [0, 255, 255],
            _ => [255, 255, 255],
        }
    } else if index < 232 {
        let cube = index - 16;
        [
            cube_level(cube / 36),
            cube_level((cube / 6) % 6),
            cube_level(cube % 6),
        ]
    } else {
        let level = u8::try_from(8 + (index - 232) * 10).unwrap_or_default();
        [level, level, level]
    }
}

const fn cube_level(index: u16) -> u8 {
    match index {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        _ => 255,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    use png::{BitDepth, ColorType, Encoder};
    use retro_deck_config::{CatalogEntry, CatalogSystem, System};
    use retro_deck_ui::rgb888_to_rgb565;

    use super::{ArtworkIssue, ArtworkStore, ArtworkStoreError, target_dimensions};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "retro-deck-artwork-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("artwork fixture directory is created");
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    fn entry(identifier: &str) -> CatalogEntry {
        CatalogEntry::new(
            identifier,
            "COVER TEST",
            CatalogSystem::Rom(System::Nes),
            "/mnt/data/roms/nes/cover-test.nes",
            "#000000",
        )
        .expect("artwork fixture entry is valid")
    }

    fn png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut png_encoder = Encoder::new(&mut bytes, width, height);
            png_encoder.set_color(ColorType::Rgba);
            png_encoder.set_depth(BitDepth::Eight);
            let mut writer = png_encoder.write_header().expect("PNG header is encoded");
            writer
                .write_image_data(pixels)
                .expect("PNG pixels are encoded");
        }
        bytes
    }

    #[test]
    fn decodes_composites_and_quantizes_a_cached_png_once() {
        let fixture = Fixture::new();
        fs::write(
            fixture.0.join("cover.png"),
            png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 128]),
        )
        .expect("cover PNG is written");
        let store = ArtworkStore::load(&fixture.0, &[entry("cover")])
            .expect("absolute artwork cache loads");

        assert_eq!(store.report().loaded, 1);
        assert!(store.issues().next().is_none());
        let Some(cover) = store.covers.first() else {
            return;
        };
        assert_eq!(cover.width, 2);
        assert_eq!(cover.height, 1);
        assert_eq!(
            cover.pixels.as_ref(),
            [rgb888_to_rgb565(0x00ff_0000), rgb888_to_rgb565(0x0000_0080)]
        );
    }

    #[test]
    fn missing_and_unsafe_optional_covers_never_enter_the_store() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("target"), png(1, 1, &[0, 0, 0, 255]))
            .expect("symlink target is written");
        symlink(fixture.0.join("target"), fixture.0.join("linked.png"))
            .expect("cover symlink is created");
        let entries = [entry("missing"), entry("linked")];
        let store = ArtworkStore::load(&fixture.0, &entries)
            .expect("optional artwork failures remain nonfatal");

        assert_eq!(store.report().missing, 1);
        assert_eq!(store.report().invalid, 1);
        assert_eq!(store.report().loaded, 0);
        assert_eq!(
            store.issues().next().map(ArtworkIssue::identifier),
            Some("linked")
        );
        assert_eq!(
            ArtworkStore::load("relative", &entries).map(|_| ()),
            Err(ArtworkStoreError(std::path::PathBuf::from("relative")))
        );
    }

    #[test]
    fn downscaling_keeps_extreme_aspect_ratios_nonzero() {
        assert_eq!(target_dimensions(2_048, 1), (600, 1));
        assert_eq!(target_dimensions(1, 2_048), (1, 378));
        assert_eq!(target_dimensions(600, 378), (600, 378));
    }
}
