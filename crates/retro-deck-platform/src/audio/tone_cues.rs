//! Pre-rendered finite tones sent over BMC's bounded audio channel.

use std::error::Error;
use std::fmt;

use retro_deck_audio::{SampleRate, SquareTone, ToneError, ToneNote, Volume};

use super::{ApplicationPcm, ApplicationPcmError, ApplicationPcmStartError, AudioGate};

/// Failure while preparing finite cues or claiming BMC audio.
#[derive(Debug)]
pub enum ToneCueStartError {
    /// A cue description could not be rendered safely.
    InvalidTone(ToneError),
    /// BMC did not provide a usable application-audio channel.
    Audio(ApplicationPcmStartError),
}

impl fmt::Display for ToneCueStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTone(source) => write!(formatter, "invalid tone cue: {source}"),
            Self::Audio(source) => source.fmt(formatter),
        }
    }
}

impl Error for ToneCueStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidTone(source) => Some(source),
            Self::Audio(source) => Some(source),
        }
    }
}

/// Same-thread player for a small set of finite chiptune cues.
///
/// Waveforms are allocated and rendered once during startup. Playing a cue
/// performs only bounded, nonblocking sends to the compositor; it never opens
/// an audio device, creates a worker, or allocates on the input path.
#[derive(Debug)]
pub struct ToneCuePlayer<C> {
    audio: ApplicationPcm,
    tones: Box<[(C, SquareTone)]>,
}

impl<C: Copy + Eq> ToneCuePlayer<C> {
    /// Prepare every cue, then claim the audio channel inherited from BMC.
    ///
    /// # Errors
    ///
    /// Returns an error when a tone is invalid or BMC did not provide a valid
    /// application-audio channel.
    pub fn from_inherited<'a>(
        rate: SampleRate,
        initial_volume: Volume,
        cues: impl IntoIterator<Item = (C, &'a [ToneNote])>,
    ) -> Result<Self, ToneCueStartError> {
        let tones = render_tones(rate, cues)?;
        let audio = ApplicationPcm::from_inherited(rate, initial_volume)
            .map_err(ToneCueStartError::Audio)?;
        Ok(Self { audio, tones })
    }

    /// Submit a cue if its identifier was present during construction.
    ///
    /// Submission is bounded and nonblocking. BMC drains the finite waveform
    /// and releases its device after its central idle grace period.
    pub fn play(&self, cue: C) {
        let Some((_, tone)) = self.tones.iter().find(|(candidate, _)| *candidate == cue) else {
            return;
        };
        self.audio.submit_mono(tone.samples());
    }

    /// Change whether cues are currently eligible for playback.
    pub fn set_gate(&self, gate: AudioGate) {
        self.audio.set_gate(gate);
    }

    /// Change the gain carried on subsequent PCM packets.
    pub fn set_volume(&self, volume: Volume) {
        self.audio.set_volume(volume);
    }

    /// Take the first retained BMC transport failure.
    pub fn take_error(&self) -> Option<ApplicationPcmError> {
        self.audio.take_error()
    }

    /// Discard queued samples and let BMC release its device promptly.
    pub fn release(&self) {
        self.audio.release();
    }
}

fn render_tones<'a, C>(
    rate: SampleRate,
    cues: impl IntoIterator<Item = (C, &'a [ToneNote])>,
) -> Result<Box<[(C, SquareTone)]>, ToneCueStartError> {
    cues.into_iter()
        .map(|(cue, notes)| {
            SquareTone::render(notes, rate)
                .map(|tone| (cue, tone))
                .map_err(ToneCueStartError::InvalidTone)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::ApplicationPcmStats;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Cue {
        Short,
        Missing,
    }

    fn rate() -> SampleRate {
        SampleRate::new(44_100).expect("test sample rate")
    }

    fn volume() -> Volume {
        Volume::new(42).expect("test volume")
    }

    fn note() -> ToneNote {
        ToneNote::new(440, 20).expect("test note")
    }

    fn player() -> ToneCuePlayer<Cue> {
        let tones =
            render_tones(rate(), [(Cue::Short, [note()].as_slice())]).expect("render test tone");
        ToneCuePlayer {
            audio: ApplicationPcm::silent(rate(), volume()),
            tones,
        }
    }

    #[test]
    fn known_cues_submit_pre_rendered_samples_without_a_device() {
        let player = player();
        player.play(Cue::Short);
        assert_eq!(player.audio.stats().disconnected_dropped_samples, 882);
    }

    #[test]
    fn missing_and_hidden_cues_are_harmless() {
        let player = player();
        player.play(Cue::Missing);
        assert_eq!(player.audio.stats(), ApplicationPcmStats::default());

        player.set_gate(AudioGate::Hidden);
        player.play(Cue::Short);
        assert_eq!(player.audio.stats().inactive_dropped_samples, 882);
    }
}
