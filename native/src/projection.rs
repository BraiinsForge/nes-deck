use crate::font;

const GLYPH_WIDTH: u32 = 5;
const GLYPH_HEIGHT: u32 = 7;
const GLYPH_ADVANCE: u32 = 6;

pub(crate) struct TextMask {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

pub(crate) struct Projection {
    scroll: f64,
    camera_distance: f64,
    maximum_depth: f64,
    horizon_y: i32,
    clip_top: i32,
    fade_invisible_y: i32,
    fade_opaque_y: i32,
    bottom_y: i32,
    color: u16,
}

impl TextMask {
    pub(crate) fn new(text: &[u8], scale: u32, canvas_size: (u32, u32)) -> Result<Self, String> {
        if text.is_empty() || scale == 0 {
            return Err("projected text and scale must be nonempty".to_owned());
        }
        let characters =
            u32::try_from(text.len()).map_err(|_| "projected text is too long".to_owned())?;
        let width = characters
            .checked_mul(GLYPH_ADVANCE)
            .and_then(|value| value.checked_sub(1))
            .and_then(|value| value.checked_mul(scale))
            .ok_or_else(|| "projected text dimensions overflow".to_owned())?;
        let height = GLYPH_HEIGHT
            .checked_mul(scale)
            .ok_or_else(|| "projected text dimensions overflow".to_owned())?;
        if width > canvas_size.0 || height > canvas_size.1 {
            return Err("projected text exceeds the native canvas".to_owned());
        }
        let length = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| "projected text dimensions overflow".to_owned())?;
        let mut pixels = vec![0; length];
        for (character_index, character) in text.iter().enumerate() {
            for (row, mask) in font::rows(*character).iter().enumerate() {
                for column in 0..GLYPH_WIDTH {
                    if mask & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                        continue;
                    }
                    let left = (character_index as u32 * GLYPH_ADVANCE + column) * scale;
                    let top = row as u32 * scale;
                    for y in top..top + scale {
                        let start = y as usize * width as usize + left as usize;
                        pixels[start..start + scale as usize].fill(1);
                    }
                }
            }
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub(crate) fn draw(
        &self,
        canvas: &mut [u8],
        canvas_size: (u32, u32),
        source_y: i32,
        projection: &Projection,
    ) -> Result<(), String> {
        let (canvas_width, canvas_height) = canvas_size;
        if canvas.len() != canvas_width as usize * canvas_height as usize * 4 {
            return Err("native canvas dimensions are invalid".to_owned());
        }
        let source_top = f64::from(source_y).max(projection.scroll - projection.maximum_depth);
        let source_bottom = (f64::from(source_y) + f64::from(self.height)).min(projection.scroll);
        if source_top >= source_bottom {
            return Ok(());
        }

        let top_y = projection.screen_y(source_top);
        let bottom_y = projection.screen_y(source_bottom);
        let first_y = projection.clip_top.max(top_y.floor() as i32);
        let last_y = (projection.bottom_y - 1).min(bottom_y.ceil() as i32 - 1);
        let projection_height = f64::from(projection.bottom_y - projection.horizon_y);
        for y in first_y..=last_y {
            let scale = (f64::from(y) + 0.5 - f64::from(projection.horizon_y)) / projection_height;
            if scale <= 0.0 {
                continue;
            }
            let depth = projection.camera_distance * (1.0 / scale - 1.0);
            let source_row = (projection.scroll - depth - f64::from(source_y)).floor() as i32;
            if source_row < 0 || source_row >= self.height as i32 {
                continue;
            }
            let center = f64::from(canvas_width) * 0.5;
            let left = center - f64::from(self.width) * 0.5 * scale;
            let right = center + f64::from(self.width) * 0.5 * scale;
            let first_x = 0.max((left - 0.5).ceil() as i32);
            let last_x = (canvas_width as i32 - 1).min((right - 0.5).floor() as i32);
            let alpha = projection.alpha(y);
            if alpha <= 0 {
                continue;
            }
            for x in first_x..=last_x {
                let source_column = ((f64::from(x) + 0.5 - center) / scale
                    + f64::from(self.width) * 0.5)
                    .floor() as i32;
                if source_column < 0
                    || source_column >= self.width as i32
                    || self.pixels
                        [source_row as usize * self.width as usize + source_column as usize]
                        == 0
                {
                    continue;
                }
                let offset = (y as usize * canvas_width as usize + x as usize) * 4;
                let color = if alpha == 256 {
                    projection.color
                } else {
                    blend_rgb565(
                        projection.color,
                        rgba_rgb565(&canvas[offset..offset + 4]),
                        alpha,
                    )
                };
                write_rgb565(&mut canvas[offset..offset + 4], color);
            }
        }
        Ok(())
    }
}

impl Projection {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
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
        canvas_height: u32,
    ) -> Result<Self, String> {
        if speed_denominator == 0 || cycle == 0 || camera_distance == 0 || maximum_depth == 0 {
            return Err("projection scale and depth parameters must be positive".to_owned());
        }
        if !(0 <= horizon_y
            && horizon_y < clip_top
            && clip_top <= fade_invisible_y
            && fade_invisible_y < fade_opaque_y
            && fade_opaque_y <= bottom_y
            && bottom_y <= canvas_height as i32)
        {
            return Err("projection vertical geometry is invalid".to_owned());
        }
        let speed = f64::from(speed_numerator) / f64::from(speed_denominator);
        let scroll = (elapsed_ms.max(0) as f64 * speed) % f64::from(cycle);
        Ok(Self {
            scroll,
            camera_distance: f64::from(camera_distance),
            maximum_depth: f64::from(maximum_depth),
            horizon_y,
            clip_top,
            fade_invisible_y,
            fade_opaque_y,
            bottom_y,
            color: color_rgb565(color),
        })
    }

