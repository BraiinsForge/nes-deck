//! Allocation-stable PCM buffering and transforms.
//!
//! Emulator callbacks can downmix directly into [`MonoPcmQueue`] after their
//! caller acquires a nonblocking queue guard. The queue retains the newest
//! frames on overload so audio latency cannot grow without bound. Volume is a
//! separate in-place transform for the dedicated audio worker.

use std::collections::TryReserveError;
use std::fmt;
use std::num::NonZeroUsize;

use crate::Volume;

/// Result of adding one callback batch to a bounded PCM queue.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PcmPushReport {
    /// Newly submitted frames retained by the queue.
    pub accepted_frames: usize,
    /// Input or previously queued frames discarded to retain recent audio.
    pub dropped_frames: usize,
}

impl PcmPushReport {
    /// Whether overload interrupted stream continuity.
    #[must_use]
    pub const fn discontinuous(self) -> bool {
        self.dropped_frames != 0
    }
}

/// Preallocated first-in, first-out mono S16 PCM storage.
///
/// Pushes never allocate after construction. If a batch does not fit, the
/// oldest audio is discarded and the newest `capacity` frames are retained.
/// This bounds both memory and playback latency.
pub struct MonoPcmQueue {
    samples: Box<[i16]>,
    head: usize,
    len: usize,
    discontinuities: u64,
}

impl MonoPcmQueue {
    /// Allocate one fixed-capacity queue.
    ///
    /// # Errors
    ///
    /// Returns [`TryReserveError`] if the requested sample storage cannot be
    /// represented or allocated.
    pub fn try_new(capacity: NonZeroUsize) -> Result<Self, TryReserveError> {
        let mut samples = Vec::new();
        samples.try_reserve_exact(capacity.get())?;
        samples.resize(capacity.get(), 0);
        Ok(Self {
            samples: samples.into_boxed_slice(),
            head: 0,
            len: 0,
            discontinuities: 0,
        })
    }

    /// Maximum number of mono frames retained at once.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.samples.len()
    }

    /// Number of mono frames currently waiting.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no PCM frame is waiting.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Retain the newest mono frames from one producer batch.
    pub fn push_mono_latest(&mut self, samples: &[i16]) -> PcmPushReport {
        let (skip, report) = self.prepare_push(samples.len());
        let (_, retained) = samples.split_at(skip);
        self.write_mono(retained);
        report
    }

    /// Downmix and retain the newest interleaved stereo frames.
    ///
    /// Each frame is averaged with a signed 32-bit intermediate. This matches
    /// the established Deck mixer without overflowing at full scale.
    pub fn push_stereo_latest(&mut self, frames: &[[i16; 2]]) -> PcmPushReport {
        let (skip, report) = self.prepare_push(frames.len());
        let (_, retained) = frames.split_at(skip);
        self.write_stereo(retained);
        report
    }

    /// Remove up to `output.len()` oldest frames into caller-owned storage.
    pub fn pop_into(&mut self, output: &mut [i16]) -> usize {
        let count = output.len().min(self.len);
        if count == 0 {
            return 0;
        }

        let first_count = count.min(self.capacity() - self.head);
        let (first_output, second_output) = output.split_at_mut(first_count);
        let (_, queue_tail) = self.samples.split_at(self.head);
        let (first_input, _) = queue_tail.split_at(first_count);
        first_output.copy_from_slice(first_input);

        let second_count = count - first_count;
        if second_count != 0 {
            let (second_input, _) = self.samples.split_at(second_count);
            let (second_output, _) = second_output.split_at_mut(second_count);
            second_output.copy_from_slice(second_input);
        }

        self.discard_front(count);
        count
    }

    /// Discard all queued frames and record a stream discontinuity.
    pub const fn clear(&mut self) -> usize {
        let discarded = self.len;
        if discarded != 0 {
            self.head = 0;
            self.len = 0;
            self.record_discontinuity();
        }
        discarded
    }

    /// Take the number of overload or explicit-clear discontinuities.
    ///
    /// A streaming resampler should reset when this returns a nonzero value,
    /// rather than interpolate across samples that were discarded.
    pub fn take_discontinuities(&mut self) -> u64 {
        std::mem::take(&mut self.discontinuities)
    }

    fn prepare_push(&mut self, submitted: usize) -> (usize, PcmPushReport) {
        let accepted = submitted.min(self.capacity());
        let skipped_input = submitted - accepted;
        let available = self.capacity() - self.len;
        let discarded_queue = accepted.saturating_sub(available);
        self.discard_front(discarded_queue);
        let dropped = skipped_input.saturating_add(discarded_queue);
        if dropped != 0 {
            self.record_discontinuity();
        }
        (
            skipped_input,
            PcmPushReport {
                accepted_frames: accepted,
                dropped_frames: dropped,
            },
        )
    }

    fn write_mono(&mut self, samples: &[i16]) {
        let tail = (self.head + self.len) % self.capacity();
        let first_count = samples.len().min(self.capacity() - tail);
        let (first_input, second_input) = samples.split_at(first_count);
        {
            let (_, queue_tail) = self.samples.split_at_mut(tail);
            let (first_output, _) = queue_tail.split_at_mut(first_count);
            first_output.copy_from_slice(first_input);
        }
        if !second_input.is_empty() {
            let (second_output, _) = self.samples.split_at_mut(second_input.len());
            second_output.copy_from_slice(second_input);
        }
        self.len += samples.len();
    }

    fn write_stereo(&mut self, frames: &[[i16; 2]]) {
        let tail = (self.head + self.len) % self.capacity();
        let first_count = frames.len().min(self.capacity() - tail);
        let (first_input, second_input) = frames.split_at(first_count);
        {
            let (_, queue_tail) = self.samples.split_at_mut(tail);
            let (first_output, _) = queue_tail.split_at_mut(first_count);
            downmix_into(first_input, first_output);
        }
        if !second_input.is_empty() {
            let (second_output, _) = self.samples.split_at_mut(second_input.len());
            downmix_into(second_input, second_output);
        }
        self.len += frames.len();
    }

    fn discard_front(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        debug_assert!(count <= self.len);
        self.head = (self.head + count) % self.capacity();
        self.len -= count;
        if self.len == 0 {
            self.head = 0;
        }
    }

    const fn record_discontinuity(&mut self) {
        self.discontinuities = self.discontinuities.saturating_add(1);
    }
}

