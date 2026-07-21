//! Allocation-stable RGB565 renderer for the native chiptune player.

use std::fmt::{self, Write as _};

use retro_deck_audio::Volume;

use super::PlaybackMode;

/// Native player canvas width before the platform applies its exact scale.
pub const CANVAS_WIDTH: usize = 624;
/// Native player canvas height before the platform applies its exact scale.
pub const CANVAS_HEIGHT: usize = 224;
const PIXELS: usize = CANVAS_WIDTH * CANVAS_HEIGHT;

const BACKGROUND: u16 = rgb888_to_rgb565(0x00_00_00);
const ORANGE: u16 = rgb888_to_rgb565(0xfe_6c_27);
const ACTIVE: u16 = rgb888_to_rgb565(0x4d_37_2d);
const TEXT: u16 = rgb888_to_rgb565(0xff_ff_ff);
const GREEN: u16 = rgb888_to_rgb565(0x87_af_87);
const RED: u16 = rgb888_to_rgb565(0xaf_87_87);
const MUTED: u16 = rgb888_to_rgb565(0x96_96_96);
const INDICATOR: u16 = rgb888_to_rgb565(0x6c_6c_6c);

const CLOSE_BUTTON: Rect = Rect::new(554, 3, 62, 34);
const PLAYBACK_MODE_BUTTON: Rect = Rect::new(113, 177, 92, 34);
const PREVIOUS_FILE_BUTTON: Rect = Rect::new(215, 177, 92, 34);
const PAUSE_BUTTON: Rect = Rect::new(317, 177, 92, 34);
const NEXT_FILE_BUTTON: Rect = Rect::new(419, 177, 92, 34);

/// Metadata and decoded waveform for one playable track.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackView<'a> {
    /// Preferred song title, or a filename-derived fallback.
    pub title: &'a str,
    /// Optional game and author line.
    pub subtitle: &'a str,
    /// Decoder-reported source system or format.
    pub system: &'a str,
    /// Zero-based catalog position.
    pub file_index: usize,
    /// Total files retained by the bounded catalog.
    pub file_count: usize,
    /// Zero-based subsong position.
    pub track_index: usize,
    /// Total subsongs exposed by the decoder.
    pub track_count: usize,
    /// Current playback position in milliseconds.
    pub position_milliseconds: u64,
    /// Known track duration in milliseconds.
    pub length_milliseconds: Option<u64>,
    /// Most recent stereo decoder block used by the visualizer.
    pub waveform: &'a [[i16; 2]],
}

/// Content area shown by the player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerContent<'a> {
    /// No decoder opened; show one bounded diagnostic line.
    Empty {
        /// Human-readable reason or directory hint.
        status: &'a str,
    },
    /// A decoder is ready and supplies current metadata.
    Track(TrackView<'a>),
}

/// Complete device-independent player view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChiptuneView<'a> {
    /// Track or empty-state content.
    pub content: PlayerContent<'a>,
    /// Whether the player currently suppresses decoding and audio.
    pub paused: bool,
    /// Current loop or shuffle selection.
    pub playback_mode: PlaybackMode,
    /// Current user gain.
    pub volume: Volume,
}

/// Fixed-size rendered chiptune frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiptuneFrame {
    pixels: Vec<u16>,
}

impl ChiptuneFrame {
    /// Allocate and render one complete player frame.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when the fixed canvas cannot be allocated.
    pub fn render(view: ChiptuneView<'_>) -> Result<Self, RenderError> {
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(PIXELS).map_err(|_| RenderError)?;
        pixels.resize(PIXELS, BACKGROUND);
        let mut frame = Self { pixels };
        frame.redraw(view);
        Ok(frame)
    }

    /// Redraw into the existing fixed allocation.
    pub fn redraw(&mut self, view: ChiptuneView<'_>) {
        self.pixels.fill(BACKGROUND);
        let mut canvas = Canvas::new(&mut self.pixels);
        draw_player(&mut canvas, view);
    }

