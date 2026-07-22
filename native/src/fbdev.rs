use crate::canvas;
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use std::cell::RefCell;
use std::ffi::{c_int, c_ulong, c_void};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::ptr;

const LOGICAL_WIDTH: usize = canvas::WIDTH as usize;
const LOGICAL_HEIGHT: usize = canvas::HEIGHT as usize;
const PHYSICAL_WIDTH: usize = 600;
const PHYSICAL_HEIGHT: usize = 1280;
const FBIOGET_VSCREENINFO: c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: c_ulong = 0x4602;
const FBIOBLANK: c_ulong = 0x4611;
const FB_BLANK_UNBLANK: c_int = 0;
const FB_TYPE_PACKED_PIXELS: u32 = 0;
const FB_VISUAL_TRUECOLOR: u32 = 2;
const EINVAL: i32 = 22;
const ENOTTY: i32 = 25;

unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Default)]
struct FbVariableScreenInfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transparency: FbBitfield,
    nonstandard: u32,
    activate: u32,
    height_mm: u32,
    width_mm: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_length: u32,
    vsync_length: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

#[repr(C)]
#[derive(Default)]
struct FbFixedScreenInfo {
    id: [u8; 16],
    smem_start: c_ulong,
    smem_len: u32,
    kind: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: c_ulong,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

const _: [(); 160] = [(); std::mem::size_of::<FbVariableScreenInfo>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 80] = [(); std::mem::size_of::<FbFixedScreenInfo>()];
#[cfg(target_pointer_width = "32")]
const _: [(); 68] = [(); std::mem::size_of::<FbFixedScreenInfo>()];

struct Geometry {
    stride: usize,
    map_size: usize,
}

fn validate_geometry(
    variable: &FbVariableScreenInfo,
    fixed: &FbFixedScreenInfo,
) -> Result<Geometry, String> {
    let rows = if variable.yres_virtual == 0 {
        variable.yres
    } else {
        variable.yres_virtual
    } as usize;
    let stride = fixed.line_length as usize;
    let required = stride
        .checked_mul(rows)
        .ok_or_else(|| "framebuffer geometry overflows the address space".to_owned())?;
    let map_size = fixed.smem_len as usize;

    if variable.xres != PHYSICAL_WIDTH as u32
        || variable.yres != PHYSICAL_HEIGHT as u32
        || variable.bits_per_pixel != 16
        || variable.xoffset != 0
        || variable.yoffset != 0
        || rows < PHYSICAL_HEIGHT
        || fixed.kind != FB_TYPE_PACKED_PIXELS
        || fixed.visual != FB_VISUAL_TRUECOLOR
        || fixed.line_length > c_int::MAX as u32
        || stride < PHYSICAL_WIDTH * 2
        || !stride.is_multiple_of(2)
        || map_size < required
        || variable.red.offset != 11
        || variable.red.length != 5
        || variable.red.msb_right != 0
        || variable.green.offset != 5
        || variable.green.length != 6
        || variable.green.msb_right != 0
        || variable.blue.offset != 0
        || variable.blue.length != 5
        || variable.blue.msb_right != 0
        || variable.transparency.length != 0
    {
        return Err(
            "unsupported framebuffer; expected 600x1280 RGB565 with a valid stride".to_owned(),
        );
    }
    Ok(Geometry { stride, map_size })
}

struct Mapping {
    pointer: *mut c_void,
    size: usize,
}

impl Mapping {
    fn new(file: &File, size: usize) -> Result<Self, String> {
        let pointer = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                file,
                0,
            )
        }
        .map_err(|error| format!("cannot map /dev/fb0: {error}"))?;
        Ok(Self { pointer, size })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            let _ = munmap(self.pointer, self.size);
        }
    }
}

struct Framebuffer {
    _file: File,
    mapping: Mapping,
    stride: usize,
    pixels: Vec<u16>,
    frame: Vec<u16>,
}

impl Framebuffer {
    fn open(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let mut variable = FbVariableScreenInfo::default();
        let mut fixed = FbFixedScreenInfo::default();
        if unsafe {
            ioctl(
                file.as_raw_fd(),
                FBIOGET_VSCREENINFO,
                &mut variable as *mut FbVariableScreenInfo,
            )
        } != 0
            || unsafe {
                ioctl(
                    file.as_raw_fd(),
                    FBIOGET_FSCREENINFO,
                    &mut fixed as *mut FbFixedScreenInfo,
                )
            } != 0
        {
            return Err(format!(
                "cannot query framebuffer geometry: {}",
                io::Error::last_os_error()
            ));
        }
        let geometry = validate_geometry(&variable, &fixed)?;
        let mapping = Mapping::new(&file, geometry.map_size)?;
        let blank_result = unsafe { ioctl(file.as_raw_fd(), FBIOBLANK, FB_BLANK_UNBLANK) };
        if blank_result != 0 {
            let error = io::Error::last_os_error();
            if !matches!(error.raw_os_error(), Some(EINVAL | ENOTTY)) {
                eprintln!("retrodeck: warning: cannot unblank framebuffer: {error}");
            }
        }
        Ok(Self {
            _file: file,
            mapping,
            stride: geometry.stride,
            pixels: vec![0; LOGICAL_WIDTH * LOGICAL_HEIGHT],
            frame: vec![0; geometry.map_size / 2],
        })
    }

