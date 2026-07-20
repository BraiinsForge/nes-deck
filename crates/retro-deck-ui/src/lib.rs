//! Device-independent, allocation-free RGB565 drawing primitives.

use std::fmt;

/// Width of one bitmap glyph before scaling.
pub const GLYPH_WIDTH: usize = 5;
/// Height of one bitmap glyph before scaling.
pub const GLYPH_HEIGHT: usize = 7;
/// Horizontal glyph advance before scaling.
pub const GLYPH_ADVANCE: usize = 6;

/// One unsigned clipping and hit-test rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    /// Left coordinate.
    pub x: usize,
    /// Top coordinate.
    pub y: usize,
    /// Horizontal extent.
    pub width: usize,
    /// Vertical extent.
    pub height: usize,
}

impl Rect {
    /// Construct one rectangle.
    #[must_use]
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether one point lies in the half-open rectangle.
    #[must_use]
    pub const fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width)
            && y < self.y.saturating_add(self.height)
    }
}

/// Mutable tightly packed RGB565 canvas.
#[derive(Debug)]
pub struct Canvas<'pixels> {
    width: usize,
    height: usize,
    pixels: &'pixels mut [u16],
}

impl<'pixels> Canvas<'pixels> {
    /// Validate and borrow a complete tightly packed canvas.
    #[must_use]
    pub fn new(pixels: &'pixels mut [u16], width: usize, height: usize) -> Option<Self> {
        let expected = width.checked_mul(height)?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Canvas width.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Canvas height.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Fill the complete canvas.
    pub fn clear(&mut self, color: u16) {
        self.pixels.fill(color);
    }

    /// Fill one rectangle after clipping it to the canvas.
    pub fn fill_rect(&mut self, rect: Rect, color: u16) {
        let left = rect.x.min(self.width);
        let top = rect.y.min(self.height);
        let right = rect.x.saturating_add(rect.width).min(self.width);
        let bottom = rect.y.saturating_add(rect.height).min(self.height);
        if left >= right || top >= bottom {
            return;
        }
        for y in top..bottom {
            let start = y.saturating_mul(self.width).saturating_add(left);
            let end = y.saturating_mul(self.width).saturating_add(right);
            if let Some(row) = self.pixels.get_mut(start..end) {
                row.fill(color);
            }
        }
    }

    /// Draw an axis-aligned rectangular outline.
    pub fn stroke_rect(&mut self, rect: Rect, thickness: usize, color: u16) {
        if thickness == 0 {
            return;
        }
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

    /// Fill a rectangle with square pixel-cut corners.
    pub fn fill_pixel_cut_rect(&mut self, rect: Rect, cut: usize, color: u16) {
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

    /// Draw a filled pixel-cut panel and its border.
    pub fn draw_panel(&mut self, rect: Rect, fill: u16, border: u16, thickness: usize) {
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

    /// Draw UTF-8 text with ASCII glyphs and one `?` per non-ASCII character.
    pub fn draw_text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: u16) {
        for (character_index, character) in text.chars().enumerate() {
            let character = if character.is_ascii() {
                u8::try_from(u32::from(character)).unwrap_or(b'?')
            } else {
                b'?'
            };
            self.draw_glyph(x, y, character_index, character, scale, color);
        }
    }

    /// Draw already normalized ASCII bytes.
    pub fn draw_bytes(&mut self, x: usize, y: usize, text: &[u8], scale: usize, color: u16) {
        for (character_index, character) in text.iter().copied().enumerate() {
            self.draw_glyph(x, y, character_index, character, scale, color);
        }
    }

    /// Center text both horizontally and vertically within `bounds`.
    pub fn draw_centered_text(&mut self, bounds: Rect, text: &str, scale: usize, color: u16) {
        let width = text_width(text, scale);
        let height = GLYPH_HEIGHT.saturating_mul(scale);
        let x = bounds
            .x
            .saturating_add(bounds.width.saturating_sub(width) / 2);
        let y = bounds
            .y
            .saturating_add(bounds.height.saturating_sub(height) / 2);
        self.draw_text(x, y, text, scale, color);
    }

    /// Draw a one-pixel Bresenham line, clipped by pixel writes.
    pub fn draw_line(
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
                self.set_pixel(x, y, color);
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

    /// Write one pixel when it lies inside the canvas.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u16) {
        if x >= self.width || y >= self.height {
            return;
        }
        let Some(offset) = y.checked_mul(self.width).and_then(|row| row.checked_add(x)) else {
            return;
        };
        if let Some(pixel) = self.pixels.get_mut(offset) {
            *pixel = color;
        }
    }

    /// Read one pixel for previews and tests.
    #[must_use]
    pub fn pixel(&self, x: usize, y: usize) -> Option<u16> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels
            .get(y.checked_mul(self.width)?.checked_add(x)?)
            .copied()
    }

    fn draw_glyph(
        &mut self,
        x: usize,
        y: usize,
        character_index: usize,
        character: u8,
        scale: usize,
        color: u16,
    ) {
        if scale == 0 {
            return;
        }
        let rows = glyph_rows(character);
        for (row, bits) in rows.into_iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                let shift = GLYPH_WIDTH.saturating_sub(1).saturating_sub(column);
                if bits & (1_u8 << shift) == 0 {
                    continue;
                }
                self.fill_rect(
                    Rect::new(
                        x.saturating_add(
                            character_index
                                .saturating_mul(GLYPH_ADVANCE)
                                .saturating_mul(scale),
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

/// Rendered width of UTF-8 text under the one-glyph-per-character policy.
#[must_use]
pub fn text_width(text: &str, scale: usize) -> usize {
    text_width_characters(text.chars().count(), scale)
}

/// Rendered width of a known number of glyphs.
#[must_use]
pub const fn text_width_characters(characters: usize, scale: usize) -> usize {
    let trailing = if characters == 0 { 0 } else { 1 };
    characters
        .saturating_mul(GLYPH_ADVANCE)
        .saturating_sub(trailing)
        .saturating_mul(scale)
}

/// Largest preferred scale that fits, never below `minimum`.
#[must_use]
pub fn fit_text_scale(text: &str, maximum_width: usize, preferred: usize, minimum: usize) -> usize {
    let minimum = minimum.max(1);
    let mut scale = preferred.max(minimum);
    loop {
        if text_width(text, scale) <= maximum_width || scale == minimum {
            return scale;
        }
        scale = scale.saturating_sub(1);
    }
}

/// Number of complete glyph cells that fit in one horizontal extent.
#[must_use]
pub const fn text_capacity(maximum_width: usize, scale: usize) -> usize {
    if scale == 0 {
        return 0;
    }
    maximum_width.saturating_add(scale) / GLYPH_ADVANCE.saturating_mul(scale)
}

/// Convert packed `0xRRGGBB` to native RGB565 without dithering.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the channel masks prove the packed value fits in u16"
)]
pub const fn rgb888_to_rgb565(rgb: u32) -> u16 {
    let red = (rgb >> 19) & 0x1f;
    let green = (rgb >> 10) & 0x3f;
    let blue = (rgb >> 3) & 0x1f;
    ((red << 11) | (green << 5) | blue) as u16
}

/// Fixed-capacity UTF-8/ASCII formatting buffer for allocation-free labels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBuffer<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    len: usize,
}

impl<const CAPACITY: usize> TextBuffer<CAPACITY> {
    /// Construct an empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            len: 0,
        }
    }

    /// Construct from a clipped byte prefix.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut output = Self::new();
        output.push_bytes(bytes);
        output
    }

    /// Construct display ASCII with one `?` for each non-ASCII character.
    #[must_use]
    pub fn from_display(text: &str) -> Self {
        let mut output = Self::new();
        output.push_display(text);
        output
    }

    /// Current byte length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no bytes have been written.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Written bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or_default()
    }