    /// Borrow tightly packed native-endian RGB565 pixels.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref Vec to a slice"
    )]
    pub fn pixels(&self) -> &[u16] {
        &self.pixels
    }

    /// Fixed row stride in bytes.
    #[must_use]
    pub const fn stride_bytes() -> usize {
        CANVAS_WIDTH * size_of::<u16>()
    }

    /// Read one pixel for preview and regression tooling.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Option<u16> {
        if x >= CANVAS_WIDTH || y >= CANVAS_HEIGHT {
            return None;
        }
        self.pixels
            .get(y.checked_mul(CANVAS_WIDTH)?.checked_add(x)?)
            .copied()
    }
}

/// Fixed canvas allocation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cannot allocate the chiptune player frame")
    }
}

impl std::error::Error for RenderError {}

fn draw_player(canvas: &mut Canvas<'_>, view: ChiptuneView<'_>) {
    canvas.draw_panel(Rect::new(236, 4, 152, 29), ACTIVE, ORANGE, 2);
    canvas.draw_centered_text(12, b"CHIPTUNES", 1, TEXT);
    canvas.draw_close_icon(CLOSE_BUTTON, TEXT);
    let mut volume = TextBuffer::<24>::new();
    if view.volume.muted() {
        volume.push_bytes(b"VOL OFF");
    } else {
        let _ignored = write!(volume, "VOL {}", view.volume.percent());
    }
    canvas.draw_text(
        8,
        14,
        volume.as_bytes(),
        1,
        if view.volume.muted() { RED } else { GREEN },
    );

    match view.content {
        PlayerContent::Empty { status } => draw_empty(canvas, status),
        PlayerContent::Track(track) => draw_track(canvas, track),
    }

    canvas.draw_playback_mode_icon(PLAYBACK_MODE_BUTTON, view.playback_mode, TEXT);
    canvas.draw_previous_icon(PREVIOUS_FILE_BUTTON, TEXT);
    canvas.draw_pause_icon(PAUSE_BUTTON, view.paused, TEXT);
    canvas.draw_next_icon(NEXT_FILE_BUTTON, TEXT);
}

fn draw_empty(canvas: &mut Canvas<'_>, status: &str) {
    canvas.draw_panel(Rect::new(78, 42, 468, 120), ACTIVE, ORANGE, 2);
    canvas.draw_centered_text(72, b"NO CHIPTUNES FOUND", 2, TEXT);
    let status = sanitized::<96>(status, 72);
    canvas.draw_centered_text(103, status.as_bytes(), 1, MUTED);
    canvas.draw_centered_text(
        126,
        b"AY GBS GYM HES KSS NSF NSFE OGG SAP SPC VGM VGZ",
        1,
        TEXT,
    );
}

fn draw_track(canvas: &mut Canvas<'_>, track: TrackView<'_>) {
    canvas.draw_panel(Rect::new(78, 42, 468, 120), ACTIVE, ORANGE, 2);
    let title = sanitized::<96>(track.title, 45);
    let subtitle = sanitized::<96>(track.subtitle, 72);
    canvas.draw_centered_text(50, title.as_bytes(), 2, TEXT);
    canvas.draw_centered_text(70, subtitle.as_bytes(), 1, MUTED);

    canvas.fill_rect(Rect::new(96, 84, 432, 44), BACKGROUND);
    canvas.fill_rect(Rect::new(96, 105, 432, 1), MUTED);
    if !track.waveform.is_empty() {
        for x in 0_usize..432 {
            let frame = x.saturating_mul(track.waveform.len()) / 432;
            let Some(samples) = track.waveform.get(frame) else {
                continue;
            };
            let mixed = (i32::from(samples[0]) + i32::from(samples[1])) / 2;
            let height = (mixed.unsigned_abs() / 1_050).min(20) as usize;
            let height = height.max(1);
            let y = if mixed < 0 {
                106
            } else {
                105_usize.saturating_sub(height)
            };
            canvas.fill_rect(Rect::new(96 + x, y, 1, height), ORANGE);
        }
    }

    canvas.fill_rect(Rect::new(96, 134, 432, 3), BACKGROUND);
    if let Some(length) = track.length_milliseconds.filter(|length| *length > 0) {
        let position = track.position_milliseconds.min(length);
        let progress = position.saturating_mul(432) / length;
        let progress = usize::try_from(progress).unwrap_or(432).min(432);
        canvas.fill_rect(Rect::new(96, 134, progress, 3), GREEN);
    }

    let position = formatted_time(track.position_milliseconds);
    canvas.draw_text(96, 143, position.as_bytes(), 1, TEXT);
    let end = track
        .length_milliseconds
        .map_or_else(|| TextBuffer::from_bytes(b"--:--"), formatted_time);
    canvas.draw_text(
        528_usize.saturating_sub(text_width(end.len(), 1)),
        143,
        end.as_bytes(),
        1,
        TEXT,
    );

    let system = sanitized::<32>(track.system, 18);
    let mut details = TextBuffer::<128>::new();
    details.push_bytes(system.as_bytes());
    let _ignored = write!(
        details,
        "  FILE {}/{}  TRACK {}/{}",
        track.file_index.saturating_add(1),
        track.file_count,
        track.track_index.saturating_add(1),
        track.track_count
    );
    details.clip_with_ellipsis(56);
    canvas.draw_centered_text(143, details.as_bytes(), 1, MUTED);
    canvas.draw_file_indicators(track.file_index, track.file_count);
}

