//! Allocation-bounded RGB565 renderer for the 10 Seconds game.

use std::{cmp::Ordering, fmt};

use retro_deck_ui::{Canvas, Rect, text_width_characters};

pub use retro_deck_ui::rgb888_to_rgb565;

use super::{Centiseconds, TimerPhase, TimerView};

/// Native timer canvas width before the platform applies its exact scale.
pub const CANVAS_WIDTH: usize = 624;
/// Native timer canvas height before the platform applies its exact scale.
pub const CANVAS_HEIGHT: usize = 224;
const PIXELS: usize = CANVAS_WIDTH * CANVAS_HEIGHT;

const BACKGROUND: u16 = rgb888_to_rgb565(0x10_0d_0c);
const AMBER: u16 = rgb888_to_rgb565(0xff_71_38);
const DIM_AMBER: u16 = rgb888_to_rgb565(0x1c_1c_1c);
const CREAM: u16 = rgb888_to_rgb565(0xff_ed_c2);
const MUTED: u16 = rgb888_to_rgb565(0xaa_8f_7c);
const SUCCESS: u16 = rgb888_to_rgb565(0x62_d3_8c);
const BUTTON: u16 = rgb888_to_rgb565(0x29_21_1e);

/// Fixed-size rendered timer frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimerFrame {
    pixels: Vec<u16>,
}

impl TimerFrame {
    /// Render one complete frame without retaining partially initialized data.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when memory for the fixed canvas is unavailable.
    pub fn render(view: TimerView) -> Result<Self, RenderError> {
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(PIXELS).map_err(|_| RenderError)?;
        pixels.resize(PIXELS, BACKGROUND);
        let mut frame = Self { pixels };
        frame.draw(view);
        Ok(frame)
    }

    /// Redraw a complete view into the existing fixed allocation.
    ///
    /// This performs no allocation and discards every pixel from the previous
    /// view before drawing the new one.
    pub fn redraw(&mut self, view: TimerView) {
        self.pixels.fill(BACKGROUND);
        self.draw(view);
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

    /// Return the fixed row stride in bytes.
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
        let offset = y.checked_mul(CANVAS_WIDTH)?.checked_add(x)?;
        self.pixels.get(offset).copied()
    }

    fn draw(&mut self, view: TimerView) {
        let Some(mut canvas) = Canvas::new(&mut self.pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return;
        };
        canvas.fill_rect(Rect::new(6, 5, 70, 25), BUTTON);
        canvas.draw_text(15, 11, "BACK", 2, CREAM);
        draw_centered_text(&mut canvas, 9, "STOP AT 10.00", 2, CREAM);

        let digit_color =
            if view.phase() == TimerPhase::Stopped && view.displayed() == Centiseconds::TARGET {
                SUCCESS
            } else {
                AMBER
            };
        let positions = [129, 219, 329, 419];
        for (x, digit) in positions
            .into_iter()
            .zip(centisecond_digits(view.displayed()))
        {
            draw_digit(&mut canvas, x, 43, digit, digit_color, DIM_AMBER);
        }
        canvas.fill_rect(Rect::new(303, 149, 14, 14), digit_color);

        if let Some(result) = result(view) {
            let color = if view.displayed() == Centiseconds::TARGET {
                SUCCESS
            } else {
                MUTED
            };
            match result {
                ResultLine::Exact => draw_centered_text(&mut canvas, 178, "EXACT", 1, color),
                ResultLine::Difference { difference, label } => {
                    draw_centered_result(&mut canvas, 178, difference, label, color);
                }
            }
        }

        let instruction = match view.phase() {
            TimerPhase::Ready => "TAP OR A TO START",
            TimerPhase::Running => "TAP OR A TO STOP",
            TimerPhase::Stopped => "TAP OR A FOR ANOTHER TRY",
        };
        draw_centered_text(&mut canvas, 198, instruction, 2, CREAM);
    }
}

fn draw_centered_text(canvas: &mut Canvas<'_>, y: usize, text: &str, scale: usize, color: u16) {
    let width = text_width_characters(text.len(), scale);
    let x = CANVAS_WIDTH.saturating_sub(width) / 2;
    canvas.draw_text(x, y, text, scale, color);
}

fn draw_centered_result(
    canvas: &mut Canvas<'_>,
    y: usize,
    difference: Centiseconds,
    label: &str,
    color: u16,
) {
    let characters = 6_usize.saturating_add(label.len());
    let x = CANVAS_WIDTH.saturating_sub(text_width_characters(characters, 1)) / 2;
    canvas.draw_bytes(x, y, &centisecond_text(difference), 1, color);
    canvas.draw_text(x.saturating_add(36), y, label, 1, color);
}

fn draw_digit(canvas: &mut Canvas<'_>, x: usize, y: usize, digit: u8, active: u16, inactive: u16) {
    let width = 76;
    let height = 128;
    let thickness = 11;
    let bounds = [
        Rect::new(
            x.saturating_add(thickness),
            y,
            width - 2 * thickness,
            thickness,
        ),
        Rect::new(
            x.saturating_add(width - thickness),
            y.saturating_add(thickness),
            thickness,
            height / 2 - thickness,
        ),
        Rect::new(
            x.saturating_add(width - thickness),
            y.saturating_add(height / 2),
            thickness,
            height / 2 - thickness,
        ),
        Rect::new(
            x.saturating_add(thickness),
            y.saturating_add(height - thickness),
            width - 2 * thickness,
            thickness,
        ),
        Rect::new(
            x,
            y.saturating_add(height / 2),
            thickness,
            height / 2 - thickness,
        ),
        Rect::new(
            x,
            y.saturating_add(thickness),
            thickness,
            height / 2 - thickness,
        ),
        Rect::new(
            x.saturating_add(thickness),
            y.saturating_add(height / 2 - thickness / 2),
            width - 2 * thickness,
            thickness,
        ),
    ];
    let mask = digit_segments(digit);
    for (index, bounds) in bounds.into_iter().enumerate() {
        let color = if mask & (1_u8 << index) == 0 {
            inactive
        } else {
            active
        };
        canvas.fill_rect(bounds, color);
    }
}

