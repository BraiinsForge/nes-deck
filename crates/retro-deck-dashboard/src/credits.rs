//! Bounded software-credits layout and perspective rendering.

use retro_deck_config::{Credits, Palette, PaletteRole};
use retro_deck_ui::{
    Canvas, GLYPH_ADVANCE, GLYPH_HEIGHT, Rect, TextBuffer, glyph_pixel, text_width,
};

use crate::Action;
use crate::render::{CANVAS_HEIGHT, CANVAS_WIDTH, palette_pixel};

const TEXT_SCALE: usize = 4;
const MAXIMUM_LINE_WIDTH: usize = 1_040;
const LINE_ADVANCE: usize = 44;
const SECTION_GAP: usize = 28;
const HORIZON_Y: f64 = 56.0;
const BOTTOM_Y: f64 = 480.0;
const CLIP_TOP: isize = 72;
const FADE_INVISIBLE_Y: usize = 104;
const FADE_OPAQUE_Y: usize = 210;
const CAMERA_DISTANCE: f64 = 420.0;
const MAXIMUM_DEPTH: f64 = 4_000.0;
const SOURCE_UNITS_PER_MILLISECOND: f64 = 0.05;

#[derive(Clone, Debug, Eq, PartialEq)]
struct CrawlLine {
    text: TextBuffer<64>,
    source_y: usize,
    source_width: usize,
}

/// Prepared, bounded attribution text shared by animated and static views.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CreditsCrawl {
    lines: Vec<CrawlLine>,
    static_lines: Vec<TextBuffer<128>>,
    content_height: usize,
}

impl CreditsCrawl {
    /// Prepare a credits crawl once, outside the redraw loop.
    #[must_use]
    pub fn from_credits(credits: &Credits) -> Self {
        let mut crawl = Self::default();
        let mut cursor = 0;
        append_crawl_text(&mut crawl, &mut cursor, "RETRO DECK");
        append_crawl_text(&mut crawl, &mut cursor, "BUILT ON FREE SOFTWARE");
        cursor = cursor.saturating_add(SECTION_GAP);
        for credit in credits.entries() {
            let mut summary = TextBuffer::<128>::new();
            summary.push_display(credit.project());
            summary.push_bytes(b" / ");
            summary.push_display(credit.license());
            crawl.static_lines.push(summary);
            append_crawl_text(&mut crawl, &mut cursor, credit.project());
            append_crawl_text(&mut crawl, &mut cursor, credit.role());
            append_crawl_text(&mut crawl, &mut cursor, credit.license());
            cursor = cursor.saturating_add(SECTION_GAP);
        }
        append_crawl_text(&mut crawl, &mut cursor, "LICENSE TEXT ARCHIVE");
        append_crawl_text(&mut crawl, &mut cursor, "/mnt/data/nes-deck/licenses");
        cursor = cursor.saturating_add(SECTION_GAP);
        append_crawl_text(&mut crawl, &mut cursor, "THANK YOU");
        crawl.content_height = cursor;
        crawl
    }

    /// Safe empty view used when the optional manifest is unavailable.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            lines: Vec::new(),
            static_lines: Vec::new(),
            content_height: 0,
        }
    }

    /// Whether there is prepared content to render.
    #[must_use]
    pub fn is_available(&self) -> bool {
        !self.lines.is_empty() && self.content_height > 0
    }
}

/// Exact credits hit target derived from its rendered close control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreditsLayout;

impl CreditsLayout {
    pub(crate) const fn fixed() -> Self {
        Self
    }

    /// Close control in the top-right corner.
    #[must_use]
    pub const fn close_button(self) -> Rect {
        Rect::new(1_212, 12, 56, 56)
    }

    /// Map one committed touch coordinate to a semantic model action.
    #[must_use]
    pub const fn action_at(self, x: usize, y: usize) -> Option<Action> {
        if self.close_button().contains(x, y) {
            Some(Action::Back)
        } else {
            None
        }
    }
}

