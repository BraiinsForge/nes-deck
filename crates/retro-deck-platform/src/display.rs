//! Validated pixel frames, nearest-neighbor scaling, and buffer ownership.

use std::error::Error;
use std::fmt;

use crate::{DECK_LOGICAL_HEIGHT, DECK_LOGICAL_WIDTH};

const SAFE_INSET: usize = 16;
const MAXIMUM_DIMENSION: usize = 16_384;
const MAXIMUM_PIXELS: usize = 8 * 1_024 * 1_024;
const SLOT_COUNT: usize = 3;

/// Nonzero two-dimensional pixel extent with a bounded allocation footprint.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Dimensions {
    width: usize,
    height: usize,
    pixels: usize,
}

impl Dimensions {
    /// Construct dimensions whose pixel count is safe for a native frame.
    #[must_use]
    pub const fn new(width: usize, height: usize) -> Option<Self> {
        if width == 0 || height == 0 || width > MAXIMUM_DIMENSION || height > MAXIMUM_DIMENSION {
            return None;
        }
        let Some(pixels) = width.checked_mul(height) else {
            return None;
        };
        if pixels > MAXIMUM_PIXELS {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Horizontal pixel count.
    #[must_use]
    pub const fn width(self) -> usize {
        self.width
    }

    /// Vertical pixel count.
    #[must_use]
    pub const fn height(self) -> usize {
        self.height
    }

    /// Complete pixel count.
    #[must_use]
    pub const fn pixel_count(self) -> usize {
        self.pixels
    }
}

/// Full logical Deck surface.
pub const DECK_DIMENSIONS: Dimensions = Dimensions {
    width: DECK_LOGICAL_WIDTH as usize,
    height: DECK_LOGICAL_HEIGHT as usize,
    pixels: DECK_LOGICAL_WIDTH as usize * DECK_LOGICAL_HEIGHT as usize,
};

/// Compute the centered, integer-scaled gameplay surface inside the safe edge.
///
/// # Errors
///
/// Returns [`DisplayError::SourceDoesNotFit`] when even a 1:1 source exceeds
/// the safe Deck area.
pub fn gameplay_dimensions(source: Dimensions) -> Result<Dimensions, DisplayError> {
    let available_width = DECK_DIMENSIONS.width - 2 * SAFE_INSET;
    let available_height = DECK_DIMENSIONS.height - 2 * SAFE_INSET;
    let horizontal = available_width / source.width;
    let vertical = available_height / source.height;
    let scale = horizontal.min(vertical);
    if scale == 0 {
        return Err(DisplayError::SourceDoesNotFit);
    }
    Dimensions::new(source.width * scale, source.height * scale)
        .ok_or(DisplayError::InvalidDimensions)
}

/// Borrowed, stride-aware source frame accepted by the display backend.
#[derive(Clone, Copy, Debug)]
pub enum Frame<'pixels> {
    /// Native RGB565 pixels.
    Rgb565 {
        dimensions: Dimensions,
        stride: usize,
        pixels: &'pixels [u16],
    },
    /// XRGB8888 pixels. The unused high byte is normalized to `0xff`.
    Xrgb8888 {
        dimensions: Dimensions,
        stride: usize,
        pixels: &'pixels [u32],
    },
    /// Eight-bit palette indexes. Missing palette entries render black.
    Indexed8 {
        dimensions: Dimensions,
        stride: usize,
        pixels: &'pixels [u8],
        palette: &'pixels [u32],
    },
}

impl<'pixels> Frame<'pixels> {
    /// Validate and borrow one RGB565 frame.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] when the stride is narrower than the frame or
    /// the slice does not include its final visible pixel.
    pub fn rgb565(
        pixels: &'pixels [u16],
        dimensions: Dimensions,
        stride: usize,
    ) -> Result<Self, DisplayError> {
        validate_source(pixels.len(), dimensions, stride)?;
        Ok(Self::Rgb565 {
            dimensions,
            stride,
            pixels,
        })
    }

    /// Validate and borrow one XRGB8888 frame.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] when the stride is narrower than the frame or
    /// the slice does not include its final visible pixel.
    pub fn xrgb8888(
        pixels: &'pixels [u32],
        dimensions: Dimensions,
        stride: usize,
    ) -> Result<Self, DisplayError> {
        validate_source(pixels.len(), dimensions, stride)?;
        Ok(Self::Xrgb8888 {
            dimensions,
            stride,
            pixels,
        })
    }