fn formatted_time(milliseconds: u64) -> TextBuffer<32> {
    let seconds = milliseconds / 1_000;
    let mut output = TextBuffer::new();
    let _ignored = write!(output, "{}:{:02}", seconds / 60, seconds % 60);
    output
}

#[derive(Debug)]
struct Canvas<'pixels> {
    pixels: &'pixels mut [u16],
}

impl<'pixels> Canvas<'pixels> {
    const fn new(pixels: &'pixels mut [u16]) -> Self {
        Self { pixels }
    }

    fn fill_rect(&mut self, rect: Rect, color: u16) {
        let left = rect.x.min(CANVAS_WIDTH);
        let top = rect.y.min(CANVAS_HEIGHT);
        let right = rect.x.saturating_add(rect.width).min(CANVAS_WIDTH);
        let bottom = rect.y.saturating_add(rect.height).min(CANVAS_HEIGHT);
        if left >= right || top >= bottom {
            return;
        }
        for y in top..bottom {
            let start = y.saturating_mul(CANVAS_WIDTH).saturating_add(left);
            let end = y.saturating_mul(CANVAS_WIDTH).saturating_add(right);
            if let Some(row) = self.pixels.get_mut(start..end) {
                row.fill(color);
            }
        }
    }

    fn stroke_rect(&mut self, rect: Rect, thickness: usize, color: u16) {
        self.fill_rect(Rect::new(rect.x, rect.y, rect.width, thickness), color);
        self.fill_rect(
            Rect::new(
                rect.x,
                rect.y.saturating_add(rect.height.saturating_sub(thickness)),
                rect.width,
                thickness,
            ),
            color,
        );
        self.fill_rect(Rect::new(rect.x, rect.y, thickness, rect.height), color);
        self.fill_rect(
            Rect::new(
                rect.x.saturating_add(rect.width.saturating_sub(thickness)),
                rect.y,
                thickness,
                rect.height,
            ),
            color,
        );
    }

    fn fill_pixel_cut_rect(&mut self, rect: Rect, cut: usize, color: u16) {
        if rect.width <= cut.saturating_mul(2) || rect.height <= cut.saturating_mul(2) {
            return;
        }
        self.fill_rect(
            Rect::new(
                rect.x.saturating_add(cut),
                rect.y,
                rect.width.saturating_sub(cut.saturating_mul(2)),
                rect.height,
            ),
            color,
        );
        self.fill_rect(
            Rect::new(
                rect.x,
                rect.y.saturating_add(cut),
                rect.width,
                rect.height.saturating_sub(cut.saturating_mul(2)),
            ),
            color,
        );
    }

    fn draw_panel(&mut self, rect: Rect, fill: u16, border: u16, thickness: usize) {
        self.fill_pixel_cut_rect(rect, thickness, border);
        self.fill_pixel_cut_rect(
            Rect::new(
                rect.x.saturating_add(thickness),
                rect.y.saturating_add(thickness),
                rect.width.saturating_sub(thickness.saturating_mul(2)),
                rect.height.saturating_sub(thickness.saturating_mul(2)),
            ),
            thickness,
            fill,
        );
    }

