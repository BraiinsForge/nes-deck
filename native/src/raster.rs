use crate::regular_file::read_regular;
use png::{BitDepth, ColorType, Decoder, Transformations};
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

const MAXIMUM_PNG_COVER_BYTES: u64 = 4 * 1024 * 1024;
const MAXIMUM_COVER_DIMENSION: u32 = 2048;
const MAXIMUM_COVER_WIDTH: u32 = 600;
const MAXIMUM_COVER_HEIGHT: u32 = 378;
const MAXIMUM_PPM_COVER_BYTES: u64 =
    1024 + MAXIMUM_COVER_WIDTH as u64 * MAXIMUM_COVER_HEIGHT as u64 * 3;
const MAXIMUM_PNG_BYTES: u64 = 65536;
const MAXIMUM_RASTER_DIMENSION: u32 = 2048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RasterPixel {
    color: u16,
    alpha: u8,
}

pub(crate) struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<RasterPixel>,
}

struct DecodedPng {
    width: u32,
    height: u32,
    pixels: Vec<[u8; 4]>,
}

impl Raster {
    pub(crate) fn load_cover(path: &Path, background: u32) -> Result<Option<Self>, String> {
        if path.extension().is_some_and(|extension| extension == "ppm") {
            return load_ppm_cover(path);
        }
        if let Some(data) = read_regular(path, 8, MAXIMUM_PNG_COVER_BYTES, "cover")? {
            return decode_cover(&data, background)
                .map(Some)
                .map_err(|error| format!("cannot decode cover PNG {}: {error}", path.display()));
        }
        load_ppm_cover(&path.with_extension("ppm"))
    }

    pub(crate) fn load_png(
        path: &Path,
        expected_width: u32,
        expected_height: u32,
    ) -> Result<Option<Self>, String> {
        let Some(data) = read_regular(path, 8, MAXIMUM_PNG_BYTES, "raster")? else {
            return Ok(None);
        };
        decode_raster(&data, expected_width, expected_height)
            .map(Some)
            .map_err(|error| format!("cannot decode raster PNG {}: {error}", path.display()))
    }

    pub(crate) fn draw(
        &self,
        canvas: &mut [u8],
        canvas_size: (u32, u32),
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let (canvas_width, canvas_height) = canvas_size;
        if width == 0 || height == 0 {
            return Err("raster destination dimensions must be positive".to_owned());
        }
        if canvas.len() != canvas_width as usize * canvas_height as usize * 4 {
            return Err("native canvas dimensions are invalid".to_owned());
        }

        let source_size = self.width.min(self.height);
        let source_left = (self.width - source_size) / 2;
        let source_top = (self.height - source_size) / 2;
        let destination_left = i64::from(x);
        let destination_top = i64::from(y);
        let left = destination_left.clamp(0, i64::from(canvas_width));
        let top = destination_top.clamp(0, i64::from(canvas_height));
        let right = destination_left
            .saturating_add(i64::from(width))
            .clamp(0, i64::from(canvas_width));
        let bottom = destination_top
            .saturating_add(i64::from(height))
            .clamp(0, i64::from(canvas_height));
        if right <= left || bottom <= top {
            return Ok(());
        }

        for destination_y in top..bottom {
            let relative_y = (destination_y - destination_top) as u64;
            let source_y =
                source_top + (relative_y * u64::from(source_size) / u64::from(height)) as u32;
            for destination_x in left..right {
                let relative_x = (destination_x - destination_left) as u64;
                let source_x =
                    source_left + (relative_x * u64::from(source_size) / u64::from(width)) as u32;
                let source =
                    self.pixels[(source_y as usize * self.width as usize) + source_x as usize];
                if source.alpha == 0 {
                    continue;
                }
                let offset =
                    ((destination_y as usize * canvas_width as usize) + destination_x as usize) * 4;
                let color = if source.alpha == 255 {
                    source.color
                } else {
                    blend_rgb565(
                        source.color,
                        rgba_rgb565(&canvas[offset..offset + 4]),
                        source.alpha,
                    )
                };
                write_rgb565(&mut canvas[offset..offset + 4], color);
            }
        }
        Ok(())
    }
}

fn load_ppm_cover(path: &Path) -> Result<Option<Raster>, String> {
    let Some(data) = read_regular(path, 12, MAXIMUM_PPM_COVER_BYTES, "cover")? else {
        return Ok(None);
    };
    decode_ppm_cover(&data)
        .map(Some)
        .map_err(|error| format!("cannot decode cover PPM {}: {error}", path.display()))
}

