//! Checked frame geometry before borrowing core-owned pixel memory.

use std::error::Error;
use std::ffi::c_void;
use std::fmt;
use std::slice;

use retro_deck_platform::{
    display::{Dimensions, DisplayError, Frame},
    wayland::WaylandPresentationError,
};

use super::PixelFormat;

const MAXIMUM_FRAME_BYTES: usize = 64 * 1_024 * 1_024;

/// Validated source geometry for one libretro video callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VideoFrameLayout {
    format: PixelFormat,
    dimensions: Dimensions,
    stride: usize,
    elements: usize,
}

impl VideoFrameLayout {
    /// Validate dimensions, byte pitch, backing length, and allocation bounds.
    pub(super) fn new(
        format: PixelFormat,
        width: u32,
        height: u32,
        pitch_bytes: usize,
    ) -> Result<Self, VideoFrameError> {
        let width = usize::try_from(width).map_err(|_| VideoFrameError::InvalidDimensions)?;
        let height = usize::try_from(height).map_err(|_| VideoFrameError::InvalidDimensions)?;
        let dimensions =
            Dimensions::new(width, height).ok_or(VideoFrameError::InvalidDimensions)?;
        let bytes_per_pixel = match format {
            PixelFormat::Xrgb8888 => size_of::<u32>(),
            PixelFormat::Rgb565 => size_of::<u16>(),
        };
        if !pitch_bytes.is_multiple_of(bytes_per_pixel) {
            return Err(VideoFrameError::InvalidPitch);
        }
        let stride = pitch_bytes / bytes_per_pixel;
        if stride < width {
            return Err(VideoFrameError::InvalidPitch);
        }
        let elements = (height - 1)
            .checked_mul(stride)
            .and_then(|offset| offset.checked_add(width))
            .ok_or(VideoFrameError::TooLarge)?;
        let backing_bytes = elements
            .checked_mul(bytes_per_pixel)
            .ok_or(VideoFrameError::TooLarge)?;
        if backing_bytes > MAXIMUM_FRAME_BYTES {
            return Err(VideoFrameError::TooLarge);
        }
        Ok(Self {
            format,
            dimensions,
            stride,
            elements,
        })
    }

    /// Borrow a frame from the core-owned callback pointer.
    ///
    /// # Safety
    ///
    /// `data` must remain readable for this frame's validated backing length
    /// and for the complete returned lifetime. The core must not mutate or
    /// free those bytes while the returned frame is borrowed.
    pub(super) unsafe fn frame<'pixels>(
        self,
        data: *const c_void,
    ) -> Result<Frame<'pixels>, VideoFrameError> {
        if data.is_null() {
            return Err(VideoFrameError::NullData);
        }
        match self.format {
            PixelFormat::Xrgb8888 => {
                let pixels = data.cast::<u32>();
                if !pixels.is_aligned() {
                    return Err(VideoFrameError::UnalignedData);
                }
                // SAFETY: The caller guarantees the validated readable extent
                // and lifetime, and alignment was checked immediately above.
                let pixels = unsafe { slice::from_raw_parts(pixels, self.elements) };
                Frame::xrgb8888(pixels, self.dimensions, self.stride)
                    .map_err(VideoFrameError::Display)
            }
            PixelFormat::Rgb565 => {
                let pixels = data.cast::<u16>();
                if !pixels.is_aligned() {
                    return Err(VideoFrameError::UnalignedData);
                }
                // SAFETY: The caller guarantees the validated readable extent
                // and lifetime, and alignment was checked immediately above.
                let pixels = unsafe { slice::from_raw_parts(pixels, self.elements) };
                Frame::rgb565(pixels, self.dimensions, self.stride)
                    .map_err(VideoFrameError::Display)
            }
        }
    }
}

/// Invalid libretro frame metadata or backing pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoFrameError {
    /// Width or height is zero or exceeds the platform frame bound.
    InvalidDimensions,
    /// Pitch is not a whole pixel row or is narrower than visible width.
    InvalidPitch,
    /// Required backing memory overflows or exceeds 64 MiB.
    TooLarge,
    /// A non-duplicate frame has no pixel pointer.
    NullData,
    /// Pixel memory does not meet the native element alignment.
    UnalignedData,
    /// The shared frame validator rejected an otherwise checked layout.
    Display(DisplayError),
}

impl fmt::Display for VideoFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDimensions => "libretro frame dimensions are invalid",
            Self::InvalidPitch => "libretro frame pitch is invalid",
            Self::TooLarge => "libretro frame backing memory exceeds its bound",
            Self::NullData => "libretro frame data is null",
            Self::UnalignedData => "libretro frame data is not naturally aligned",
            Self::Display(_) => "libretro frame failed shared display validation",
        })
    }
}