    /// Written text when every submitted fragment was UTF-8.
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap_or_default()
    }

    /// Append as many bytes as fit.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
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

    /// Append display ASCII with one glyph for each Unicode character.
    pub fn push_display(&mut self, text: &str) {
        for character in text.chars() {
            let character = if character.is_ascii() {
                u8::try_from(u32::from(character)).unwrap_or(b'?')
            } else {
                b'?'
            };
            let Some(destination) = self.bytes.get_mut(self.len) else {
                return;
            };
            *destination = character;
            self.len = self.len.saturating_add(1);
        }
    }

    /// Clip the current value and append an ASCII ellipsis when possible.
    pub fn clip_with_ellipsis(&mut self, maximum: usize) {
        let maximum = maximum.min(CAPACITY);
        if self.len <= maximum {
            return;
        }
        if maximum < 3 {
            self.len = maximum;
            return;
        }
        let mut prefix = maximum - 3;
        while prefix > 0
            && self
                .bytes
                .get(..prefix)
                .is_some_and(|bytes| std::str::from_utf8(bytes).is_err())
        {
            prefix = prefix.saturating_sub(1);
        }
        self.len = prefix;
        self.push_bytes(b"...");
    }

    /// Clip to the number of scaled glyphs that fit in `maximum_width`.
    pub fn fit_width(&mut self, maximum_width: usize, scale: usize) {
        self.clip_with_ellipsis(text_capacity(maximum_width, scale));
    }
}