pub(crate) fn draw_credits(
    canvas: &mut Canvas<'_>,
    crawl: &CreditsCrawl,
    palette: &Palette,
    reduced_motion: bool,
    elapsed_ms: u64,
) -> CreditsLayout {
    canvas.clear(palette_pixel(palette, PaletteRole::Background));
    let layout = CreditsLayout::fixed();
    if !reduced_motion {
        draw_starfield(canvas, palette_pixel(palette, PaletteRole::Muted));
    }

    if !crawl.is_available() {
        canvas.draw_centered_text(
            Rect::new(80, 180, 1_120, 120),
            "CREDITS UNAVAILABLE",
            3,
            palette_pixel(palette, PaletteRole::Text),
        );
    } else if reduced_motion {
        draw_static_credits(canvas, crawl, palette);
    } else {
        draw_moving_credits(canvas, crawl, palette, elapsed_ms);
    }
    draw_close(
        canvas,
        layout.close_button(),
        palette_pixel(palette, PaletteRole::Muted),
    );
    layout
}

fn append_crawl_text(crawl: &mut CreditsCrawl, cursor: &mut usize, text: &str) {
    let maximum_characters =
        MAXIMUM_LINE_WIDTH.saturating_add(TEXT_SCALE) / GLYPH_ADVANCE.saturating_mul(TEXT_SCALE);
    let mut remaining = text;
    while !remaining.is_empty() {
        remaining = remaining.trim_start_matches(' ');
        if remaining.is_empty() {
            break;
        }
        let split = if text_width(remaining, TEXT_SCALE) <= MAXIMUM_LINE_WIDTH {
            remaining.len()
        } else {
            remaining
                .as_bytes()
                .get(..=maximum_characters)
                .and_then(|prefix| prefix.iter().rposition(|byte| *byte == b' '))
                .filter(|position| *position > 0)
                .unwrap_or(maximum_characters)
        };
        let Some(line_text) = remaining.get(..split) else {
            break;
        };
        let line = TextBuffer::<64>::from_display(line_text);
        crawl.lines.push(CrawlLine {
            source_width: text_width(line.as_str(), TEXT_SCALE),
            source_y: *cursor,
            text: line,
        });
        *cursor = cursor.saturating_add(LINE_ADVANCE);
        let Some(rest) = remaining.get(split..) else {
            break;
        };
        remaining = rest;
    }
}

fn draw_starfield(canvas: &mut Canvas<'_>, color: u16) {
    for index in 0..96_usize {
        if index % 7 == 0 {
            continue;
        }
        let x = (index.saturating_mul(193).saturating_add(47)) % CANVAS_WIDTH;
        let y = (index.saturating_mul(83).saturating_add(29)) % CANVAS_HEIGHT;
        let size = if index % 11 == 0 { 2 } else { 1 };
        canvas.fill_rect(Rect::new(x, y, size, size), color);
    }
}

fn draw_static_credits(canvas: &mut Canvas<'_>, crawl: &CreditsCrawl, palette: &Palette) {
    canvas.draw_text(
        20,
        20,
        "FOSS CREDITS",
        2,
        palette_pixel(palette, PaletteRole::Title),
    );
    canvas.draw_text(
        20,
        48,
        "PROJECT / LICENSE",
        1,
        palette_pixel(palette, PaletteRole::Muted),
    );
    let rows_per_column = 16;
    let columns = crawl.static_lines.len().div_ceil(rows_per_column).max(1);
    let left_margin = 24;
    let column_width = CANVAS_WIDTH.saturating_sub(left_margin * 2) / columns;
    for (index, source) in crawl.static_lines.iter().enumerate() {
        let column = index / rows_per_column;
        let row = index % rows_per_column;
        let mut shown = source.clone();
        shown.fit_width(column_width.saturating_sub(20), 1);
        canvas.draw_text(
            left_margin.saturating_add(column.saturating_mul(column_width)),
            78_usize.saturating_add(row.saturating_mul(22)),
            shown.as_str(),
            1,
            palette_pixel(palette, PaletteRole::Text),
        );
    }
    canvas.draw_text(
        20,
        458,
        "/mnt/data/nes-deck/licenses",
        1,
        palette_pixel(palette, PaletteRole::Muted),
    );
}

#[allow(
    clippy::cast_precision_loss,
    reason = "elapsed milliseconds are converted to the deployed f64 perspective model"
)]
fn draw_moving_credits(
    canvas: &mut Canvas<'_>,
    crawl: &CreditsCrawl,
    palette: &Palette,
    elapsed_ms: u64,
) {
    let cycle = crawl.content_height as f64 + MAXIMUM_DEPTH;
    let scroll = (elapsed_ms as f64 * SOURCE_UNITS_PER_MILLISECOND) % cycle;
    let color = palette_pixel(palette, PaletteRole::Title);
    for line in &crawl.lines {
        draw_crawl_line(canvas, line, scroll, color);
    }
}