impl Error for VideoFrameError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Display(source) => Some(source),
            Self::InvalidDimensions
            | Self::InvalidPitch
            | Self::TooLarge
            | Self::NullData
            | Self::UnalignedData => None,
        }
    }
}

/// Failure while validating or presenting one core video callback.
#[derive(Debug)]
pub enum VideoCallbackError {
    /// Core-owned frame metadata or memory was invalid.
    Frame(VideoFrameError),
    /// The checked Wayland presentation path failed.
    Presentation(WaylandPresentationError),
}

impl fmt::Display for VideoCallbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(source) => source.fmt(formatter),
            Self::Presentation(source) => source.fmt(formatter),
        }
    }
}

impl Error for VideoCallbackError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Frame(source) => Some(source),
            Self::Presentation(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn common_core_frames_have_exact_checked_layouts() {
        let nes = VideoFrameLayout::new(PixelFormat::Xrgb8888, 256, 240, 256 * 4)
            .expect("packed NES frame");
        assert_eq!(
            nes.dimensions,
            Dimensions::new(256, 240).expect("NES dimensions")
        );
        assert_eq!(nes.stride, 256);
        assert_eq!(nes.elements, 256 * 240);

        let game_boy = VideoFrameLayout::new(PixelFormat::Rgb565, 160, 144, 160 * 2)
            .expect("packed Game Boy frame");
        assert_eq!(game_boy.stride, 160);
        assert_eq!(game_boy.elements, 160 * 144);

        let padded =
            VideoFrameLayout::new(PixelFormat::Rgb565, 320, 240, 672).expect("padded ZX frame");
        assert_eq!(padded.stride, 336);
        assert_eq!(padded.elements, 336 * 239 + 320);
    }

    #[test]
    fn invalid_geometry_pitch_and_backing_bounds_are_rejected() {
        assert!(matches!(
            VideoFrameLayout::new(PixelFormat::Rgb565, 0, 240, 640),
            Err(VideoFrameError::InvalidDimensions)
        ));
        assert!(matches!(
            VideoFrameLayout::new(PixelFormat::Rgb565, 320, 240, 639),
            Err(VideoFrameError::InvalidPitch)
        ));
        assert!(matches!(
            VideoFrameLayout::new(PixelFormat::Xrgb8888, 320, 240, 319 * 4),
            Err(VideoFrameError::InvalidPitch)
        ));
        assert!(matches!(
            VideoFrameLayout::new(PixelFormat::Xrgb8888, 1, 2, usize::MAX - 3),
            Err(VideoFrameError::TooLarge)
        ));
        assert!(matches!(
            VideoFrameLayout::new(PixelFormat::Xrgb8888, 1, 2, MAXIMUM_FRAME_BYTES),
            Err(VideoFrameError::TooLarge)
        ));
        assert!(matches!(
            VideoFrameLayout::new(PixelFormat::Xrgb8888, 16_384, 1_024, 16_384 * 4),
            Err(VideoFrameError::InvalidDimensions)
        ));
    }

    #[test]
    fn checked_aligned_memory_becomes_the_expected_frame_variant() {
        let xrgb_layout =
            VideoFrameLayout::new(PixelFormat::Xrgb8888, 2, 2, 8).expect("small XRGB layout");
        let xrgb = [0_u32; 4];
        // SAFETY: The array is aligned and covers the complete checked layout.
        let frame = unsafe { xrgb_layout.frame(xrgb.as_ptr().cast()) }.expect("XRGB frame");
        assert!(matches!(frame, Frame::Xrgb8888 { stride: 2, .. }));

        let rgb_layout =
            VideoFrameLayout::new(PixelFormat::Rgb565, 2, 2, 4).expect("small RGB layout");
        let rgb = [0_u16; 4];
        // SAFETY: The array is aligned and covers the complete checked layout.
        let frame = unsafe { rgb_layout.frame(rgb.as_ptr().cast()) }.expect("RGB565 frame");
        assert!(matches!(frame, Frame::Rgb565 { stride: 2, .. }));
    }

    #[test]
    fn null_and_unaligned_data_are_rejected_before_slice_creation() {
        let layout =
            VideoFrameLayout::new(PixelFormat::Xrgb8888, 1, 1, 4).expect("one-pixel layout");
        // SAFETY: Null is explicitly rejected before a slice is formed.
        assert!(matches!(
            unsafe { layout.frame(ptr::null()) },
            Err(VideoFrameError::NullData)
        ));
        let words = [0_u32; 2];
        let unaligned = words.as_ptr().cast::<u8>().wrapping_add(1).cast();
        // SAFETY: The intentionally unaligned pointer is rejected before a
        // slice is formed and is never dereferenced.
        assert!(matches!(
            unsafe { layout.frame(unaligned) },
            Err(VideoFrameError::UnalignedData)
        ));
    }
}
