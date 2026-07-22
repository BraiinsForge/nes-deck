use std::cell::RefCell;
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 480;

struct Canvas {
    pixmap: Pixmap,
}

impl Canvas {
    fn new() -> Self {
        let mut pixmap = Pixmap::new(WIDTH, HEIGHT).expect("fixed canvas dimensions are valid");
        pixmap.fill(Color::BLACK);
        Self { pixmap }
    }

    fn clear(&mut self, color: u32) {
        self.pixmap.fill(color_value(color));
    }

    fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: u32) {
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
}

fn color_value(color: u32) -> Color {
    Color::from_rgba8(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
        0xff,
    )
}

fn clipped_rect(x: i32, y: i32, width: u32, height: u32) -> Option<(i32, i32, u32, u32)> {
    let left = i64::from(x).clamp(0, i64::from(WIDTH));
    let top = i64::from(y).clamp(0, i64::from(HEIGHT));
    let right = (i64::from(x) + i64::from(width)).clamp(0, i64::from(WIDTH));
    let bottom = (i64::from(y) + i64::from(height)).clamp(0, i64::from(HEIGHT));
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
}
