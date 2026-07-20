//! Allocation-free streaming linear PCM resampling.

use crate::SampleRate;

/// Progress made by one bounded resampler call.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResampleReport {
    /// Source frames consumed or retained in the streaming state.
    pub consumed_frames: usize,
    /// Destination frames written into caller-owned storage.
    pub produced_frames: usize,
}

/// Streaming mono linear resampler with a two-sample maximum carry.
///
/// The resampler never allocates. It preserves fractional phase across input
/// batches, so changing callback boundaries does not change the output. A
/// caller must invoke [`Self::reset`] after dropping queued source frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinearResampler {
    source_rate: SampleRate,
    output_rate: SampleRate,
    left: Option<i16>,
    right: Option<i16>,
    position: u64,
}

impl LinearResampler {
    /// Start a stream between two validated sample rates.
    #[must_use]
    pub const fn new(source_rate: SampleRate, output_rate: SampleRate) -> Self {
        Self {
            source_rate,
            output_rate,
            left: None,
            right: None,
            position: 0,
        }
    }

    /// Source frames expected per second.
    #[must_use]
    pub const fn source_rate(&self) -> SampleRate {
        self.source_rate
    }

    /// Destination frames generated per source second.
    #[must_use]
    pub const fn output_rate(&self) -> SampleRate {
        self.output_rate
    }

    /// Whether no rate conversion is required.
    #[must_use]
    pub const fn is_passthrough(&self) -> bool {
        self.source_rate.get() == self.output_rate.get()
    }

    /// Convert as much source PCM as the output slice permits.
    ///
    /// One source frame may be retained internally after the destination
    /// fills. Call again with the unconsumed input and fresh output storage.
    /// If [`Self::has_pending_output`] is true after all input was consumed,
    /// call again with an empty input slice to finish that interpolation.
    pub fn process(&mut self, input: &[i16], output: &mut [i16]) -> ResampleReport {
        if output.is_empty() {
            return ResampleReport::default();
        }

        let mut consumed = 0;
        let mut produced = 0;
        loop {
            if self.left.is_none() {
                let (Some(source), Some(destination)) =
                    (input.get(consumed), output.get_mut(produced))
                else {
                    break;
                };
                *destination = *source;
                self.left = Some(*source);
                self.position = u64::from(self.source_rate.get());
                consumed += 1;
                produced += 1;
                continue;
            }

            if self.right.is_none() {
                let Some(source) = input.get(consumed) else {
                    break;
                };
                self.right = Some(*source);
                consumed += 1;
            }

            let output_rate = u64::from(self.output_rate.get());
            if self.position > output_rate {
                self.position -= output_rate;
                self.left = self.right.take();
                continue;
            }

            let Some(destination) = output.get_mut(produced) else {
                break;
            };
            let (Some(left), Some(right)) = (self.left, self.right) else {
                break;
            };
            *destination = interpolate(left, right, self.position, output_rate);
            self.position += u64::from(self.source_rate.get());
            produced += 1;
        }

        ResampleReport {
            consumed_frames: consumed,
            produced_frames: produced,
        }
    }

    /// Whether output remains for a source frame already retained internally.
    #[must_use]
    pub const fn has_pending_output(&self) -> bool {
        self.right.is_some() && self.position <= self.output_rate.get() as u64
    }

    /// Forget interpolation history after a queue drop or forced release.
    pub const fn reset(&mut self) {
        self.left = None;
        self.right = None;
        self.position = 0;
    }
}

fn interpolate(left: i16, right: i16, numerator: u64, denominator: u64) -> i16 {
    debug_assert!(denominator != 0);
    debug_assert!(numerator <= denominator);
    let difference = i64::from(right) - i64::from(left);
    let scaled = difference * i64::try_from(numerator).unwrap_or(i64::MAX)
        / i64::try_from(denominator).unwrap_or(i64::MAX);
    let sample = i64::from(left) + scaled;
    i16::try_from(sample).unwrap_or(if sample < 0 { i16::MIN } else { i16::MAX })
}

#[cfg(test)]
mod tests {
    use super::{LinearResampler, ResampleReport};
    use crate::SampleRate;

    fn rate(hertz: u32) -> Option<SampleRate> {
        SampleRate::new(hertz)
    }