    /// Validate and borrow one indexed frame and palette.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] when the source is truncated, its stride is
    /// too narrow, or its palette is empty.
    pub fn indexed8(
        pixels: &'pixels [u8],
        dimensions: Dimensions,
        stride: usize,
        palette: &'pixels [u32],
    ) -> Result<Self, DisplayError> {
        validate_source(pixels.len(), dimensions, stride)?;
        if palette.is_empty() {
            return Err(DisplayError::EmptyPalette);
        }
        Ok(Self::Indexed8 {
            dimensions,
            stride,
            pixels,
            palette,
        })
    }

    /// Source dimensions shared by every pixel format.
    #[must_use]
    pub const fn dimensions(self) -> Dimensions {
        match self {
            Self::Rgb565 { dimensions, .. }
            | Self::Xrgb8888 { dimensions, .. }
            | Self::Indexed8 { dimensions, .. } => dimensions,
        }
    }
}

/// Cached nearest-neighbor coordinate mapping for a fixed target surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalePlan {
    source: Dimensions,
    target: Dimensions,
    horizontal: Box<[usize]>,
    vertical: Box<[usize]>,
}

impl ScalePlan {
    /// Precompute a nearest-neighbor mapping with no per-frame allocation.
    #[must_use]
    pub fn new(source: Dimensions, target: Dimensions) -> Self {
        let horizontal = coordinate_map(source.width, target.width);
        let vertical = coordinate_map(source.height, target.height);
        Self {
            source,
            target,
            horizontal,
            vertical,
        }
    }

    /// Target surface dimensions.
    #[must_use]
    pub const fn target(&self) -> Dimensions {
        self.target
    }

    /// Source frame dimensions currently represented by the coordinate maps.
    #[must_use]
    pub const fn source(&self) -> Dimensions {
        self.source
    }

    /// Rebuild coordinate maps for a changed source and the same target.
    ///
    /// Returns whether the source changed. The target surface and its backing
    /// allocation remain untouched.
    pub fn update_source(&mut self, source: Dimensions) -> bool {
        if source == self.source {
            return false;
        }
        let horizontal = coordinate_map(source.width, self.target.width);
        let vertical = coordinate_map(source.height, self.target.height);
        self.source = source;
        self.horizontal = horizontal;
        self.vertical = vertical;
        true
    }

    /// Convert and scale one complete frame into a persistent XRGB8888 slot.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the source dimensions changed or the target
    /// slot is not exactly the configured surface size.
    pub fn blit(&self, frame: Frame<'_>, target: &mut [u32]) -> Result<(), DisplayError> {
        if frame.dimensions() != self.source {
            return Err(DisplayError::SourceDimensionsChanged);
        }
        if target.len() != self.target.pixels {
            return Err(DisplayError::TargetLength);
        }

        match frame {
            Frame::Rgb565 { stride, pixels, .. } => self.blit_with(target, |x, y| {
                pixels
                    .get(y * stride + x)
                    .copied()
                    .map_or(0xff00_0000, rgb565_to_xrgb8888)
            }),
            Frame::Xrgb8888 { stride, pixels, .. } => self.blit_with(target, |x, y| {
                pixels
                    .get(y * stride + x)
                    .copied()
                    .map_or(0xff00_0000, normalize_xrgb8888)
            }),
            Frame::Indexed8 {
                stride,
                pixels,
                palette,
                ..
            } => self.blit_with(target, |x, y| {
                pixels
                    .get(y * stride + x)
                    .and_then(|index| palette.get(usize::from(*index)))
                    .copied()
                    .map_or(0xff00_0000, normalize_xrgb8888)
            }),
        }
    }

    fn blit_with(
        &self,
        target: &mut [u32],
        mut pixel: impl FnMut(usize, usize) -> u32,
    ) -> Result<(), DisplayError> {
        for (target_y, row) in target.chunks_exact_mut(self.target.width).enumerate() {
            let Some(source_y) = self.vertical.get(target_y).copied() else {
                return Err(DisplayError::InternalScaleMap);
            };
            for (target_x, destination) in row.iter_mut().enumerate() {
                let Some(source_x) = self.horizontal.get(target_x).copied() else {
                    return Err(DisplayError::InternalScaleMap);
                };
                *destination = pixel(source_x, source_y);
            }
        }
        Ok(())
    }
}