    fn draw_text(&mut self, x: usize, y: usize, text: &[u8], scale: usize, color: u16) {
        for (character_index, character) in text.iter().copied().enumerate() {
            let rows = glyph_rows(character);
            for (row, bits) in rows.into_iter().enumerate() {
                for column in 0..5 {
                    let shift = 4_usize.saturating_sub(column);
                    if bits & (1_u8 << shift) == 0 {
                        continue;
                    }
                    self.fill_rect(
                        Rect::new(
                            x.saturating_add(
                                character_index.saturating_mul(6).saturating_mul(scale),
                            )
                            .saturating_add(column.saturating_mul(scale)),
                            y.saturating_add(row.saturating_mul(scale)),
                            scale,
                            scale,
                        ),
                        color,
                    );
                }
            }
        }
    }

    fn draw_centered_text(&mut self, y: usize, text: &[u8], scale: usize, color: u16) {
        let x = CANVAS_WIDTH.saturating_sub(text_width(text.len(), scale)) / 2;
        self.draw_text(x, y, text, scale, color);
    }

    fn draw_close_icon(&mut self, bounds: Rect, color: u16) {
        let center_x = bounds.x + bounds.width / 2;
        let center_y = bounds.y + bounds.height / 2;
        for offset in (0..=16).step_by(2) {
            let signed = isize::try_from(offset).unwrap_or_default() - 8;
            let first_x = center_x.saturating_add_signed(signed);
            let first_y = center_y.saturating_add_signed(signed);
            let second_y = center_y.saturating_add_signed(-signed);
            self.fill_rect(Rect::new(first_x, first_y, 2, 2), color);
            self.fill_rect(Rect::new(first_x, second_y, 2, 2), color);
        }
    }

    fn draw_line(
        &mut self,
        mut from_x: isize,
        mut from_y: isize,
        to_x: isize,
        to_y: isize,
        color: u16,
    ) {
        let delta_x = (to_x - from_x).abs();
        let step_x = if from_x < to_x { 1 } else { -1 };
        let delta_y = -(to_y - from_y).abs();
        let step_y = if from_y < to_y { 1 } else { -1 };
        let mut error = delta_x + delta_y;
        loop {
            if let (Ok(x), Ok(y)) = (usize::try_from(from_x), usize::try_from(from_y)) {
                self.fill_rect(Rect::new(x, y, 1, 1), color);
            }
            if from_x == to_x && from_y == to_y {
                break;
            }
            let doubled = error.saturating_mul(2);
            if doubled >= delta_y {
                error += delta_y;
                from_x += step_x;
            }
            if doubled <= delta_x {
                error += delta_x;
                from_y += step_y;
            }
        }
    }

    fn draw_arrow_head(&mut self, point_x: isize, point_y: isize, right: bool, color: u16) {
        let direction = if right { -1 } else { 1 };
        self.draw_line(
            point_x,
            point_y,
            point_x + direction * 4,
            point_y - 4,
            color,
        );
        self.draw_line(
            point_x,
            point_y,
            point_x + direction * 4,
            point_y + 4,
            color,
        );
    }

    fn draw_transport_triangle(
        &mut self,
        center_x: usize,
        center_y: usize,
        right: bool,
        color: u16,
    ) {
        for row in (-6_isize..=6).step_by(2) {
            let width = usize::try_from(14 - row.abs() * 2).unwrap_or_default();
            let left = if right {
                center_x.saturating_sub(6)
            } else {
                center_x.saturating_add(6).saturating_sub(width)
            };
            self.fill_rect(
                Rect::new(left, center_y.saturating_add_signed(row), width, 2),
                color,
            );
        }
    }

    fn draw_previous_icon(&mut self, rect: Rect, color: u16) {
        let center_x = rect.x + rect.width / 2;
        let center_y = rect.y + rect.height / 2;
        self.fill_rect(
            Rect::new(
                center_x.saturating_sub(10),
                center_y.saturating_sub(7),
                2,
                14,
            ),
            color,
        );
        self.draw_transport_triangle(center_x + 1, center_y, false, color);
    }

    fn draw_next_icon(&mut self, rect: Rect, color: u16) {
        let center_x = rect.x + rect.width / 2;
        let center_y = rect.y + rect.height / 2;
        self.draw_transport_triangle(center_x.saturating_sub(1), center_y, true, color);
        self.fill_rect(
            Rect::new(center_x + 8, center_y.saturating_sub(7), 2, 14),
            color,
        );
    }

