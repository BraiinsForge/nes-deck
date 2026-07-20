//! Checked borrowing for interleaved libretro PCM callbacks.

use std::error::Error;
use std::fmt;
use std::slice;

/// Largest stereo batch borrowed from one core callback.
const MAXIMUM_AUDIO_CALLBACK_FRAMES: usize = 65_536;

/// Borrow one interleaved stereo S16 callback as frame pairs.
///
/// # Safety
///
/// For a nonzero `frames`, `data` must remain aligned and readable for
/// `frames * 2` samples throughout the returned lifetime. The core must not
/// mutate or free those samples while the slice is borrowed.
pub(super) unsafe fn stereo_frames<'samples>(
    data: *const i16,
    frames: usize,
) -> Result<&'samples [[i16; 2]], AudioBatchError> {
    if frames == 0 {
        return Ok(&[]);
    }
    if frames > MAXIMUM_AUDIO_CALLBACK_FRAMES {
        return Err(AudioBatchError::TooLarge);
    }
    if data.is_null() {
        return Err(AudioBatchError::NullData);
    }
    let stereo = data.cast::<[i16; 2]>();
    if !stereo.is_aligned() {
        return Err(AudioBatchError::UnalignedData);
    }
    // SAFETY: The caller guarantees the complete readable extent and
    // lifetime, and alignment was checked immediately above.
    Ok(unsafe { slice::from_raw_parts(stereo, frames) })
}

/// Invalid libretro PCM callback size or backing pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioBatchError {
    /// A nonempty callback has no PCM pointer.
    NullData,
    /// PCM memory is not naturally aligned for signed 16-bit samples.
    UnalignedData,
    /// One callback exceeds the fixed 65,536-frame safety bound.
    TooLarge,
}

impl fmt::Display for AudioBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NullData => "libretro PCM data is null",
            Self::UnalignedData => "libretro PCM data is not naturally aligned",
            Self::TooLarge => "libretro PCM callback exceeds its frame bound",
        })
    }
}

impl Error for AudioBatchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn aligned_interleaved_samples_are_borrowed_as_stereo_frames() {
        let samples = [1_i16, -1, 2, -2, 3, -3];
        // SAFETY: The array is aligned and contains three complete pairs.
        let frames = unsafe { stereo_frames(samples.as_ptr(), 3) }.expect("valid PCM batch");
        assert_eq!(frames, [[1, -1], [2, -2], [3, -3]]);
    }

    #[test]
    fn empty_callbacks_do_not_require_a_pointer() {
        // SAFETY: Zero frames form no slice from the null pointer.
        assert_eq!(unsafe { stereo_frames(ptr::null(), 0) }, Ok(&[][..]));
    }

    #[test]
    fn null_unaligned_and_excessive_batches_are_rejected() {
        // SAFETY: Null is rejected before a nonempty slice is formed.
        assert_eq!(
            unsafe { stereo_frames(ptr::null(), 1) },
            Err(AudioBatchError::NullData)
        );
        let words = [0_i16; 4];
        let unaligned = words.as_ptr().cast::<u8>().wrapping_add(1).cast();
        // SAFETY: The intentionally unaligned pointer is rejected before use.
        assert_eq!(
            unsafe { stereo_frames(unaligned, 1) },
            Err(AudioBatchError::UnalignedData)
        );
        // SAFETY: The excessive count is rejected before the pointer is read.
        assert_eq!(
            unsafe { stereo_frames(words.as_ptr(), MAXIMUM_AUDIO_CALLBACK_FRAMES + 1) },
            Err(AudioBatchError::TooLarge)
        );
    }
}