    fn present_solid(&mut self, color: u32) -> Result<(), String> {
        self.pixels.fill(rgb565(color));
        self.publish()
    }

    fn present_rgba(&mut self, rgba: &[u8]) -> Result<(), String> {
        if !convert_rgba_to_rgb565(rgba, &mut self.pixels) {
            return Err("native canvas dimensions are invalid".to_owned());
        }
        self.publish()
    }

    fn publish(&mut self) -> Result<(), String> {
        let row_words = self.stride / 2;
        if !stage_canvas_for_scanout(&self.pixels, row_words, &mut self.frame) {
            return Err("framebuffer staging buffer is unavailable".to_owned());
        }
        let active_row_bytes = LOGICAL_HEIGHT * 2;
        for physical_row in 0..PHYSICAL_HEIGHT {
            let source = unsafe {
                self.frame
                    .as_ptr()
                    .add(physical_row * row_words)
                    .cast::<u8>()
            };
            let destination = unsafe {
                self.mapping
                    .pointer
                    .cast::<u8>()
                    .add(physical_row * self.stride)
            };
            unsafe {
                ptr::copy_nonoverlapping(source, destination, active_row_bytes);
            }
        }
        Ok(())
    }
}

fn rgb565(color: u32) -> u16 {
    let red = (color >> 16) & 0xff;
    let green = (color >> 8) & 0xff;
    let blue = color & 0xff;
    (((red & 0xf8) << 8) | ((green & 0xfc) << 3) | (blue >> 3)) as u16
}

fn convert_rgba_to_rgb565(rgba: &[u8], pixels: &mut [u16]) -> bool {
    if rgba.len() != LOGICAL_WIDTH * LOGICAL_HEIGHT * 4
        || pixels.len() != LOGICAL_WIDTH * LOGICAL_HEIGHT
    {
        return false;
    }
    for (pixel, color) in pixels.iter_mut().zip(rgba.chunks_exact(4)) {
        *pixel =
            rgb565((u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2]));
    }
    true
}

fn stage_canvas_for_scanout(canvas: &[u16], row_words: usize, frame: &mut [u16]) -> bool {
    let Some(required) = row_words.checked_mul(PHYSICAL_HEIGHT) else {
        return false;
    };
    if canvas.len() != LOGICAL_WIDTH * LOGICAL_HEIGHT
        || row_words < PHYSICAL_WIDTH
        || frame.len() < required
    {
        return false;
    }
    for logical_x in 0..LOGICAL_WIDTH {
        let physical_row = PHYSICAL_HEIGHT - 1 - logical_x;
        let destination = &mut frame[physical_row * row_words..][..LOGICAL_HEIGHT];
        for logical_y in 0..LOGICAL_HEIGHT {
            destination[logical_y] = canvas[logical_y * LOGICAL_WIDTH + logical_x];
        }
    }
    true
}

thread_local! {
    static FRAMEBUFFER: RefCell<Option<Framebuffer>> = const { RefCell::new(None) };
}

pub fn open() -> Result<(), String> {
    FRAMEBUFFER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Framebuffer::open(Path::new("/dev/fb0"))?);
        }
        Ok(())
    })
}

pub fn close() {
    FRAMEBUFFER.with(|slot| *slot.borrow_mut() = None);
}

pub fn present_solid(color: u32) -> Result<(), String> {
    FRAMEBUFFER.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .ok_or_else(|| "fbdev display is not open".to_owned())?
            .present_solid(color)
    })
}

pub fn present_rgba(rgba: &[u8]) -> Result<(), String> {
    FRAMEBUFFER.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .ok_or_else(|| "fbdev display is not open".to_owned())?
            .present_rgba(rgba)
    })
}