const fn crawl_screen_y(depth: f64) -> f64 {
    HORIZON_Y + (BOTTOM_Y - HORIZON_Y) * crawl_scale(depth)
}

const fn crawl_scale(depth: f64) -> f64 {
    CAMERA_DISTANCE / (CAMERA_DISTANCE + depth)
}

fn crawl_alpha(screen_y: usize) -> u32 {
    if screen_y <= FADE_INVISIBLE_Y {
        0
    } else if screen_y >= FADE_OPAQUE_Y {
        256
    } else {
        u32::try_from((screen_y - FADE_INVISIBLE_Y) * 256 / (FADE_OPAQUE_Y - FADE_INVISIBLE_Y))
            .unwrap_or_default()
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "bounded 1280x480 projection coordinates are explicitly clipped before pixel access"
)]
fn draw_crawl_line(canvas: &mut Canvas<'_>, line: &CrawlLine, scroll: f64, color: u16) {
    if line.source_width == 0 {
        return;
    }
    let source_top = (line.source_y as f64).max(scroll - MAXIMUM_DEPTH);
    let source_bottom =
        (line.source_y.saturating_add(GLYPH_HEIGHT * TEXT_SCALE) as f64).min(scroll);
    if source_top >= source_bottom {
        return;
    }

    let top_y = crawl_screen_y(scroll - source_top);
    let bottom_y = crawl_screen_y(scroll - source_bottom);
    let first_y = CLIP_TOP.max(top_y.floor() as isize);
    let last_y = 479_isize.min(bottom_y.ceil() as isize - 1);
    let projection_height = BOTTOM_Y - HORIZON_Y;
    for screen_y in first_y..=last_y {
        let scale = (screen_y as f64 + 0.5 - HORIZON_Y) / projection_height;
        if scale <= 0.0 {
            continue;
        }
        let depth = CAMERA_DISTANCE * (1.0 / scale - 1.0);
        let source_row = (scroll - depth - line.source_y as f64).floor() as isize;
        let source_height = 28_isize;
        if source_row < 0 || source_row >= source_height {
            continue;
        }
        let left = CANVAS_WIDTH as f64 * 0.5 - line.source_width as f64 * 0.5 * scale;
        let right = CANVAS_WIDTH as f64 * 0.5 + line.source_width as f64 * 0.5 * scale;
        let first_x = 0_isize.max((left - 0.5).ceil() as isize);
        let last_x = 1_279_isize.min((right - 0.5).floor() as isize);
        let Ok(y) = usize::try_from(screen_y) else {
            continue;
        };
        let alpha = crawl_alpha(y);
        if alpha == 0 {
            continue;
        }
        for screen_x in first_x..=last_x {
            let source_column = ((screen_x as f64 + 0.5 - CANVAS_WIDTH as f64 * 0.5) / scale
                + line.source_width as f64 * 0.5)
                .floor() as isize;
            let source_width = isize::try_from(line.source_width).unwrap_or(isize::MAX);
            if source_column < 0 || source_column >= source_width {
                continue;
            }
            let (Ok(x), Ok(column), Ok(row)) = (
                usize::try_from(screen_x),
                usize::try_from(source_column),
                usize::try_from(source_row),
            ) else {
                continue;
            };
            if !crawl_line_pixel(line, column, row) {
                continue;
            }
            let Some(background) = canvas.pixel(x, y) else {
                continue;
            };
            canvas.set_pixel(
                x,
                y,
                if alpha == 256 {
                    color
                } else {
                    blend_rgb565(color, background, alpha)
                },
            );
        }
    }
}

fn crawl_line_pixel(line: &CrawlLine, source_column: usize, source_row: usize) -> bool {
    let base_column = source_column / TEXT_SCALE;
    let character_index = base_column / GLYPH_ADVANCE;
    let glyph_column = base_column % GLYPH_ADVANCE;
    let glyph_row = source_row / TEXT_SCALE;
    line.text
        .as_bytes()
        .get(character_index)
        .is_some_and(|character| glyph_pixel(*character, glyph_column, glyph_row))
}