    #[test]
    fn equal_rates_preserve_every_sample_across_small_outputs() {
        let (Some(source), Some(output)) = (rate(44_100), rate(44_100)) else {
            return;
        };
        let mut resampler = LinearResampler::new(source, output);
        assert!(resampler.is_passthrough());
        let input = [1_i16, 2, 3];
        let mut first = [0_i16; 2];
        assert_eq!(
            resampler.process(&input, &mut first),
            ResampleReport {
                consumed_frames: 3,
                produced_frames: 2,
            }
        );
        assert_eq!(first, [1, 2]);
        assert!(resampler.has_pending_output());

        let mut last = [0_i16; 1];
        assert_eq!(
            resampler.process(&[], &mut last),
            ResampleReport {
                consumed_frames: 0,
                produced_frames: 1,
            }
        );
        assert_eq!(last, [3]);
        assert!(!resampler.has_pending_output());
    }

    #[test]
    fn upsampling_interpolates_across_callback_boundaries() {
        let (Some(source), Some(output)) = (rate(2), rate(4)) else {
            return;
        };
        let mut resampler = LinearResampler::new(source, output);
        let mut first = [0_i16; 2];
        assert_eq!(
            resampler.process(&[0, 1_000], &mut first).produced_frames,
            2
        );
        assert_eq!(first, [0, 500]);
        assert!(resampler.has_pending_output());

        let mut second = [0_i16; 3];
        let report = resampler.process(&[2_000], &mut second);
        assert_eq!(report.consumed_frames, 1);
        assert_eq!(report.produced_frames, 3);
        assert_eq!(second, [1_000, 1_500, 2_000]);
    }

    #[test]
    fn downsampling_retains_the_stream_clock_without_aliasing_batches() {
        let (Some(source), Some(output)) = (rate(4), rate(2)) else {
            return;
        };
        let mut resampler = LinearResampler::new(source, output);
        let mut samples = [0_i16; 3];
        let report = resampler.process(&[0, 10, 20, 30, 40], &mut samples);
        assert_eq!(report.consumed_frames, 5);
        assert_eq!(report.produced_frames, 3);
        assert_eq!(samples, [0, 20, 40]);
        assert!(!resampler.has_pending_output());
    }

    #[test]
    fn callback_partitioning_does_not_change_fractional_output() {
        let (Some(source), Some(output)) = (rate(4), rate(6)) else {
            return;
        };
        let input = [0_i16, 1_000, 2_000, 3_000, 4_000];

        let mut whole = LinearResampler::new(source, output);
        let mut whole_output = [0_i16; 7];
        assert_eq!(whole.process(&input, &mut whole_output).produced_frames, 7);

        let mut partitioned = LinearResampler::new(source, output);
        let mut partitioned_output = Vec::new();
        let (first_input, remaining) = input.split_at(2);
        let (second_input, third_input) = remaining.split_at(1);
        for batch in [first_input, second_input, third_input] {
            let mut remaining = batch;
            loop {
                let mut one = [0_i16; 1];
                let report = partitioned.process(remaining, &mut one);
                if report.produced_frames != 0 {
                    partitioned_output.push(one[0]);
                }
                let (_, unconsumed) = remaining.split_at(report.consumed_frames);
                remaining = unconsumed;
                if remaining.is_empty() && !partitioned.has_pending_output() {
                    break;
                }
            }
        }
        assert_eq!(partitioned_output.as_slice(), whole_output);
        assert_eq!(whole_output, [0, 666, 1_333, 2_000, 2_666, 3_333, 4_000]);
    }

    #[test]
    fn reset_prevents_interpolation_across_a_dropped_gap() {
        let (Some(source), Some(output)) = (rate(2), rate(4)) else {
            return;
        };
        let mut resampler = LinearResampler::new(source, output);
        let mut before = [0_i16; 2];
        let _ = resampler.process(&[0, 1_000], &mut before);
        assert!(resampler.has_pending_output());

        resampler.reset();
        let mut after = [0_i16; 1];
        assert_eq!(resampler.process(&[20_000], &mut after).produced_frames, 1);
        assert_eq!(after, [20_000]);
        assert!(!resampler.has_pending_output());
    }

    #[test]
    fn empty_destination_never_consumes_source() {
        let (Some(source), Some(output)) = (rate(32_768), rate(32_000)) else {
            return;
        };
        let mut resampler = LinearResampler::new(source, output);
        assert_eq!(
            resampler.process(&[1, 2], &mut []),
            ResampleReport::default()
        );
    }
}