pub fn size() -> Option<(u32, u32)> {
    FRAMEBUFFER.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|_| (LOGICAL_WIDTH as u32, LOGICAL_HEIGHT as u32))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    fn canvas_hash(canvas: &[u16]) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for pixel in canvas {
            hash ^= u64::from(pixel & 0xff);
            hash = hash.wrapping_mul(0x100000001b3);
            hash ^= u64::from(pixel >> 8);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    fn valid_geometry() -> (FbVariableScreenInfo, FbFixedScreenInfo) {
        let variable = FbVariableScreenInfo {
            xres: PHYSICAL_WIDTH as u32,
            yres: PHYSICAL_HEIGHT as u32,
            xres_virtual: PHYSICAL_WIDTH as u32,
            yres_virtual: PHYSICAL_HEIGHT as u32,
            bits_per_pixel: 16,
            red: FbBitfield {
                offset: 11,
                length: 5,
                msb_right: 0,
            },
            green: FbBitfield {
                offset: 5,
                length: 6,
                msb_right: 0,
            },
            blue: FbBitfield {
                offset: 0,
                length: 5,
                msb_right: 0,
            },
            ..FbVariableScreenInfo::default()
        };
        let fixed = FbFixedScreenInfo {
            smem_len: (PHYSICAL_WIDTH * PHYSICAL_HEIGHT * 2) as u32,
            kind: FB_TYPE_PACKED_PIXELS,
            visual: FB_VISUAL_TRUECOLOR,
            line_length: (PHYSICAL_WIDTH * 2) as u32,
            ..FbFixedScreenInfo::default()
        };
        (variable, fixed)
    }

    #[test]
    fn matches_linux_framebuffer_layout_and_geometry() {
        assert_eq!(mem::size_of::<FbVariableScreenInfo>(), 160);
        assert_eq!(
            mem::size_of::<FbFixedScreenInfo>(),
            if usize::BITS == 64 { 80 } else { 68 }
        );
        let (variable, fixed) = valid_geometry();
        let geometry = validate_geometry(&variable, &fixed).unwrap();
        assert_eq!(geometry.stride, 1200);
        assert_eq!(geometry.map_size, 1_536_000);
    }

    #[test]
    fn rejects_incompatible_framebuffer_geometry() {
        let (mut variable, fixed) = valid_geometry();
        variable.red.offset = 10;
        assert!(validate_geometry(&variable, &fixed).is_err());

        let (variable, mut fixed) = valid_geometry();
        fixed.line_length = 1198;
        assert!(validate_geometry(&variable, &fixed).is_err());
    }

    #[test]
    fn matches_cpp_ui_fixture() {
        canvas::clear(0x000000);
        canvas::fill_rect(104, 100, 192, 80, 0xfe6c27);
        canvas::fill_rect(100, 104, 200, 72, 0xfe6c27);
        canvas::fill_rect(108, 104, 184, 72, 0x121212);
        canvas::fill_rect(104, 108, 192, 64, 0x121212);
        canvas::fill_rect(340, 100, 180, 4, 0xeeeeee);
        canvas::fill_rect(340, 176, 180, 4, 0xeeeeee);
        canvas::fill_rect(340, 100, 4, 80, 0xeeeeee);
        canvas::fill_rect(516, 100, 4, 80, 0xeeeeee);
        for (index, character) in b"RETRO".iter().enumerate() {
            canvas::draw_glyph(171 + index as i32 * 12, 133, *character, 2, 0xeeeeee);
        }
        for (index, character) in b"ABCDE...".iter().enumerate() {
            canvas::draw_glyph(383 + index as i32 * 12, 133, *character, 2, 0xeeeeee);
        }
        canvas::draw_glyph(10, 10, b'A', 1, 0xffffaf);
        canvas::draw_glyph(16, 10, b'?', 1, 0xffffaf);

        let mut pixels = vec![0; LOGICAL_WIDTH * LOGICAL_HEIGHT];
        canvas::with_pixels(|rgba| assert!(convert_rgba_to_rgb565(rgba, &mut pixels)));
        // Shared with tests/menu_ui_test.cpp.
        assert_eq!(canvas_hash(&pixels), 0x4140_7945_3e13_44d5);
    }

    #[test]
    fn rotates_the_logical_canvas_into_rgb565_scanout() {
        assert_eq!(rgb565(0x000000), 0x0000);
        assert_eq!(rgb565(0xffffff), 0xffff);
        assert_eq!(rgb565(0xfe6c27), 0xfb64);
        let mut rgba = vec![0; LOGICAL_WIDTH * LOGICAL_HEIGHT * 4];
        rgba[..4].copy_from_slice(&[0xfe, 0x6c, 0x27, 0xff]);
        let mut pixels = vec![0; LOGICAL_WIDTH * LOGICAL_HEIGHT];
        assert!(convert_rgba_to_rgb565(&rgba, &mut pixels));
        assert_eq!(pixels[0], 0xfb64);

        let row_words = PHYSICAL_WIDTH + 3;
        let mut canvas = vec![0; LOGICAL_WIDTH * LOGICAL_HEIGHT];
        canvas[0] = 0x1234;
        let last = canvas.len() - 1;
        canvas[last] = 0xabcd;
        let mut frame = vec![0xdead; row_words * PHYSICAL_HEIGHT];
        assert!(stage_canvas_for_scanout(&canvas, row_words, &mut frame));
        assert_eq!(frame[(PHYSICAL_HEIGHT - 1) * row_words], 0x1234);
        assert_eq!(frame[LOGICAL_HEIGHT - 1], 0xabcd);
        assert_eq!(frame[LOGICAL_HEIGHT], 0xdead);
        assert_eq!(frame[row_words - 1], 0xdead);
    }
}