impl fmt::Debug for MonoPcmQueue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MonoPcmQueue")
            .field("capacity", &self.capacity())
            .field("len", &self.len)
            .field("discontinuities", &self.discontinuities)
            .finish_non_exhaustive()
    }
}

/// Apply validated gain to a mono PCM buffer in place.
pub fn apply_volume(samples: &mut [i16], volume: Volume) {
    let percent = i32::from(volume.percent());
    for sample in samples {
        let scaled = i32::from(*sample) * percent / 100;
        *sample = i16::try_from(scaled).unwrap_or(if scaled < 0 { i16::MIN } else { i16::MAX });
    }
}

fn downmix_into(input: &[[i16; 2]], output: &mut [i16]) {
    debug_assert_eq!(input.len(), output.len());
    for (frame, sample) in input.iter().zip(output) {
        let [left, right] = *frame;
        let mixed = (i32::from(left) + i32::from(right)) / 2;
        *sample = i16::try_from(mixed).unwrap_or(if mixed < 0 { i16::MIN } else { i16::MAX });
    }
}

#[cfg(test)]
mod tests {
    use super::{MonoPcmQueue, PcmPushReport, apply_volume};
    use crate::Volume;
    use std::num::NonZeroUsize;

    fn queue(capacity: usize) -> Option<MonoPcmQueue> {
        NonZeroUsize::new(capacity).and_then(|capacity| MonoPcmQueue::try_new(capacity).ok())
    }

    #[test]
    fn mono_queue_preserves_fifo_order_across_wraparound() {
        let Some(mut queue) = queue(5) else {
            return;
        };
        assert_eq!(
            queue.push_mono_latest(&[1, 2, 3, 4]),
            PcmPushReport {
                accepted_frames: 4,
                dropped_frames: 0,
            }
        );
        let mut first = [0_i16; 3];
        assert_eq!(queue.pop_into(&mut first), 3);
        assert_eq!(first, [1, 2, 3]);

        assert_eq!(queue.push_mono_latest(&[5, 6, 7, 8]).dropped_frames, 0);
        let mut wrapped = [0_i16; 6];
        assert_eq!(queue.pop_into(&mut wrapped), 5);
        assert_eq!(wrapped, [4, 5, 6, 7, 8, 0]);
        assert!(queue.is_empty());
    }

    #[test]
    fn overload_keeps_latest_audio_and_counts_each_discontinuity() {
        let Some(mut queue) = queue(4) else {
            return;
        };
        let _ = queue.push_mono_latest(&[1, 2, 3]);
        assert_eq!(
            queue.push_mono_latest(&[4, 5, 6]),
            PcmPushReport {
                accepted_frames: 3,
                dropped_frames: 2,
            }
        );
        let mut output = [0_i16; 4];
        assert_eq!(queue.pop_into(&mut output), 4);
        assert_eq!(output, [3, 4, 5, 6]);

        assert_eq!(
            queue.push_mono_latest(&[7, 8, 9, 10, 11, 12]),
            PcmPushReport {
                accepted_frames: 4,
                dropped_frames: 2,
            }
        );
        assert_eq!(queue.take_discontinuities(), 2);
        assert_eq!(queue.take_discontinuities(), 0);
        assert_eq!(queue.pop_into(&mut output), 4);
        assert_eq!(output, [9, 10, 11, 12]);
    }

    #[test]
    fn stereo_downmix_uses_wide_signed_arithmetic() {
        let Some(mut queue) = queue(5) else {
            return;
        };
        let frames = [
            [i16::MAX, i16::MAX],
            [i16::MIN, i16::MIN],
            [i16::MAX, i16::MIN],
            [20_000, -10_000],
            [-20_001, 10_000],
        ];
        assert_eq!(queue.push_stereo_latest(&frames).accepted_frames, 5);
        let mut output = [0_i16; 5];
        assert_eq!(queue.pop_into(&mut output), 5);
        assert_eq!(output, [i16::MAX, i16::MIN, 0, 5_000, -5_000]);
    }

    #[test]
    fn explicit_clear_is_observable_by_stateful_consumers() {
        let Some(mut queue) = queue(3) else {
            return;
        };
        let _ = queue.push_mono_latest(&[1, 2]);
        assert_eq!(queue.clear(), 2);
        assert_eq!(queue.clear(), 0);
        assert_eq!(queue.take_discontinuities(), 1);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn volume_is_worker_side_in_place_and_exact_at_bounds() {
        let Some(volume) = Volume::new(42) else {
            return;
        };
        let mut samples = [i16::MIN, -100, 0, 100, i16::MAX];
        apply_volume(&mut samples, volume);
        assert_eq!(samples, [-13_762, -42, 0, 42, 13_762]);

        apply_volume(&mut samples, Volume::MUTED);
        assert_eq!(samples, [0; 5]);

        let Some(full) = Volume::new(100) else {
            return;
        };
        let mut full_scale = [i16::MIN, i16::MAX];
        apply_volume(&mut full_scale, full);
        assert_eq!(full_scale, [i16::MIN, i16::MAX]);
    }
}