    fn draw_pause_icon(&mut self, rect: Rect, paused: bool, color: u16) {
        let center_x = rect.x + rect.width / 2;
        let center_y = rect.y + rect.height / 2;
        if paused {
            self.draw_transport_triangle(center_x.saturating_sub(1), center_y, true, color);
        } else {
            self.fill_rect(
                Rect::new(
                    center_x.saturating_sub(5),
                    center_y.saturating_sub(7),
                    3,
                    14,
                ),
                color,
            );
            self.fill_rect(
                Rect::new(center_x + 2, center_y.saturating_sub(7), 3, 14),
                color,
            );
        }
    }

    fn draw_loop_icon(&mut self, rect: Rect, one: bool, color: u16) {
        let center_x = isize::try_from(rect.x + rect.width / 2).unwrap_or_default();
        let center_y = isize::try_from(rect.y + rect.height / 2).unwrap_or_default();
        self.draw_line(
            center_x - 11,
            center_y - 5,
            center_x + 9,
            center_y - 5,
            color,
        );
        self.draw_arrow_head(center_x + 11, center_y - 5, true, color);
        self.draw_line(
            center_x + 11,
            center_y + 5,
            center_x - 9,
            center_y + 5,
            color,
        );
        self.draw_arrow_head(center_x - 11, center_y + 5, false, color);
        if one {
            self.draw_text(
                usize::try_from(center_x - 2).unwrap_or_default(),
                usize::try_from(center_y - 3).unwrap_or_default(),
                b"1",
                1,
                color,
            );
        }
    }

    fn draw_shuffle_icon(&mut self, rect: Rect, color: u16) {
        let center_x = isize::try_from(rect.x + rect.width / 2).unwrap_or_default();
        let center_y = isize::try_from(rect.y + rect.height / 2).unwrap_or_default();
        self.draw_line(
            center_x - 12,
            center_y - 5,
            center_x - 7,
            center_y - 5,
            color,
        );
        self.draw_line(
            center_x - 7,
            center_y - 5,
            center_x + 6,
            center_y + 5,
            color,
        );
        self.draw_line(
            center_x - 12,
            center_y + 5,
            center_x - 7,
            center_y + 5,
            color,
        );
        self.draw_line(
            center_x - 7,
            center_y + 5,
            center_x + 6,
            center_y - 5,
            color,
        );
        self.draw_line(
            center_x + 6,
            center_y - 5,
            center_x + 10,
            center_y - 5,
            color,
        );
        self.draw_line(
            center_x + 6,
            center_y + 5,
            center_x + 10,
            center_y + 5,
            color,
        );
        self.draw_arrow_head(center_x + 12, center_y - 5, true, color);
        self.draw_arrow_head(center_x + 12, center_y + 5, true, color);
    }

    fn draw_playback_mode_icon(&mut self, rect: Rect, mode: PlaybackMode, color: u16) {
        match mode {
            PlaybackMode::LoopAll => self.draw_loop_icon(rect, false, color),
            PlaybackMode::LoopOne => self.draw_loop_icon(rect, true, color),
            PlaybackMode::Shuffle => self.draw_shuffle_icon(rect, color),
        }
    }

    fn draw_file_indicators(&mut self, file_index: usize, file_count: usize) {
        if file_count == 0 {
            return;
        }
        let visible = file_count.min(40);
        let width = 6;
        let gap = 4;
        let row_width = visible
            .saturating_mul(width)
            .saturating_add(visible.saturating_sub(1).saturating_mul(gap));
        let mut x = CANVAS_WIDTH.saturating_sub(row_width) / 2;
        let mut first = file_index.saturating_sub(visible / 2);
        first = first.min(file_count.saturating_sub(visible));
        for index in 0..visible {
            let color = if first.saturating_add(index) == file_index {
                ORANGE
            } else {
                INDICATOR
            };
            self.stroke_rect(Rect::new(x, 166, width, 4), 1, color);
            x = x.saturating_add(width + gap);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rect {
    const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextBuffer<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    len: usize,
}

impl<const CAPACITY: usize> TextBuffer<CAPACITY> {
    const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            len: 0,
        }
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        let mut output = Self::new();
        output.push_bytes(bytes);
        output
    }

    const fn len(&self) -> usize {
        self.len
    }

    fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or_default()
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        let remaining = CAPACITY.saturating_sub(self.len);
        let amount = bytes.len().min(remaining);
        if let (Some(source), Some(destination)) = (
            bytes.get(..amount),
            self.bytes
                .get_mut(self.len..self.len.saturating_add(amount)),
        ) {
            destination.copy_from_slice(source);
            self.len = self.len.saturating_add(amount);
        }
    }

