//! Frame-paced square-wave PCM for CHIP-8 sound timers.

use retro_deck_audio::{SampleRate, Volume};

use super::{ApplicationPcm, ApplicationPcmError, ApplicationPcmStartError, AudioGate};

const STREAM_FRAMES_PER_SECOND: u32 = 60;
const BASE_AMPLITUDE: i16 = 6_000;

/// Validated square-wave stream description.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SquareStream {
    sample_rate: SampleRate,
    frequency_hz: u32,
}

impl SquareStream {
    /// Construct a stream whose tone does not exceed the Nyquist frequency.
    #[must_use]
    pub const fn new(sample_rate: SampleRate, frequency_hz: u32) -> Option<Self> {
        if frequency_hz == 0 || frequency_hz > sample_rate.get() / 2 {
            None
        } else {
            Some(Self {
                sample_rate,
                frequency_hz,
            })
        }
    }

    /// PCM samples generated per second.
    #[must_use]
    pub const fn sample_rate(self) -> SampleRate {
        self.sample_rate
    }

    /// Square-wave cycles generated per second.
    #[must_use]
    pub const fn frequency_hz(self) -> u32 {
        self.frequency_hz
    }
}

/// Same-thread CHIP-8 square-wave producer backed by BMC application PCM.
#[derive(Debug)]
pub struct SquarePcm {
    audio: ApplicationPcm,
    stream: SquareStream,
    samples: Box<[i16]>,
    phase: u32,
    source_active: bool,
}

impl SquarePcm {
    /// Claim inherited BMC audio and allocate one fixed 60 Hz sample block.
    ///
    /// # Errors
    ///
    /// Returns an error when BMC did not supply a valid application-audio
    /// channel.
    pub fn from_inherited(
        stream: SquareStream,
        initial_volume: Volume,
    ) -> Result<Self, ApplicationPcmStartError> {
        let audio = ApplicationPcm::from_inherited(stream.sample_rate(), initial_volume)?;
        Ok(Self::new(audio, stream))
    }

    fn new(audio: ApplicationPcm, stream: SquareStream) -> Self {
        let sample_count = usize::try_from(
            stream
                .sample_rate()
                .get()
                .div_ceil(STREAM_FRAMES_PER_SECOND),
        )
        .unwrap_or(1)
        .max(1);
        Self {
            audio,
            stream,
            samples: vec![0_i16; sample_count].into_boxed_slice(),
            phase: 0,
            source_active: false,
        }
    }

    /// Emit one emulated frame of tone, or release BMC on the first silence.
    pub fn render_frame(&mut self, active: bool) {
        if !active {
            if self.source_active {
                self.audio.release();
            }
            self.source_active = false;
            return;
        }
        self.source_active = true;
        fill_square_samples(
            &mut self.samples,
            &mut self.phase,
            self.stream.sample_rate(),
            self.stream.frequency_hz(),
        );
        self.audio.submit_mono(&self.samples);
    }

    /// Change whether the application is eligible for audible playback.
    pub fn set_gate(&self, gate: AudioGate) {
        self.audio.set_gate(gate);
    }

    /// Take the first retained BMC transport failure.
    pub fn take_error(&self) -> Option<ApplicationPcmError> {
        self.audio.take_error()
    }

    /// Release BMC's queued samples promptly.
    pub fn release(&self) {
        self.audio.release();
    }
}

fn fill_square_samples(samples: &mut [i16], phase: &mut u32, rate: SampleRate, frequency_hz: u32) {
    let period = (rate.get() / frequency_hz).max(2);
    for sample in samples {
        *sample = if *phase < period / 2 {
            BASE_AMPLITUDE
        } else {
            -BASE_AMPLITUDE
        };
        *phase += 1;
        if *phase == period {
            *phase = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> SampleRate {
        SampleRate::new(44_100).expect("test rate")
    }

    #[test]
    fn stream_description_enforces_frequency_bounds() {
        assert_eq!(SquareStream::new(rate(), 0), None);
        assert_eq!(SquareStream::new(rate(), 22_051), None);
        assert_eq!(
            SquareStream::new(rate(), 440).map(SquareStream::frequency_hz),
            Some(440)
        );
    }

    #[test]
    fn square_blocks_keep_phase_at_the_legacy_amplitude() {
        let mut samples = [0_i16; 120];
        let mut phase = 0;
        fill_square_samples(&mut samples, &mut phase, rate(), 440);
        assert_eq!(samples.first(), Some(&6_000));
        assert_eq!(samples.get(49), Some(&6_000));
        assert_eq!(samples.get(50), Some(&-6_000));
        assert_eq!(samples.get(99), Some(&-6_000));
        assert_eq!(samples.get(100), Some(&6_000));
        assert_eq!(phase, 20);
    }

    #[test]
    fn inactive_source_releases_and_hidden_audio_is_suppressed() {
        let volume = Volume::new(80).expect("test volume");
        let audio = ApplicationPcm::silent(rate(), volume);
        let stream = SquareStream::new(rate(), 440).expect("test stream");
        let mut square = SquarePcm::new(audio, stream);

        square.render_frame(true);
        assert_eq!(
            square.audio.stats().disconnected_dropped_samples,
            u64::from(44_100_u32.div_ceil(60))
        );
        square.set_gate(AudioGate::Hidden);
        square.render_frame(true);
        assert_eq!(
            square.audio.stats().inactive_dropped_samples,
            u64::from(44_100_u32.div_ceil(60))
        );
        square.render_frame(false);
        assert!(!square.source_active);
    }
}
