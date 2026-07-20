//! Bounded chiptune-style tone synthesis for short interface cues.

use std::error::Error;
use std::fmt;

use crate::{SampleRate, Volume};

const MAXIMUM_FREQUENCY: u32 = 24_000;
const MAXIMUM_NOTE_DURATION_MS: u32 = 2_000;
const MAXIMUM_TONE_DURATION_MS: u32 = 5_000;
const MAXIMUM_NOTES: usize = 16;
const MAXIMUM_AMPLITUDE: i32 = 5_000;
const MINIMUM_AUDIBLE_AMPLITUDE: i32 = 256;

/// One positive square-wave note in a short cue.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ToneNote {
    frequency_hz: u32,
    duration_ms: u32,
}

impl ToneNote {
    /// Validate a note bounded to 24 kHz and two seconds.
    #[must_use]
    pub const fn new(frequency_hz: u32, duration_ms: u32) -> Option<Self> {
        if frequency_hz == 0
            || frequency_hz > MAXIMUM_FREQUENCY
            || duration_ms == 0
            || duration_ms > MAXIMUM_NOTE_DURATION_MS
        {
            None
        } else {
            Some(Self {
                frequency_hz,
                duration_ms,
            })
        }
    }

    /// Note frequency in hertz.
    #[must_use]
    pub const fn frequency_hz(self) -> u32 {
        self.frequency_hz
    }

    /// Note duration in milliseconds.
    #[must_use]
    pub const fn duration_ms(self) -> u32 {
        self.duration_ms
    }
}

/// Fully rendered finite mono S16 little-endian cue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SquareTone {
    rate: SampleRate,
    samples: Box<[i16]>,
}

impl SquareTone {
    /// Render a bounded sequence with a five-millisecond edge envelope.
    ///
    /// Muted volume produces an empty tone so callers can skip opening the
    /// audio device. Each note restarts square-wave phase, matching the
    /// existing dashboard and timer cues.
    ///
    /// # Errors
    ///
    /// Returns [`ToneError`] when there are no notes, too many notes, the
    /// combined duration exceeds five seconds, or sample sizing overflows.
    pub fn render(notes: &[ToneNote], rate: SampleRate, volume: Volume) -> Result<Self, ToneError> {
        let sample_count = sample_count(notes, rate)?;
        if volume.muted() {
            return Ok(Self {
                rate,
                samples: Box::new([]),
            });
        }

        let mut samples = Vec::with_capacity(sample_count);
        let rate_value = usize::try_from(rate.get()).map_err(|_| ToneError::SampleCountOverflow)?;
        let amplitude =
            (MAXIMUM_AMPLITUDE * i32::from(volume.percent()) / 100).max(MINIMUM_AUDIBLE_AMPLITUDE);
        let ramp_samples = (rate_value / 200).max(1);

        for note in notes {
            let duration =
                usize::try_from(note.duration_ms).map_err(|_| ToneError::SampleCountOverflow)?;
            let note_samples = rate_value
                .checked_mul(duration)
                .ok_or(ToneError::SampleCountOverflow)?
                / 1_000;
            let note_samples = note_samples.max(1);
            let frequency =
                usize::try_from(note.frequency_hz).map_err(|_| ToneError::SampleCountOverflow)?;
            let period = (rate_value / frequency).max(2);

            for index in 0..note_samples {
                let polarity = if index % period < period / 2 { 1 } else { -1 };
                let remaining = note_samples - index;
                let envelope = ramp_samples.min((index + 1).min(remaining));
                let scaled = i64::from(polarity * amplitude)
                    * i64::try_from(envelope).map_err(|_| ToneError::SampleCountOverflow)?
                    / i64::try_from(ramp_samples).map_err(|_| ToneError::SampleCountOverflow)?;
                samples.push(i16::try_from(scaled).map_err(|_| ToneError::SampleCountOverflow)?);
            }
        }

        debug_assert_eq!(samples.len(), sample_count);
        Ok(Self {
            rate,
            samples: samples.into_boxed_slice(),
        })
    }

    /// Rendered sample rate.
    #[must_use]
    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    /// Borrow the complete mono PCM sequence.
    #[must_use]
    pub const fn samples(&self) -> &[i16] {
        &self.samples
    }