impl<const CAPACITY: usize> Default for TextBuffer<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> fmt::Write for TextBuffer<CAPACITY> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.push_bytes(value.as_bytes());
        Ok(())
    }
}

const fn glyph_rows(character: u8) -> [u8; GLYPH_HEIGHT] {
    match character {
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
        b'a' => [0, 0, 14, 1, 15, 17, 15],
        b'b' => [16, 16, 30, 17, 17, 17, 30],
        b'c' => [0, 0, 14, 16, 16, 17, 14],
        b'd' => [1, 1, 15, 17, 17, 17, 15],
        b'e' => [0, 0, 14, 17, 31, 16, 14],
        b'f' => [6, 9, 8, 28, 8, 8, 8],
        b'g' => [0, 0, 15, 17, 15, 1, 14],
        b'h' => [16, 16, 30, 17, 17, 17, 17],
        b'i' => [4, 0, 12, 4, 4, 4, 14],
        b'j' => [2, 0, 6, 2, 2, 18, 12],
        b'k' => [16, 16, 18, 20, 24, 20, 18],
        b'l' => [12, 4, 4, 4, 4, 4, 14],
        b'm' => [0, 0, 26, 21, 21, 17, 17],
        b'n' => [0, 0, 30, 17, 17, 17, 17],
        b'o' => [0, 0, 14, 17, 17, 17, 14],
        b'p' => [0, 0, 30, 17, 30, 16, 16],
        b'q' => [0, 0, 15, 17, 15, 1, 1],
        b'r' => [0, 0, 22, 25, 16, 16, 16],
        b's' => [0, 0, 15, 16, 14, 1, 30],
        b't' => [8, 8, 28, 8, 8, 9, 6],
        b'u' => [0, 0, 17, 17, 17, 19, 13],
        b'v' => [0, 0, 17, 17, 17, 10, 4],
        b'w' => [0, 0, 17, 17, 21, 21, 10],
        b'x' => [0, 0, 17, 10, 4, 10, 17],
        b'y' => [0, 0, 17, 17, 15, 1, 14],
        b'z' => [0, 0, 31, 2, 4, 8, 31],
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
        b' ' => [0; 7],
        b'.' => [0, 0, 0, 0, 0, 6, 6],
        b',' => [0, 0, 0, 0, 6, 6, 4],
        b':' => [0, 6, 6, 0, 6, 6, 0],
        b'-' => [0, 0, 0, 31, 0, 0, 0],
        b'/' => [1, 2, 2, 4, 8, 8, 16],
        b'+' => [0, 4, 4, 31, 4, 4, 0],
        b'!' => [4, 4, 4, 4, 4, 0, 4],
        b'\'' => [4, 4, 8, 0, 0, 0, 0],
        b'(' => [2, 4, 8, 8, 8, 4, 2],
        b')' => [8, 4, 2, 2, 2, 4, 8],
        b'&' => [12, 18, 20, 8, 21, 18, 13],
        b'#' => [10, 10, 31, 10, 31, 10, 10],
        b'_' => [0, 0, 0, 0, 0, 0, 31],
        b';' => [0, 6, 6, 0, 6, 6, 4],
        b'=' => [0, 31, 0, 31, 0, 0, 0],
        b'"' => [10, 10, 20, 0, 0, 0, 0],
        b'*' => [0, 21, 14, 31, 14, 21, 0],
        b'%' => [25, 25, 2, 4, 8, 19, 19],
        b'^' => [4, 10, 17, 0, 0, 0, 0],
        b'|' => [4, 4, 4, 4, 4, 4, 4],
        b'\\' => [16, 8, 8, 4, 2, 2, 1],
        b'<' => [2, 4, 8, 16, 8, 4, 2],
        b'>' => [8, 4, 2, 1, 2, 4, 8],
        b'[' => [14, 8, 8, 8, 8, 8, 14],
        b']' => [14, 2, 2, 2, 2, 2, 14],
        b'{' => [6, 4, 4, 24, 4, 4, 6],
        b'}' => [12, 4, 4, 3, 4, 4, 12],
        b'@' => [14, 17, 23, 21, 23, 16, 14],
        b'$' => [4, 15, 20, 14, 5, 30, 4],
        b'`' => [8, 4, 0, 0, 0, 0, 0],
        b'~' => [0, 0, 9, 22, 0, 0, 0],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Canvas, Rect, TextBuffer, fit_text_scale, rgb888_to_rgb565, text_capacity, text_width,
    };
    use std::fmt::Write as _;

    #[test]
    fn canvas_is_exact_clipped_and_pixel_cut() {
        let mut pixels = [0_u16; 60];
        assert!(Canvas::new(&mut pixels, 10, 6).is_some());
        let Some(mut canvas) = Canvas::new(&mut pixels, 10, 6) else {
            return;
        };
        canvas.fill_rect(Rect::new(8, 4, 8, 8), 7);
        assert_eq!(canvas.pixel(9, 5), Some(7));
        assert_eq!(canvas.pixel(7, 5), Some(0));
        canvas.fill_pixel_cut_rect(Rect::new(0, 0, 6, 6), 1, 9);
        assert_eq!(canvas.pixel(0, 0), Some(0));
        assert_eq!(canvas.pixel(1, 0), Some(9));
        assert!(Rect::new(2, 2, 3, 3).contains(4, 4));
        assert!(!Rect::new(2, 2, 3, 3).contains(5, 4));
    }

    #[test]
    fn font_distinguishes_case_and_counts_unicode_once() {
        let mut pixels = [0_u16; 36 * 9];
        let Some(mut canvas) = Canvas::new(&mut pixels, 36, 9) else {
            return;
        };
        canvas.draw_text(0, 0, "Aa", 1, 1);
        assert_ne!(canvas.pixel(1, 0), canvas.pixel(7, 0));
        assert_eq!(text_width("AžB", 2), 34);
        assert_eq!(fit_text_scale("ABCDE", 29, 3, 1), 1);
    }

    #[test]
    fn packed_color_and_fixed_text_are_exact() {
        assert_eq!(rgb888_to_rgb565(0xff_00_00), 0xf800);
        assert_eq!(rgb888_to_rgb565(0x00_ff_00), 0x07e0);
        assert_eq!(rgb888_to_rgb565(0x00_00_ff), 0x001f);
        let mut text = TextBuffer::<12>::new();
        assert!(write!(text, "VOL {}", 42).is_ok());
        assert_eq!(text.as_str(), "VOL 42");
        text.push_bytes(b" PERCENT");
        text.clip_with_ellipsis(10);
        assert_eq!(text.as_bytes(), b"VOL 42 ...");

        let mut display = TextBuffer::<16>::from_display("Wi-Fi Česko");
        assert_eq!(display.as_str(), "Wi-Fi ?esko");
        assert_eq!(text_capacity(29, 1), 5);
        display.fit_width(29, 1);
        assert_eq!(display.as_str(), "Wi...");
    }
}