fn coordinate_map(source: usize, target: usize) -> Box<[usize]> {
    (0..target)
        .map(|coordinate| coordinate * source / target)
        .collect()
}

fn validate_source(
    available: usize,
    dimensions: Dimensions,
    stride: usize,
) -> Result<(), DisplayError> {
    if stride < dimensions.width {
        return Err(DisplayError::InvalidStride);
    }
    let required = (dimensions.height - 1)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(dimensions.width))
        .ok_or(DisplayError::InvalidDimensions)?;
    if available < required {
        return Err(DisplayError::TruncatedSource);
    }
    Ok(())
}

/// Convert one RGB565 pixel to opaque XRGB8888.
#[must_use]
pub const fn rgb565_to_xrgb8888(pixel: u16) -> u32 {
    let red = ((pixel >> 11) & 0x1f) as u32;
    let green = ((pixel >> 5) & 0x3f) as u32;
    let blue = (pixel & 0x1f) as u32;
    0xff00_0000 | ((red * 255 / 31) << 16) | ((green * 255 / 63) << 8) | (blue * 255 / 31)
}

const fn normalize_xrgb8888(pixel: u32) -> u32 {
    0xff00_0000 | (pixel & 0x00ff_ffff)
}

/// Display validation or fixed-surface mismatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayError {
    /// Width, height, or their product is unsupported.
    InvalidDimensions,
    /// One source scanline is narrower than its visible width.
    InvalidStride,
    /// The source slice ends before the last visible pixel.
    TruncatedSource,
    /// An indexed frame has no palette entries.
    EmptyPalette,
    /// A gameplay source is larger than the safe Deck surface.
    SourceDoesNotFit,
    /// A surface received a frame with different source dimensions.
    SourceDimensionsChanged,
    /// The shared-memory slot length differs from the configured target.
    TargetLength,
    /// A cached coordinate map was internally inconsistent.
    InternalScaleMap,
}

impl fmt::Display for DisplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDimensions => "pixel dimensions are invalid or excessive",
            Self::InvalidStride => "frame stride is narrower than its visible width",
            Self::TruncatedSource => "frame source is truncated",
            Self::EmptyPalette => "indexed frame palette is empty",
            Self::SourceDoesNotFit => "gameplay source does not fit inside the safe Deck surface",
            Self::SourceDimensionsChanged => "frame source dimensions changed after configuration",
            Self::TargetLength => "target slot does not match the configured surface",
            Self::InternalScaleMap => "cached scaling map is inconsistent",
        })
    }
}

impl Error for DisplayError {}

/// Stable identifier for one of three persistent presentation slots.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SlotId(usize);

impl SlotId {
    pub(crate) const fn from_index(index: usize) -> Option<Self> {
        if index < SLOT_COUNT {
            Some(Self(index))
        } else {
            None
        }
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotState {
    Available,
    Writing,
    InFlight,
}

/// Explicit ownership state for three persistent compositor buffers.
///
/// Slots are never recycled until the compositor releases them. Visibility
/// changes do not alter this state or request buffer destruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentationSlots {
    states: [SlotState; SLOT_COUNT],
    next: usize,
}

impl PresentationSlots {
    /// Construct three available persistent slots.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            states: [SlotState::Available; SLOT_COUNT],
            next: 0,
        }
    }

    /// Reserve the next free slot for CPU writes without waiting.
    pub fn acquire(&mut self) -> Option<SlotId> {
        for offset in 0..SLOT_COUNT {
            let index = (self.next + offset) % SLOT_COUNT;
            let state = self.states.get_mut(index)?;
            if *state == SlotState::Available {
                *state = SlotState::Writing;
                self.next = (index + 1) % SLOT_COUNT;
                return SlotId::from_index(index);
            }
        }
        None
    }

    /// Transfer a written slot to the compositor.
    ///
    /// # Errors
    ///
    /// Returns [`SlotError`] unless the slot is currently reserved for writes.
    pub fn submit(&mut self, slot: SlotId) -> Result<(), SlotError> {
        self.transition(slot, SlotState::Writing, SlotState::InFlight)
    }

    /// Cancel a CPU write reservation without submitting it.
    ///
    /// # Errors
    ///
    /// Returns [`SlotError`] unless the slot is currently reserved for writes.
    pub fn cancel(&mut self, slot: SlotId) -> Result<(), SlotError> {
        self.transition(slot, SlotState::Writing, SlotState::Available)
    }

    /// Accept one compositor release and make the slot writable again.
    ///
    /// # Errors
    ///
    /// Returns [`SlotError`] unless the slot is currently owned by the
    /// compositor.
    pub fn release(&mut self, slot: SlotId) -> Result<(), SlotError> {
        self.transition(slot, SlotState::InFlight, SlotState::Available)
    }

    fn transition(
        &mut self,
        slot: SlotId,
        expected: SlotState,
        replacement: SlotState,
    ) -> Result<(), SlotError> {
        let state = self
            .states
            .get_mut(slot.index())
            .ok_or(SlotError::UnknownSlot)?;
        if *state != expected {
            return Err(SlotError::InvalidOwnership);
        }
        *state = replacement;
        Ok(())
    }
}