    fn clip_with_ellipsis(&mut self, maximum: usize) {
        let maximum = maximum.min(CAPACITY);
        if self.len <= maximum {
            return;
        }
        if maximum < 3 {
            self.len = maximum;
            return;
        }
        self.len = maximum - 3;
        self.push_bytes(b"...");
    }
}

impl<const CAPACITY: usize> fmt::Write for TextBuffer<CAPACITY> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.push_bytes(value.as_bytes());
        Ok(())
    }
}

fn sanitized<const CAPACITY: usize>(input: &str, maximum: usize) -> TextBuffer<CAPACITY> {
    let maximum = maximum.min(CAPACITY);
    let mut output = TextBuffer::new();
    let mut overflow = false;
    for byte in input.bytes() {
        let normalized = match byte {
            b'a'..=b'z' => Some(byte.to_ascii_uppercase()),
            b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'.' | b':' | b'-' | b'+' | b'/' => Some(byte),
            b'_' | b'\t' => Some(b' '),
            _ => None,
        };
        let Some(normalized) = normalized else {
            continue;
        };
        if output.len < maximum {
            output.push_bytes(&[normalized]);
        } else {
            overflow = true;
        }
    }
    if overflow && maximum >= 3 {
        output.len = maximum - 3;
        output.push_bytes(b"...");
    }
    output
}

const fn text_width(characters: usize, scale: usize) -> usize {
    let trailing = if characters == 0 { 0 } else { 1 };
    characters
        .saturating_mul(6)
        .saturating_sub(trailing)
        .saturating_mul(scale)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the channel masks prove the packed value fits in u16"
)]
const fn rgb888_to_rgb565(rgb: u32) -> u16 {
    let red = (rgb >> 19) & 0x1f;
    let green = (rgb >> 10) & 0x3f;
    let blue = (rgb >> 3) & 0x1f;
    ((red << 11) | (green << 5) | blue) as u16
}