    /// Whether muted rendering intentionally produced no samples.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

fn sample_count(notes: &[ToneNote], rate: SampleRate) -> Result<usize, ToneError> {
    if notes.is_empty() {
        return Err(ToneError::Empty);
    }
    if notes.len() > MAXIMUM_NOTES {
        return Err(ToneError::TooManyNotes);
    }
    let mut duration_ms = 0_u32;
    let mut count = 0_u64;
    for note in notes {
        duration_ms = duration_ms
            .checked_add(note.duration_ms)
            .ok_or(ToneError::DurationOverflow)?;
        let note_samples = u64::from(rate.get())
            .checked_mul(u64::from(note.duration_ms))
            .ok_or(ToneError::SampleCountOverflow)?
            / 1_000;
        count = count
            .checked_add(note_samples.max(1))
            .ok_or(ToneError::SampleCountOverflow)?;
    }
    if duration_ms > MAXIMUM_TONE_DURATION_MS {
        return Err(ToneError::TooLong);
    }
    usize::try_from(count).map_err(|_| ToneError::SampleCountOverflow)
}

/// Rejected finite-tone description.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToneError {
    /// At least one note is required.
    Empty,
    /// More than 16 notes were supplied.
    TooManyNotes,
    /// Adding note durations overflowed.
    DurationOverflow,
    /// The combined cue exceeds five seconds.
    TooLong,
    /// The target sample allocation cannot be represented safely.
    SampleCountOverflow,
}

impl fmt::Display for ToneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a tone requires at least one note"),
            Self::TooManyNotes => formatter.write_str("a tone cannot contain more than 16 notes"),
            Self::DurationOverflow => formatter.write_str("tone duration overflowed"),
            Self::TooLong => formatter.write_str("a tone cannot exceed five seconds"),
            Self::SampleCountOverflow => formatter.write_str("tone sample count overflowed"),
        }
    }
}

impl Error for ToneError {}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = match SampleRate::new(44_100) {
        Some(rate) => rate,
        None => panic!("test sample rate must be valid"),
    };
    const FULL: Volume = match Volume::new(100) {
        Some(volume) => volume,
        None => panic!("test volume must be valid"),
    };

    fn note(frequency: u32, duration: u32) -> ToneNote {
        ToneNote::new(frequency, duration).unwrap_or(ToneNote {
            frequency_hz: 1,
            duration_ms: 1,
        })
    }

    #[test]
    fn note_values_are_strictly_bounded() {
        assert_eq!(ToneNote::new(0, 20), None);
        assert_eq!(ToneNote::new(440, 0), None);
        assert_eq!(ToneNote::new(24_001, 20), None);
        assert_eq!(ToneNote::new(440, 2_001), None);
    }

    #[test]
    fn start_cue_matches_the_legacy_length_phase_and_envelope() {
        let tone = SquareTone::render(&[note(523, 28), note(784, 38)], RATE, FULL);
        assert!(tone.is_ok());
        let tone = tone.unwrap_or(SquareTone {
            rate: RATE,
            samples: Box::new([]),
        });
        assert_eq!(tone.rate(), RATE);
        assert_eq!(tone.samples().len(), 2_909);
        assert_eq!(tone.samples().first(), Some(&22));
        assert_eq!(tone.samples().get(41), Some(&954));
        assert_eq!(tone.samples().get(42), Some(&-977));
        assert_eq!(tone.samples().get(1_234), Some(&22));
        assert_eq!(tone.samples().last(), Some(&-22));
    }

    #[test]
    fn volume_scales_without_clipping_and_muting_allocates_no_pcm() {
        let Some(quiet_volume) = Volume::new(1) else {
            return;
        };
        let quiet = SquareTone::render(&[note(440, 20)], RATE, quiet_volume);
        assert!(quiet.is_ok());
        let quiet = quiet.unwrap_or(SquareTone {
            rate: RATE,
            samples: Box::new([]),
        });
        assert_eq!(quiet.samples().iter().copied().max(), Some(256));
        assert_eq!(quiet.samples().iter().copied().min(), Some(-256));

        let muted = SquareTone::render(&[note(440, 20)], RATE, Volume::MUTED);
        assert!(muted.is_ok_and(|tone| tone.is_empty()));
    }

    #[test]
    fn descriptions_are_bounded_before_allocation() {
        assert_eq!(SquareTone::render(&[], RATE, FULL), Err(ToneError::Empty));
        let many = [note(440, 1); MAXIMUM_NOTES + 1];
        assert_eq!(
            SquareTone::render(&many, RATE, FULL),
            Err(ToneError::TooManyNotes)
        );
        let long = [note(440, 1_001); 5];
        assert_eq!(
            SquareTone::render(&long, RATE, FULL),
            Err(ToneError::TooLong)
        );
    }
}