fn decode_ppm_cover(data: &[u8]) -> Result<Raster, String> {
    let mut offset = 0;
    let magic = next_ppm_token(data, &mut offset);
    let width = next_ppm_token(data, &mut offset)
        .and_then(|token| parse_ppm_dimension(token, MAXIMUM_COVER_WIDTH));
    let height = next_ppm_token(data, &mut offset)
        .and_then(|token| parse_ppm_dimension(token, MAXIMUM_COVER_HEIGHT));
    let maximum = next_ppm_token(data, &mut offset);
    if magic != Some(b"P6".as_slice())
        || width.is_none()
        || height.is_none()
        || maximum != Some(b"255".as_slice())
        || offset >= data.len()
        || !ppm_space(data[offset])
    {
        return Err("invalid P6 header".to_owned());
    }
    if data[offset] == b'\r' && data.get(offset + 1) == Some(&b'\n') {
        offset += 2;
    } else {
        offset += 1;
    }
    let width = width.unwrap();
    let height = height.unwrap();
    let pixel_count = width as usize * height as usize;
    if data.len() - offset != pixel_count * 3 {
        return Err("pixel data has the wrong size".to_owned());
    }
    let mut pixels = Vec::with_capacity(pixel_count);
    for source in data[offset..].chunks_exact(3) {
        let color = xterm_rgb565(source[0], source[1], source[2])
            .ok_or_else(|| "pixel color is outside xterm-256".to_owned())?;
        pixels.push(RasterPixel { color, alpha: 255 });
    }
    Ok(Raster {
        width,
        height,
        pixels,
    })
}

fn next_ppm_token<'a>(data: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    while *offset < data.len() {
        if ppm_space(data[*offset]) {
            *offset += 1;
        } else if data[*offset] == b'#' {
            while *offset < data.len() && data[*offset] != b'\n' {
                *offset += 1;
            }
        } else {
            break;
        }
    }
    let start = *offset;
    while *offset < data.len() && !ppm_space(data[*offset]) && data[*offset] != b'#' {
        *offset += 1;
    }
    let length = *offset - start;
    (length > 0 && length <= 32).then_some(&data[start..*offset])
}

fn parse_ppm_dimension(token: &[u8], maximum: u32) -> Option<u32> {
    if token.is_empty() || token[0] == b'0' {
        return None;
    }
    let mut value = 0u32;
    for digit in token {
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(*digit - b'0');
        if value > maximum {
            return None;
        }
    }
    Some(value)
}

fn ppm_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn xterm_rgb565(red: u8, green: u8, blue: u8) -> Option<u16> {
    (0..256).find_map(|index| {
        (xterm_color(index) == [red, green, blue]).then_some(rgb565(red, green, blue))
    })
}

fn decode_cover(data: &[u8], background: u32) -> Result<Raster, String> {
    let decoded = decode_png(data, MAXIMUM_COVER_DIMENSION, MAXIMUM_COVER_DIMENSION)?;
    let (target_width, target_height) = cover_dimensions(decoded.width, decoded.height);
    let mut pixels = Vec::with_capacity(target_width as usize * target_height as usize);
    let background = [
        (background >> 16) & 0xff,
        (background >> 8) & 0xff,
        background & 0xff,
    ];
    let quantized = xterm_quantization_table();
    for y in 0..target_height {
        let source_y = (u64::from(y) * u64::from(decoded.height) / u64::from(target_height)) as u32;
        for x in 0..target_width {
            let source_x =
                (u64::from(x) * u64::from(decoded.width) / u64::from(target_width)) as u32;
            let source =
                decoded.pixels[(source_y as usize * decoded.width as usize) + source_x as usize];
            let alpha = u32::from(source[3]);
            let inverse_alpha = 255 - alpha;
            let red = (u32::from(source[0]) * alpha + background[0] * inverse_alpha + 127) / 255;
            let green = (u32::from(source[1]) * alpha + background[1] * inverse_alpha + 127) / 255;
            let blue = (u32::from(source[2]) * alpha + background[2] * inverse_alpha + 127) / 255;
            let offset = (((red >> 3) << 10) | ((green >> 3) << 5) | (blue >> 3)) as usize;
            pixels.push(RasterPixel {
                color: quantized[offset],
                alpha: 255,
            });
        }
    }
    Ok(Raster {
        width: target_width,
        height: target_height,
        pixels,
    })
}