fn blend_rgb565(foreground: u16, background: u16, alpha: u32) -> u16 {
    let inverse = 256_u32.saturating_sub(alpha);
    let foreground = u32::from(foreground);
    let background = u32::from(background);
    let red =
        (((foreground >> 11) & 0x1f) * alpha + ((background >> 11) & 0x1f) * inverse + 128) >> 8;
    let green =
        (((foreground >> 5) & 0x3f) * alpha + ((background >> 5) & 0x3f) * inverse + 128) >> 8;
    let blue = ((foreground & 0x1f) * alpha + (background & 0x1f) * inverse + 128) >> 8;
    u16::try_from((red << 11) | (green << 5) | blue).unwrap_or_default()
}

fn draw_close(canvas: &mut Canvas<'_>, bounds: Rect, color: u16) {
    let center_x = bounds.x.saturating_add(bounds.width / 2);
    let center_y = bounds.y.saturating_add(bounds.height / 2);
    for offset in [-12_isize, -8, -4, 0, 4, 8, 12] {
        let (Some(first_x), Some(first_y), Some(second_x), Some(second_y)) = (
            center_x.checked_add_signed(offset),
            center_y.checked_add_signed(offset),
            center_x.checked_add_signed(offset),
            center_y.checked_add_signed(-offset),
        ) else {
            continue;
        };
        canvas.fill_rect(Rect::new(first_x, first_y, 4, 4), color);
        canvas.fill_rect(Rect::new(second_x, second_y, 4, 4), color);
    }
}

#[cfg(test)]
mod tests {
    use retro_deck_config::{Credits, Palette};

    use super::{CreditsCrawl, CreditsLayout, draw_credits};
    use crate::{Action, CANVAS_HEIGHT, CANVAS_WIDTH};
    use retro_deck_ui::Canvas;

    const DEPLOYED_CREDITS: &[u8] = include_bytes!("../../../deploy/menu/credits.tsv");

    fn crawl() -> Option<CreditsCrawl> {
        Credits::parse(DEPLOYED_CREDITS)
            .ok()
            .map(|credits| CreditsCrawl::from_credits(&credits))
    }

    fn render(crawl: &CreditsCrawl, reduced_motion: bool, elapsed_ms: u64) -> Vec<u16> {
        let mut pixels = vec![0_u16; CANVAS_WIDTH * CANVAS_HEIGHT];
        let Some(mut canvas) = Canvas::new(&mut pixels, CANVAS_WIDTH, CANVAS_HEIGHT) else {
            return Vec::new();
        };
        let _ = draw_credits(
            &mut canvas,
            crawl,
            &Palette::default(),
            reduced_motion,
            elapsed_ms,
        );
        pixels
    }

    fn hash(pixels: &[u16]) -> u64 {
        pixels.iter().fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
            (hash ^ u64::from(*pixel)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    #[test]
    fn prepares_bounded_lines_and_hit_target() {
        let Some(crawl) = crawl() else {
            return;
        };
        assert!(crawl.is_available());
        assert!(
            crawl
                .lines
                .iter()
                .all(|line| line.source_width < CANVAS_WIDTH)
        );
        assert_eq!(
            CreditsLayout::fixed().action_at(1_240, 40),
            Some(Action::Back)
        );
        assert_eq!(CreditsLayout::fixed().action_at(600, 240), None);
    }

    #[test]
    fn moving_and_static_frames_have_expected_time_behavior() {
        let Some(crawl) = crawl() else {
            return;
        };
        assert_ne!(render(&crawl, false, 0), render(&crawl, false, 2_000));
        assert_eq!(render(&crawl, true, 0), render(&crawl, true, 60_000));
    }

    #[test]
    fn unavailable_manifest_renders_a_complete_safe_frame() {
        let pixels = render(&CreditsCrawl::unavailable(), false, 0);
        assert_eq!(pixels.len(), CANVAS_WIDTH * CANVAS_HEIGHT);
        assert!(pixels.iter().any(|pixel| *pixel != 0));
    }

    #[test]
    fn canonical_credits_views_have_stable_snapshots() {
        let Some(crawl) = crawl() else {
            return;
        };
        assert_eq!(
            [
                hash(&render(&crawl, false, 2_000)),
                hash(&render(&crawl, false, 20_000)),
                hash(&render(&crawl, true, 0)),
            ],
            [
                11_741_800_488_097_228_061,
                11_931_739_180_033_280_179,
                1_446_444_681_621_451_047,
            ]
        );
    }
}