    fn screen_y(&self, source_y: f64) -> f64 {
        let depth = self.scroll - source_y;
        let scale = self.camera_distance / (self.camera_distance + depth);
        f64::from(self.horizon_y) + f64::from(self.bottom_y - self.horizon_y) * scale
    }

    fn alpha(&self, screen_y: i32) -> i32 {
        if screen_y <= self.fade_invisible_y {
            0
        } else if screen_y >= self.fade_opaque_y {
            256
        } else {
            (screen_y - self.fade_invisible_y) * 256 / (self.fade_opaque_y - self.fade_invisible_y)
        }
    }
}

fn color_rgb565(color: u32) -> u16 {
    rgb565((color >> 16) as u8, (color >> 8) as u8, color as u8)
}

fn rgb565(red: u8, green: u8, blue: u8) -> u16 {
    ((u16::from(red & 0xf8)) << 8) | ((u16::from(green & 0xfc)) << 3) | u16::from(blue >> 3)
}

fn rgba_rgb565(rgba: &[u8]) -> u16 {
    rgb565(rgba[0], rgba[1], rgba[2])
}

fn blend_rgb565(foreground: u16, background: u16, alpha: i32) -> u16 {
    let inverse = 256 - alpha;
    let red = (i32::from((foreground >> 11) & 0x1f) * alpha
        + i32::from((background >> 11) & 0x1f) * inverse
        + 128)
        >> 8;
    let green = (i32::from((foreground >> 5) & 0x3f) * alpha
        + i32::from((background >> 5) & 0x3f) * inverse
        + 128)
        >> 8;
    let blue =
        (i32::from(foreground & 0x1f) * alpha + i32::from(background & 0x1f) * inverse + 128) >> 8;
    ((red << 11) | (green << 5) | blue) as u16
}

fn write_rgb565(rgba: &mut [u8], color: u16) {
    let red = ((color >> 11) & 0x1f) as u8;
    let green = ((color >> 5) & 0x3f) as u8;
    let blue = (color & 0x1f) as u8;
    rgba.copy_from_slice(&[
        (red << 3) | (red >> 2),
        (green << 2) | (green >> 4),
        (blue << 3) | (blue >> 2),
        255,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_rgb565(canvas: &[u8]) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for rgba in canvas.chunks_exact(4) {
            let pixel = rgba_rgb565(rgba);
            hash = (hash ^ u64::from(pixel & 0xff)).wrapping_mul(0x100000001b3);
            hash = (hash ^ u64::from(pixel >> 8)).wrapping_mul(0x100000001b3);
        }
        hash
    }

    fn draw_starfield(canvas: &mut [u8], size: (u32, u32), color: u16) {
        for index in 0..96_u32 {
            if index % 7 == 0 {
                continue;
            }
            let x = (index * 193 + 47) % size.0;
            let y = (index * 83 + 29) % size.1;
            let side = if index % 11 == 0 { 2 } else { 1 };
            for row in y..(y + side).min(size.1) {
                for column in x..(x + side).min(size.0) {
                    let offset = (row as usize * size.0 as usize + column as usize) * 4;
                    write_rgb565(&mut canvas[offset..offset + 4], color);
                }
            }
        }
    }

    fn draw_close(canvas: &mut [u8], size: (u32, u32), color: u16) {
        for offset in (-12..=12).step_by(4) {
            for (left, top) in [(1240 + offset, 40 + offset), (1240 + offset, 40 - offset)] {
                for y in top..top + 4 {
                    for x in left..left + 4 {
                        let index = (y as usize * size.0 as usize + x as usize) * 4;
                        write_rgb565(&mut canvas[index..index + 4], color);
                    }
                }
            }
        }
    }

    #[test]
    fn matches_the_cpp_projected_line_fixture() {
        let size = (1280, 480);
        let mask = TextMask::new(b"HHHHHHHHHH", 4, size).unwrap();
        assert_eq!((mask.width, mask.height), (236, 28));
        for (elapsed_ms, expected) in [
            (1000, 0xd10a_d74c_8e29_e39b),
            (3000, 0x0281_da94_51fb_064d),
            (50000, 0x1b6e_2c79_2c1c_8f1b),
            (55000, 0xe693_4df8_e360_bd4b),
        ] {
            let mut canvas = vec![0; size.0 as usize * size.1 as usize * 4];
            for pixel in canvas.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[0, 0, 0, 255]);
            }
            draw_starfield(&mut canvas, size, color_rgb565(0x949494));
            let projection = Projection::new(
                elapsed_ms, 1, 20, 4044, 420, 4000, 56, 72, 104, 210, 480, 0xffffaf, size.1,
            )
            .unwrap();
            mask.draw(&mut canvas, size, 0, &projection).unwrap();
            draw_close(&mut canvas, size, color_rgb565(0x949494));
            assert_eq!(hash_rgb565(&canvas), expected);
        }
    }
}