fn decode_raster(data: &[u8], expected_width: u32, expected_height: u32) -> Result<Raster, String> {
    if expected_width == 0
        || expected_height == 0
        || expected_width > MAXIMUM_RASTER_DIMENSION
        || expected_height > MAXIMUM_RASTER_DIMENSION
    {
        return Err(format!(
            "expected raster dimensions must be within 1..{MAXIMUM_RASTER_DIMENSION}"
        ));
    }
    let decoded = decode_png(data, expected_width, expected_height)?;
    if decoded.width != expected_width || decoded.height != expected_height {
        return Err(format!(
            "dimensions are {}x{}, expected {}x{}",
            decoded.width, decoded.height, expected_width, expected_height
        ));
    }
    let pixels = decoded
        .pixels
        .into_iter()
        .map(|pixel| RasterPixel {
            color: rgb565(pixel[0], pixel[1], pixel[2]),
            alpha: pixel[3],
        })
        .collect();
    Ok(Raster {
        width: expected_width,
        height: expected_height,
        pixels,
    })
}

fn decode_png(data: &[u8], maximum_width: u32, maximum_height: u32) -> Result<DecodedPng, String> {
    let mut decoder = Decoder::new(Cursor::new(data));
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let width = reader.info().width;
    let height = reader.info().height;
    if width == 0 || height == 0 || width > maximum_width || height > maximum_height {
        return Err(format!(
            "dimensions are outside 1..{} by 1..{}",
            maximum_width, maximum_height
        ));
    }
    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| "decoded image is too large".to_owned())?;
    let mut buffer = vec![0; buffer_size];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    if output.bit_depth != BitDepth::Eight || output.width != width || output.height != height {
        return Err("decoder returned unexpected image geometry".to_owned());
    }
    let bytes = &buffer[..output.buffer_size()];
    let channels = match output.color_type {
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        ColorType::Indexed => return Err("decoder left indexed pixels unexpanded".to_owned()),
    };
    let pixel_count = width as usize * height as usize;
    if bytes.len() != pixel_count * channels {
        return Err("decoder returned an incomplete image".to_owned());
    }
    let mut pixels = Vec::with_capacity(pixel_count);
    for pixel in bytes.chunks_exact(channels) {
        pixels.push(match output.color_type {
            ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
            ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
            ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
            ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
            ColorType::Indexed => unreachable!(),
        });
    }
    Ok(DecodedPng {
        width,
        height,
        pixels,
    })
}

fn cover_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width <= MAXIMUM_COVER_WIDTH && height <= MAXIMUM_COVER_HEIGHT {
        return (width, height);
    }
    if u64::from(width) * u64::from(MAXIMUM_COVER_HEIGHT)
        > u64::from(height) * u64::from(MAXIMUM_COVER_WIDTH)
    {
        (
            MAXIMUM_COVER_WIDTH,
            (u64::from(height) * u64::from(MAXIMUM_COVER_WIDTH) / u64::from(width)).max(1) as u32,
        )
    } else {
        (
            (u64::from(width) * u64::from(MAXIMUM_COVER_HEIGHT) / u64::from(height)).max(1) as u32,
            MAXIMUM_COVER_HEIGHT,
        )
    }
}

fn xterm_quantization_table() -> &'static [u16] {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = vec![0; 32 * 32 * 32];
        for red5 in 0..32 {
            let red = (red5 << 3) | (red5 >> 2);
            for green5 in 0..32 {
                let green = (green5 << 3) | (green5 >> 2);
                for blue5 in 0..32 {
                    let blue = (blue5 << 3) | (blue5 >> 2);
                    let mut best_distance = u32::MAX;
                    let mut best_color = 0;
                    for index in 0..256 {
                        let candidate = xterm_color(index);
                        let red_delta = red - i32::from(candidate[0]);
                        let green_delta = green - i32::from(candidate[1]);
                        let blue_delta = blue - i32::from(candidate[2]);
                        let distance = (red_delta * red_delta
                            + green_delta * green_delta
                            + blue_delta * blue_delta)
                            as u32;
                        if distance < best_distance {
                            best_distance = distance;
                            best_color = rgb565(candidate[0], candidate[1], candidate[2]);
                        }
                    }
                    table[((red5 << 10) | (green5 << 5) | blue5) as usize] = best_color;
                }
            }
        }
        table
    })
}

fn xterm_color(index: u32) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [128, 0, 0],
        [0, 128, 0],
        [128, 128, 0],
        [0, 0, 128],
        [128, 0, 128],
        [0, 128, 128],
        [192, 192, 192],
        [128, 128, 128],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [0, 0, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0..=15 => ANSI[index as usize],
        16..=231 => {
            let cube = index - 16;
            [
                CUBE[(cube / 36) as usize],
                CUBE[((cube / 6) % 6) as usize],
                CUBE[(cube % 6) as usize],
            ]
        }
        232..=255 => {
            let level = (8 + (index - 232) * 10) as u8;
            [level, level, level]
        }
        _ => [0, 0, 0],
    }
}

