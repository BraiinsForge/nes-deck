use crate::{
    font,
    projection::{Projection, TextMask},
    raster::Raster,
};
use std::cell::RefCell;
use std::path::Path;
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 480;

struct Canvas {
    pixmap: Pixmap,
    rasters: Vec<Raster>,
    text_masks: Vec<TextMask>,
    projection: Option<Projection>,
}

impl Canvas {
    fn new() -> Self {
        let mut pixmap = Pixmap::new(WIDTH, HEIGHT).expect("fixed canvas dimensions are valid");
        pixmap.fill(Color::BLACK);
        Self {
            pixmap,
            rasters: Vec::new(),
            text_masks: Vec::new(),
            projection: None,
        }
    }

    fn clear(&mut self, color: u32) {
        self.pixmap.fill(color_value(color));
    }

    fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: u32) {
        self.fill_rect_at(i64::from(x), i64::from(y), width, height, color);
    }

    fn fill_rect_at(&mut self, x: i64, y: i64, width: u32, height: u32, color: u32) {
        let Some((x, y, width, height)) = clipped_rect(x, y, width, height) else {
            return;
        };
        let rect = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32)
            .expect("clipped rectangle is nonempty");
        let mut paint = Paint::default();
        paint.anti_alias = false;
        paint.set_color(color_value(color));
        self.pixmap
            .fill_rect(rect, &paint, Transform::identity(), None);
    }

    fn draw_glyph(&mut self, x: i32, y: i32, character: u8, scale: u32, color: u32) {
        for (row, mask) in font::rows(character).iter().enumerate() {
            for column in 0..5 {
                if mask & (1 << (4 - column)) != 0 {
                    self.fill_rect_at(
                        i64::from(x) + column * i64::from(scale),
                        i64::from(y) + row as i64 * i64::from(scale),
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
    }

    fn store_raster(&mut self, raster: Raster) -> Result<u32, String> {
        let handle = u32::try_from(self.rasters.len() + 1)
            .map_err(|_| "native raster handle space is exhausted".to_owned())?;
        self.rasters.push(raster);
        Ok(handle)
    }

    fn clear_rasters(&mut self) {
        self.rasters.clear();
    }

    fn store_text_mask(&mut self, mask: TextMask) -> Result<u32, String> {
        let handle = u32::try_from(self.text_masks.len() + 1)
            .map_err(|_| "native text mask handle space is exhausted".to_owned())?;
        self.text_masks.push(mask);
        Ok(handle)
    }

    fn clear_text_masks(&mut self) {
        self.text_masks.clear();
        self.projection = None;
    }

    fn configure_projection(&mut self, projection: Projection) {
        self.projection = Some(projection);
    }

    fn draw_projected_text(&mut self, handle: u32, source_y: i32) -> Result<(), String> {
        let index = handle
            .checked_sub(1)
            .ok_or_else(|| "native text mask handle must be positive".to_owned())?;
        let mask = self
            .text_masks
            .get(index as usize)
            .ok_or_else(|| format!("native text mask handle {handle} is unavailable"))?;
        let projection = self
            .projection
            .as_ref()
            .ok_or_else(|| "native text projection is not configured".to_owned())?;
        mask.draw(
            self.pixmap.data_mut(),
            (WIDTH, HEIGHT),
            source_y,
            projection,
        )
    }

    fn draw_raster(
        &mut self,
        handle: u32,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let index = handle
            .checked_sub(1)
            .ok_or_else(|| "native raster handle must be positive".to_owned())?;
        let raster = self
            .rasters
            .get(index as usize)
            .ok_or_else(|| format!("native raster handle {handle} is unavailable"))?;
        raster.draw(self.pixmap.data_mut(), (WIDTH, HEIGHT), x, y, width, height)
    }
}

fn color_value(color: u32) -> Color {
    Color::from_rgba8(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
        0xff,
    )
}

fn clipped_rect(x: i64, y: i64, width: u32, height: u32) -> Option<(i32, i32, u32, u32)> {
    let left = x.clamp(0, i64::from(WIDTH));
    let top = y.clamp(0, i64::from(HEIGHT));
    let right = x
        .saturating_add(i64::from(width))
        .clamp(0, i64::from(WIDTH));
    let bottom = y
        .saturating_add(i64::from(height))
        .clamp(0, i64::from(HEIGHT));
    (right > left && bottom > top).then_some((
        left as i32,
        top as i32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

thread_local! {
    static CANVAS: RefCell<Canvas> = RefCell::new(Canvas::new());
}

pub fn clear(color: u32) {
    CANVAS.with(|canvas| canvas.borrow_mut().clear(color));
}

pub fn fill_rect(x: i32, y: i32, width: u32, height: u32, color: u32) {
    CANVAS.with(|canvas| {
        canvas.borrow_mut().fill_rect(x, y, width, height, color);
    });
}

pub fn draw_glyph(x: i32, y: i32, character: u8, scale: u32, color: u32) {
    CANVAS.with(|canvas| {
        canvas
            .borrow_mut()
            .draw_glyph(x, y, character, scale, color);
    });
}

pub fn load_text_mask(text: &[u8], scale: u32) -> Result<u32, String> {
    let mask = TextMask::new(text, scale, (WIDTH, HEIGHT))?;
    CANVAS.with(|canvas| canvas.borrow_mut().store_text_mask(mask))
}

pub fn clear_text_masks() {
    CANVAS.with(|canvas| canvas.borrow_mut().clear_text_masks());
}

#[allow(clippy::too_many_arguments)]
pub fn configure_projection(
    elapsed_ms: i64,
    speed_numerator: u32,
    speed_denominator: u32,
    cycle: u32,
    camera_distance: u32,
    maximum_depth: u32,
    horizon_y: i32,
    clip_top: i32,
    fade_invisible_y: i32,
    fade_opaque_y: i32,
    bottom_y: i32,
    color: u32,
) -> Result<(), String> {
    let projection = Projection::new(
        elapsed_ms,
        speed_numerator,
        speed_denominator,
        cycle,
        camera_distance,
        maximum_depth,
        horizon_y,
        clip_top,
        fade_invisible_y,
        fade_opaque_y,
        bottom_y,
        color,
        HEIGHT,
    )?;
    CANVAS.with(|canvas| canvas.borrow_mut().configure_projection(projection));
    Ok(())
}

pub fn draw_projected_text(handle: u32, source_y: i32) -> Result<(), String> {
    CANVAS.with(|canvas| canvas.borrow_mut().draw_projected_text(handle, source_y))
}

pub fn load_cover_raster(path: &Path, background: u32) -> Result<u32, String> {
    let Some(raster) = Raster::load_cover(path, background)? else {
        return Ok(0);
    };
    CANVAS.with(|canvas| canvas.borrow_mut().store_raster(raster))
}

pub fn load_png_raster(path: &Path, width: u32, height: u32) -> Result<u32, String> {
    let Some(raster) = Raster::load_png(path, width, height)? else {
        return Ok(0);
    };
    CANVAS.with(|canvas| canvas.borrow_mut().store_raster(raster))
}

pub fn clear_rasters() {
    CANVAS.with(|canvas| canvas.borrow_mut().clear_rasters());
}

pub fn draw_raster(handle: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
    CANVAS.with(|canvas| canvas.borrow_mut().draw_raster(handle, x, y, width, height))
}

pub fn with_pixels<T>(callback: impl FnOnce(&[u8]) -> T) -> T {
    CANVAS.with(|canvas| callback(canvas.borrow().pixmap.data()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(data: &[u8], x: usize, y: usize) -> &[u8] {
        let offset = (y * WIDTH as usize + x) * 4;
        &data[offset..offset + 4]
    }

    #[test]
    fn clears_to_opaque_policy_colors() {
        clear(0xfe6c27);
        with_pixels(|data| {
            assert_eq!(data.len(), WIDTH as usize * HEIGHT as usize * 4);
            assert_eq!(pixel(data, 0, 0), &[0xfe, 0x6c, 0x27, 0xff]);
            assert_eq!(
                pixel(data, WIDTH as usize - 1, HEIGHT as usize - 1),
                &[0xfe, 0x6c, 0x27, 0xff]
            );
        });
    }

    #[test]
    fn fills_exact_clipped_integer_rectangles() {
        clear(0x000000);
        fill_rect(-1, -1, 2, 2, 0xffffff);
        fill_rect(WIDTH as i32 - 1, HEIGHT as i32 - 1, 2, 2, 0xecb6e7);
        with_pixels(|data| {
            assert_eq!(pixel(data, 0, 0), &[0xff, 0xff, 0xff, 0xff]);
            assert_eq!(pixel(data, 1, 0), &[0x00, 0x00, 0x00, 0xff]);
            assert_eq!(
                pixel(data, WIDTH as usize - 1, HEIGHT as usize - 1),
                &[0xec, 0xb6, 0xe7, 0xff]
            );
            assert_eq!(
                pixel(data, WIDTH as usize - 2, HEIGHT as usize - 1),
                &[0x00, 0x00, 0x00, 0xff]
            );
        });
    }

    #[test]
    fn draws_exact_scaled_bitmap_glyphs() {
        clear(0x000000);
        draw_glyph(20, 30, b'A', 2, 0xfe6c27);
        draw_glyph(40, 30, b'?', 1, 0xffffff);
        draw_glyph(50, 30, 0xff, 1, 0xffffff);
        with_pixels(|data| {
            assert_eq!(pixel(data, 20, 30), &[0x00, 0x00, 0x00, 0xff]);
            assert_eq!(pixel(data, 22, 30), &[0xfe, 0x6c, 0x27, 0xff]);
            assert_eq!(pixel(data, 27, 31), &[0xfe, 0x6c, 0x27, 0xff]);
            assert_eq!(pixel(data, 28, 30), &[0x00, 0x00, 0x00, 0xff]);
            assert_eq!(pixel(data, 20, 36), &[0xfe, 0x6c, 0x27, 0xff]);
            assert_eq!(pixel(data, 29, 37), &[0xfe, 0x6c, 0x27, 0xff]);
            for row in 0..7 {
                for column in 0..5 {
                    assert_eq!(
                        pixel(data, 40 + column, 30 + row),
                        pixel(data, 50 + column, 30 + row)
                    );
                }
            }
        });
    }

    #[test]
    fn clips_scaled_glyphs_without_coordinate_overflow() {
        clear(0x000000);
        draw_glyph(-2, -2, b'A', 2, 0xfe6c27);
        draw_glyph(WIDTH as i32 - 4, HEIGHT as i32 - 4, b'A', 2, 0xecb6e7);
        with_pixels(|data| {
            assert_eq!(pixel(data, 6, 0), &[0xfe, 0x6c, 0x27, 0xff]);
            assert_eq!(pixel(data, 5, 0), &[0x00, 0x00, 0x00, 0xff]);
            assert_eq!(
                pixel(data, WIDTH as usize - 2, HEIGHT as usize - 4),
                &[0xec, 0xb6, 0xe7, 0xff]
            );
            assert_eq!(
                pixel(data, WIDTH as usize - 5, HEIGHT as usize - 4),
                &[0x00, 0x00, 0x00, 0xff]
            );
        });

        clear(0x000000);
        draw_glyph(0, 0, b'B', u32::MAX, 0xffffaf);
        with_pixels(|data| {
            assert_eq!(pixel(data, 0, 0), &[0xff, 0xff, 0xaf, 0xff]);
            assert_eq!(
                pixel(data, WIDTH as usize - 1, HEIGHT as usize - 1),
                &[0xff, 0xff, 0xaf, 0xff]
            );
        });
    }
}