impl Default for PresentationSlots {
    fn default() -> Self {
        Self::new()
    }
}

/// Invalid presentation-slot ownership transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotError {
    /// The identifier is outside the fixed three-slot pool.
    UnknownSlot,
    /// The caller does not currently own the slot in the required state.
    InvalidOwnership,
}

impl fmt::Display for SlotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownSlot => "presentation slot does not exist",
            Self::InvalidOwnership => "presentation slot ownership transition is invalid",
        })
    }
}

impl Error for SlotError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: Dimensions = Dimensions {
        width: 2,
        height: 2,
        pixels: 4,
    };
    const TARGET: Dimensions = Dimensions {
        width: 4,
        height: 4,
        pixels: 16,
    };

    #[test]
    fn dimensions_are_nonzero_bounded_and_checked() {
        assert_eq!(Dimensions::new(0, 1), None);
        assert_eq!(Dimensions::new(1, 0), None);
        assert_eq!(Dimensions::new(usize::MAX, 2), None);
        assert_eq!(Dimensions::new(2, 3).map(Dimensions::pixel_count), Some(6));
    }

    #[test]
    fn gameplay_size_is_integer_scaled_inside_the_safe_edge() {
        let gb = Dimensions::new(160, 144).expect("fixed dimensions are valid");
        let nes = Dimensions::new(256, 240).expect("fixed dimensions are valid");
        assert_eq!(
            gameplay_dimensions(gb),
            Dimensions::new(480, 432).ok_or(DisplayError::InvalidDimensions)
        );
        assert_eq!(
            gameplay_dimensions(nes),
            Dimensions::new(256, 240).ok_or(DisplayError::InvalidDimensions)
        );
        let oversized = Dimensions::new(1_249, 1).expect("fixed dimensions are valid");
        assert_eq!(
            gameplay_dimensions(oversized),
            Err(DisplayError::SourceDoesNotFit)
        );
    }

    #[test]
    fn frame_constructors_validate_stride_length_and_palette() {
        assert_eq!(
            Frame::rgb565(&[0; 4], SOURCE, 1).map(|_| ()),
            Err(DisplayError::InvalidStride)
        );
        assert_eq!(
            Frame::rgb565(&[0; 3], SOURCE, 2).map(|_| ()),
            Err(DisplayError::TruncatedSource)
        );
        assert_eq!(
            Frame::indexed8(&[0; 4], SOURCE, 2, &[]).map(|_| ()),
            Err(DisplayError::EmptyPalette)
        );
        assert!(Frame::xrgb8888(&[0; 4], SOURCE, 2).is_ok());
    }

    #[test]
    fn rgb565_conversion_matches_primary_colors() {
        assert_eq!(rgb565_to_xrgb8888(0x0000), 0xff00_0000);
        assert_eq!(rgb565_to_xrgb8888(0xf800), 0xffff_0000);
        assert_eq!(rgb565_to_xrgb8888(0x07e0), 0xff00_ff00);
        assert_eq!(rgb565_to_xrgb8888(0x001f), 0xff00_00ff);
        assert_eq!(rgb565_to_xrgb8888(0xffff), 0xffff_ffff);
    }

    #[test]
    fn scale_plan_nearest_scales_and_normalizes_xrgb() {
        let source = [0x0011_2233, 0x0044_5566, 0x0077_8899, 0x00aa_bbcc];
        let frame = Frame::xrgb8888(&source, SOURCE, 2).expect("fixed frame is valid");
        let plan = ScalePlan::new(SOURCE, TARGET);
        let mut target = [0_u32; 16];
        assert_eq!(plan.blit(frame, &mut target), Ok(()));
        assert_eq!(
            target,
            [
                0xff11_2233,
                0xff11_2233,
                0xff44_5566,
                0xff44_5566,
                0xff11_2233,
                0xff11_2233,
                0xff44_5566,
                0xff44_5566,
                0xff77_8899,
                0xff77_8899,
                0xffaa_bbcc,
                0xffaa_bbcc,
                0xff77_8899,
                0xff77_8899,
                0xffaa_bbcc,
                0xffaa_bbcc,
            ]
        );
    }

    #[test]
    fn indexed_pixels_outside_the_palette_render_black() {
        let pixels = [0_u8, 1, 2, 255];
        let palette = [0x0012_3456, 0x0065_4321];
        let frame = Frame::indexed8(&pixels, SOURCE, 2, &palette).expect("fixed frame is valid");
        let plan = ScalePlan::new(SOURCE, SOURCE);
        let mut target = [0_u32; 4];
        assert_eq!(plan.blit(frame, &mut target), Ok(()));
        assert_eq!(target, [0xff12_3456, 0xff65_4321, 0xff00_0000, 0xff00_0000]);
    }

    #[test]
    fn scale_plan_rejects_changed_sources_and_wrong_slots() {
        let frame = Frame::rgb565(&[0; 4], SOURCE, 2).expect("fixed frame is valid");
        let other = Dimensions::new(1, 4).expect("fixed dimensions are valid");
        let plan = ScalePlan::new(other, TARGET);
        assert_eq!(
            plan.blit(frame, &mut [0; 16]),
            Err(DisplayError::SourceDimensionsChanged)
        );
        let plan = ScalePlan::new(SOURCE, TARGET);
        assert_eq!(
            plan.blit(frame, &mut [0; 15]),
            Err(DisplayError::TargetLength)
        );
    }

    #[test]
    fn scale_plan_updates_its_source_without_changing_the_target() {
        let other = Dimensions::new(1, 4).expect("fixed dimensions are valid");
        let source = [0xf800_u16, 0x07e0, 0x001f, 0xffff];
        let frame = Frame::rgb565(&source, other, 1).expect("changed frame is valid");
        let mut plan = ScalePlan::new(SOURCE, TARGET);

        assert_eq!(plan.source(), SOURCE);
        assert_eq!(plan.target(), TARGET);
        assert!(plan.update_source(other));
        assert!(!plan.update_source(other));
        assert_eq!(plan.source(), other);
        assert_eq!(plan.target(), TARGET);

        let mut target = [0_u32; 16];
        assert_eq!(plan.blit(frame, &mut target), Ok(()));
        for row in target.chunks_exact(4) {
            assert!(
                row.first()
                    .is_some_and(|first| row.iter().all(|pixel| pixel == first))
            );
        }
        assert_eq!(target[0], 0xffff_0000);
        assert_eq!(target[4], 0xff00_ff00);
        assert_eq!(target[8], 0xff00_00ff);
        assert_eq!(target[12], 0xffff_ffff);
    }

    #[test]
    fn slots_are_not_reused_before_compositor_release() {
        let mut slots = PresentationSlots::new();
        let first = slots.acquire().expect("slot one is available");
        let second = slots.acquire().expect("slot two is available");
        let third = slots.acquire().expect("slot three is available");
        assert_eq!(slots.acquire(), None);
        assert_eq!(slots.submit(first), Ok(()));
        assert_eq!(slots.submit(second), Ok(()));
        assert_eq!(slots.submit(third), Ok(()));
        assert_eq!(slots.acquire(), None);
        assert_eq!(slots.release(second), Ok(()));
        assert_eq!(slots.acquire(), Some(second));
    }

    #[test]
    fn slot_transitions_reject_double_submit_release_and_cancel() {
        let mut slots = PresentationSlots::new();
        let slot = slots.acquire().expect("slot is available");
        assert_eq!(slots.cancel(slot), Ok(()));
        assert_eq!(slots.cancel(slot), Err(SlotError::InvalidOwnership));
        let slot = slots.acquire().expect("slot is available again");
        assert_eq!(slots.submit(slot), Ok(()));
        assert_eq!(slots.submit(slot), Err(SlotError::InvalidOwnership));
        assert_eq!(slots.release(slot), Ok(()));
        assert_eq!(slots.release(slot), Err(SlotError::InvalidOwnership));
    }
}