fn rgb565(red: u8, green: u8, blue: u8) -> u16 {
    ((u16::from(red & 0xf8)) << 8) | ((u16::from(green & 0xfc)) << 3) | u16::from(blue >> 3)
}

fn rgba_rgb565(rgba: &[u8]) -> u16 {
    rgb565(rgba[0], rgba[1], rgba[2])
}

fn blend_rgb565(foreground: u16, background: u16, alpha: u8) -> u16 {
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let red = (u32::from((foreground >> 11) & 0x1f) * alpha
        + u32::from((background >> 11) & 0x1f) * inverse
        + 127)
        / 255;
    let green = (u32::from((foreground >> 5) & 0x3f) * alpha
        + u32::from((background >> 5) & 0x3f) * inverse
        + 127)
        / 255;
    let blue =
        (u32::from(foreground & 0x1f) * alpha + u32::from(background & 0x1f) * inverse + 127) / 255;
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

    fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, width, height);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        data
    }

    fn pixel(canvas: &[u8], width: usize, x: usize, y: usize) -> u16 {
        let offset = (y * width + x) * 4;
        rgba_rgb565(&canvas[offset..offset + 4])
    }

    #[test]
    fn decodes_cover_alpha_resize_and_xterm_quantization() {
        let orange = xterm_color(202);
        let data = encode_rgba(
            2,
            1,
            &[255, 255, 255, 0, orange[0], orange[1], orange[2], 255],
        );
        let background =
            (u32::from(orange[0]) << 16) | (u32::from(orange[1]) << 8) | u32::from(orange[2]);
        let raster = decode_cover(&data, background).unwrap();
        assert_eq!((raster.width, raster.height), (2, 1));
        assert_eq!(
            raster.pixels[0].color,
            rgb565(orange[0], orange[1], orange[2])
        );
        assert_eq!(raster.pixels[0], raster.pixels[1]);
        assert_eq!(cover_dimensions(1200, 378), (600, 189));
        assert_eq!(cover_dimensions(600, 756), (300, 378));
    }

    #[test]
    fn draws_exact_center_crop_and_rgb565_alpha() {
        let pixels = (1..=8)
            .map(|index| {
                let color = xterm_color(index);
                RasterPixel {
                    color: rgb565(color[0], color[1], color[2]),
                    alpha: 255,
                }
            })
            .collect();
        let raster = Raster {
            width: 4,
            height: 2,
            pixels,
        };
        let mut canvas = vec![0; 4 * 4 * 4];
        for offset in (0..canvas.len()).step_by(4) {
            canvas[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        raster.draw(&mut canvas, (4, 4), 1, 1, 2, 2).unwrap();
        for (x, y, index) in [(1, 1, 2), (2, 1, 3), (1, 2, 6), (2, 2, 7)] {
            let color = xterm_color(index);
            assert_eq!(
                pixel(&canvas, 4, x, y),
                rgb565(color[0], color[1], color[2])
            );
        }

        let translucent = Raster {
            width: 1,
            height: 1,
            pixels: vec![RasterPixel {
                color: rgb565(160, 160, 160),
                alpha: 128,
            }],
        };
        translucent.draw(&mut canvas, (4, 4), 0, 0, 1, 1).unwrap();
        assert_eq!(
            pixel(&canvas, 4, 0, 0),
            blend_rgb565(rgb565(160, 160, 160), 0, 128)
        );
    }

    #[test]
    fn loads_ppm_only_after_a_missing_png() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("retrodeck-raster-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let png_path = directory.join("fixture.png");
        let ppm_path = directory.join("fixture.ppm");
        let first = xterm_color(202);
        let second = xterm_color(81);
        let mut ppm = b"P6\n# fixture\n2 1\n255\n".to_vec();
        ppm.extend_from_slice(&first);
        ppm.extend_from_slice(&second);
        std::fs::write(&ppm_path, &ppm).unwrap();

        let raster = Raster::load_cover(&png_path, 0).unwrap().unwrap();
        assert_eq!((raster.width, raster.height), (2, 1));
        assert_eq!(raster.pixels[0].color, rgb565(first[0], first[1], first[2]));
        assert_eq!(
            raster.pixels[1].color,
            rgb565(second[0], second[1], second[2])
        );

        std::fs::write(&png_path, b"not a png").unwrap();
        assert!(Raster::load_cover(&png_path, 0).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn validates_generic_png_dimensions() {
        let data = encode_rgba(1, 1, &[46, 46, 46, 255]);
        assert!(decode_raster(&data, 1, 1).is_ok());
        assert!(decode_raster(&data, 23, 23).is_err());
        assert!(decode_raster(&data, MAXIMUM_RASTER_DIMENSION + 1, 1).is_err());
    }
}