/// Fixed canvas allocation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderError;

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cannot allocate the 10 Seconds frame")
    }
}

impl std::error::Error for RenderError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResultLine {
    Exact,
    Difference {
        difference: Centiseconds,
        label: &'static str,
    },
}

fn result(view: TimerView) -> Option<ResultLine> {
    if view.phase() != TimerPhase::Stopped {
        return None;
    }
    let displayed = view.displayed().get();
    match displayed.cmp(&Centiseconds::TARGET.get()) {
        Ordering::Equal => Some(ResultLine::Exact),
        Ordering::Less => Some(ResultLine::Difference {
            difference: Centiseconds(Centiseconds::TARGET.get() - displayed),
            label: "EARLY",
        }),
        Ordering::Greater => Some(ResultLine::Difference {
            difference: Centiseconds(displayed - Centiseconds::TARGET.get()),
            label: "LATE",
        }),
    }
}

const fn centisecond_digits(value: Centiseconds) -> [u8; 4] {
    let value = value.get();
    [
        b'0' + (value / 1_000) as u8,
        b'0' + (value / 100 % 10) as u8,
        b'0' + (value / 10 % 10) as u8,
        b'0' + (value % 10) as u8,
    ]
}

const fn centisecond_text(value: Centiseconds) -> [u8; 5] {
    let digits = centisecond_digits(value);
    [digits[0], digits[1], b'.', digits[2], digits[3]]
}

const fn digit_segments(digit: u8) -> u8 {
    match digit {
        b'0' => 0x3f,
        b'1' => 0x06,
        b'2' => 0x5b,
        b'3' => 0x4f,
        b'4' => 0x66,
        b'5' => 0x6d,
        b'6' => 0x7d,
        b'7' => 0x07,
        b'8' => 0x7f,
        b'9' => 0x6f,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(phase: TimerPhase, displayed: u16) -> TimerView {
        TimerView {
            phase,
            displayed: Centiseconds(displayed),
        }
    }

    fn hash(frame: &TimerFrame) -> u64 {
        frame
            .pixels()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
                (hash ^ u64::from(*pixel)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    }

    #[test]
    fn renders_fixed_complete_frames() {
        let frame = TimerFrame::render(view(TimerPhase::Ready, 0));
        assert!(frame.is_ok());
        let Some(frame) = frame.ok() else {
            return;
        };
        assert_eq!(frame.pixels().len(), PIXELS);
        assert_eq!(TimerFrame::stride_bytes(), CANVAS_WIDTH * 2);
        assert_eq!(frame.pixel(0, 0), Some(BACKGROUND));
        assert_eq!(frame.pixel(6, 5), Some(BUTTON));
        assert_eq!(frame.pixel(CANVAS_WIDTH, 0), None);
        assert_eq!(frame.pixel(0, CANVAS_HEIGHT), None);
    }

    #[test]
    fn exact_results_turn_the_digits_green() {
        let exact = TimerFrame::render(view(TimerPhase::Stopped, 1_000));
        let miss = TimerFrame::render(view(TimerPhase::Stopped, 999));
        let (Some(exact), Some(miss)) = (exact.ok(), miss.ok()) else {
            return;
        };
        assert_eq!(exact.pixel(194, 60), Some(SUCCESS));
        assert_eq!(miss.pixel(194, 60), Some(AMBER));
        assert!(exact.pixels().iter().any(|pixel| *pixel == SUCCESS));
        assert!(!miss.pixels().iter().any(|pixel| *pixel == SUCCESS));
        assert!(miss.pixels().iter().any(|pixel| *pixel == MUTED));
    }

    #[test]
    fn canonical_views_have_stable_pixel_snapshots() {
        let views = [
            view(TimerPhase::Ready, 0),
            view(TimerPhase::Running, 742),
            view(TimerPhase::Stopped, 1_000),
            view(TimerPhase::Stopped, 913),
        ];
        let hashes = views.map(|view| {
            TimerFrame::render(view)
                .ok()
                .map_or(0, |frame| hash(&frame))
        });
        // Captured from the legacy C++ renderer before this replacement.
        assert_eq!(
            hashes,
            [
                9_498_646_050_448_728_937,
                8_473_485_944_558_349_693,
                11_964_939_762_664_925_513,
                2_612_144_829_727_006_395,
            ]
        );
    }

    #[test]
    fn redraw_reuses_the_fixed_frame_allocation() {
        let Some(mut frame) = TimerFrame::render(view(TimerPhase::Ready, 0)).ok() else {
            return;
        };
        let allocation = frame.pixels.as_ptr();
        let capacity = frame.pixels.capacity();

        frame.redraw(view(TimerPhase::Running, 742));

        assert_eq!(frame.pixels.as_ptr(), allocation);
        assert_eq!(frame.pixels.capacity(), capacity);
        assert_eq!(hash(&frame), 8_473_485_944_558_349_693);
    }
}