const fn glyph_rows(character: u8) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        b'A' => [14, 17, 17, 31, 17, 17, 17],
        b'B' => [30, 17, 17, 30, 17, 17, 30],
        b'C' => [14, 17, 16, 16, 16, 17, 14],
        b'D' => [30, 17, 17, 17, 17, 17, 30],
        b'E' => [31, 16, 16, 30, 16, 16, 31],
        b'F' => [31, 16, 16, 30, 16, 16, 16],
        b'G' => [14, 17, 16, 23, 17, 17, 15],
        b'H' => [17, 17, 17, 31, 17, 17, 17],
        b'I' => [14, 4, 4, 4, 4, 4, 14],
        b'J' => [7, 2, 2, 2, 18, 18, 12],
        b'K' => [17, 18, 20, 24, 20, 18, 17],
        b'L' => [16, 16, 16, 16, 16, 16, 31],
        b'M' => [17, 27, 21, 21, 17, 17, 17],
        b'N' => [17, 25, 21, 19, 17, 17, 17],
        b'O' => [14, 17, 17, 17, 17, 17, 14],
        b'P' => [30, 17, 17, 30, 16, 16, 16],
        b'Q' => [14, 17, 17, 17, 21, 18, 13],
        b'R' => [30, 17, 17, 30, 20, 18, 17],
        b'S' => [15, 16, 16, 14, 1, 1, 30],
        b'T' => [31, 4, 4, 4, 4, 4, 4],
        b'U' => [17, 17, 17, 17, 17, 17, 14],
        b'V' => [17, 17, 17, 17, 17, 10, 4],
        b'W' => [17, 17, 17, 17, 21, 21, 10],
        b'X' => [17, 17, 10, 4, 10, 17, 17],
        b'Y' => [17, 17, 10, 4, 4, 4, 4],
        b'Z' => [31, 1, 2, 4, 8, 16, 31],
        b'0' => [14, 17, 19, 21, 25, 17, 14],
        b'1' => [4, 12, 4, 4, 4, 4, 14],
        b'2' => [14, 17, 1, 2, 4, 8, 31],
        b'3' => [30, 1, 1, 14, 1, 1, 30],
        b'4' => [2, 6, 10, 18, 31, 2, 2],
        b'5' => [31, 16, 16, 30, 1, 1, 30],
        b'6' => [14, 16, 16, 30, 17, 17, 14],
        b'7' => [31, 1, 2, 4, 8, 8, 8],
        b'8' => [14, 17, 17, 14, 17, 17, 14],
        b'9' => [14, 17, 17, 15, 1, 1, 14],
        b'.' => [0, 0, 0, 0, 0, 6, 6],
        b':' => [0, 6, 6, 0, 6, 6, 0],
        b'-' => [0, 0, 0, 31, 0, 0, 0],
        b'+' => [0, 4, 4, 31, 4, 4, 0],
        b'/' => [1, 2, 2, 4, 8, 8, 16],
        b' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAVEFORM: [[i16; 2]; 4] = [
        [0, 0],
        [12_000, 12_000],
        [-18_000, -18_000],
        [4_000, -4_000],
    ];

    const fn volume(percent: u8) -> Volume {
        match Volume::new(percent) {
            Some(volume) => volume,
            None => Volume::MUTED,
        }
    }

    fn track() -> TrackView<'static> {
        TrackView {
            title: "Opening Theme",
            subtitle: "Example Game - Composer",
            system: "NES",
            file_index: 4,
            file_count: 10,
            track_index: 1,
            track_count: 3,
            position_milliseconds: 61_000,
            length_milliseconds: Some(122_000),
            waveform: &WAVEFORM,
        }
    }

    fn view(content: PlayerContent<'static>) -> ChiptuneView<'static> {
        ChiptuneView {
            content,
            paused: false,
            playback_mode: PlaybackMode::LoopAll,
            volume: volume(42),
        }
    }

    #[test]
    fn renders_one_fixed_complete_frame() {
        let frame = ChiptuneFrame::render(view(PlayerContent::Track(track())))
            .expect("chiptune frame allocation succeeds");
        assert_eq!(frame.pixels().len(), PIXELS);
        assert_eq!(ChiptuneFrame::stride_bytes(), CANVAS_WIDTH * 2);
        assert_eq!(frame.pixel(0, 0), Some(BACKGROUND));
        assert_eq!(frame.pixel(236, 6), Some(ORANGE));
        assert_eq!(frame.pixel(CANVAS_WIDTH, 0), None);
    }

    #[test]
    fn waveform_progress_and_file_indicator_are_visible() {
        let frame = ChiptuneFrame::render(view(PlayerContent::Track(track())))
            .expect("chiptune frame allocation succeeds");
        assert!(frame.pixels().contains(&GREEN));
        assert!(frame.pixels().contains(&ORANGE));
        assert_eq!(frame.pixel(96 + 215, 135), Some(GREEN));
    }

    #[test]
    fn empty_and_muted_view_uses_the_red_volume_state() {
        let mut empty = view(PlayerContent::Empty {
            status: "Add music to /mnt/data/chiptunes",
        });
        empty.volume = Volume::MUTED;
        let frame = ChiptuneFrame::render(empty).expect("chiptune frame allocation succeeds");
        assert!(frame.pixels().contains(&RED));
        assert!(frame.pixels().contains(&MUTED));
    }

    #[test]
    fn redraw_reuses_the_fixed_pixel_allocation() {
        let mut frame = ChiptuneFrame::render(view(PlayerContent::Track(track())))
            .expect("chiptune frame allocation succeeds");
        let before = frame.pixels().as_ptr();
        let mut paused = view(PlayerContent::Track(track()));
        paused.paused = true;
        paused.playback_mode = PlaybackMode::Shuffle;
        frame.redraw(paused);
        assert_eq!(before, frame.pixels().as_ptr());
    }

    #[test]
    fn metadata_sanitization_is_bounded_and_marks_clipping() {
        let clean = sanitized::<16>("abc_123!ž", 16);
        assert_eq!(clean.as_bytes(), b"ABC 123");
        let clipped = sanitized::<16>("abcdefghijklmnop", 8);
        assert_eq!(clipped.as_bytes(), b"ABCDE...");
    }
}
