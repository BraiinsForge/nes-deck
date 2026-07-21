//! Nonblocking client for BMC-owned foreground PCM playback.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::{fmt, io};

use deck_audio_v1::{ApplicationAudioClient, Volume as WireVolume};
use retro_deck_audio::{SampleRate, Volume};

use super::AudioGate;

/// Counters from one application-side PCM sender.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApplicationPcmStats {
    /// Samples accepted by BMC's bounded transport.
    pub sent_samples: u64,
    /// Samples discarded because the bounded transport was full.
    pub transport_dropped_samples: u64,
    /// Samples suppressed while muted, paused, or hidden.
    pub inactive_dropped_samples: u64,
    /// Samples lost because no usable BMC channel remained.
    pub disconnected_dropped_samples: u64,
}

/// Failure while claiming BMC's inherited application-audio channel.
#[derive(Debug)]
pub enum ApplicationPcmStartError {
    /// The process was not launched as a BMC-managed foreground application.
    MissingChannel,
    /// BMC advertised an invalid or unusable inherited descriptor.
    InvalidChannel(io::Error),
}

impl fmt::Display for ApplicationPcmStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingChannel => formatter.write_str("BMC application audio channel is absent"),
            Self::InvalidChannel(source) => {
                write!(
                    formatter,
                    "BMC application audio channel is invalid: {source}"
                )
            }
        }
    }
}

impl Error for ApplicationPcmStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MissingChannel => None,
            Self::InvalidChannel(source) => Some(source),
        }
    }
}

/// Nonfatal failure after an inherited application-audio channel was claimed.
#[derive(Debug)]
pub struct ApplicationPcmError(io::Error);

impl fmt::Display for ApplicationPcmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "BMC application audio transport failed: {}",
            self.0
        )
    }
}

impl Error for ApplicationPcmError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

/// Thin, same-thread PCM producer for one BMC-managed application.
///
/// Submission performs bounded stack encoding and a nonblocking datagram send.
/// It never opens an audio device, waits for playback, or creates a worker.
#[derive(Debug)]
pub struct ApplicationPcm {
    client: Option<ApplicationAudioClient>,
    source_rate: SampleRate,
    volume: Cell<Volume>,
    gate: Cell<AudioGate>,
    stats: Cell<ApplicationPcmStats>,
    error: RefCell<Option<ApplicationPcmError>>,
}

impl ApplicationPcm {
    /// Claim the channel inherited from BMC without opening an audio device.
    ///
    /// # Errors
    ///
    /// Returns an error when the process was not launched by BMC or the
    /// inherited descriptor is invalid.
    pub fn from_inherited(
        source_rate: SampleRate,
        initial_volume: Volume,
    ) -> Result<Self, ApplicationPcmStartError> {
        let client = ApplicationAudioClient::from_inherited_environment()
            .map_err(ApplicationPcmStartError::InvalidChannel)?
            .ok_or(ApplicationPcmStartError::MissingChannel)?;
        Ok(Self::with_client(client, source_rate, initial_volume))
    }

    /// Construct an intentionally silent sink for optional integration and
    /// deterministic host tests.
    #[must_use]
    pub fn silent(source_rate: SampleRate, initial_volume: Volume) -> Self {
        Self::new(None, source_rate, initial_volume)
    }

    fn with_client(
        client: ApplicationAudioClient,
        source_rate: SampleRate,
        initial_volume: Volume,
    ) -> Self {
        Self::new(Some(client), source_rate, initial_volume)
    }

    fn new(
        client: Option<ApplicationAudioClient>,
        source_rate: SampleRate,
        initial_volume: Volume,
    ) -> Self {
        Self {
            client,
            source_rate,
            volume: Cell::new(initial_volume),
            gate: Cell::new(if initial_volume.muted() {
                AudioGate::Muted
            } else {
                AudioGate::Active
            }),
            stats: Cell::new(ApplicationPcmStats::default()),
            error: RefCell::new(None),
        }
    }

    /// Downmix and submit one libretro-style stereo callback.
    pub fn submit_stereo(&self, frames: &[[i16; 2]]) {
        self.submit(frames.len(), |client, rate, volume| {
            client.try_send_stereo(rate, volume, frames)
        });
    }

    /// Submit one mono PCM block.
    pub fn submit_mono(&self, samples: &[i16]) {
        self.submit(samples.len(), |client, rate, volume| {
            client.try_send_mono(rate, volume, samples)
        });
    }

    /// Change whether application PCM is currently eligible for playback.
    pub fn set_gate(&self, gate: AudioGate) {
        let previous = self.gate.replace(gate);
        if previous == AudioGate::Active && gate != AudioGate::Active {
            self.release();
        }
    }

    /// Change the gain carried on subsequent PCM packets.
    pub fn set_volume(&self, volume: Volume) {
        self.volume.set(volume);
        if volume.muted() {
            self.release();
        }
    }

    /// Current producer-side counters.
    #[must_use]
    pub fn stats(&self) -> ApplicationPcmStats {
        self.stats.get()
    }

    /// Take the first transport error retained since the previous call.
    pub fn take_error(&self) -> Option<ApplicationPcmError> {
        self.error.borrow_mut().take()
    }

    /// Ask BMC to discard queued samples and release its device promptly.
    pub fn release(&self) {
        let Some(client) = &self.client else {
            return;
        };
        if let Err(source) = client.try_release() {
            self.record_error(source);
        }
    }

    fn submit(
        &self,
        sample_count: usize,
        send: impl FnOnce(
            &ApplicationAudioClient,
            u32,
            WireVolume,
        ) -> io::Result<deck_audio_v1::PcmSendReport>,
    ) {
        if sample_count == 0 {
            return;
        }
        if self.gate.get() != AudioGate::Active || self.volume.get().muted() {
            self.update_stats(|stats| {
                stats.inactive_dropped_samples = stats
                    .inactive_dropped_samples
                    .saturating_add(sample_count_u64(sample_count));
            });
            return;
        }
        let Some(client) = &self.client else {
            self.update_stats(|stats| {
                stats.disconnected_dropped_samples = stats
                    .disconnected_dropped_samples
                    .saturating_add(sample_count_u64(sample_count));
            });
            return;
        };
        let volume = WireVolume::new(self.volume.get().percent())
            .expect("validated Retro Deck volume must be valid on the BMC wire");
        match send(client, self.source_rate.get(), volume) {
            Ok(report) => self.update_stats(|stats| {
                stats.sent_samples = stats
                    .sent_samples
                    .saturating_add(sample_count_u64(report.sent));
                stats.transport_dropped_samples = stats
                    .transport_dropped_samples
                    .saturating_add(sample_count_u64(report.dropped));
            }),
            Err(source) => {
                self.update_stats(|stats| {
                    stats.disconnected_dropped_samples = stats
                        .disconnected_dropped_samples
                        .saturating_add(sample_count_u64(sample_count));
                });
                self.record_error(source);
            }
        }
    }

    fn update_stats(&self, update: impl FnOnce(&mut ApplicationPcmStats)) {
        let mut stats = self.stats.get();
        update(&mut stats);
        self.stats.set(stats);
    }

    fn record_error(&self, source: io::Error) {
        let mut retained = self.error.borrow_mut();
        if retained.is_none() {
            *retained = Some(ApplicationPcmError(source));
        }
    }
}

impl Drop for ApplicationPcm {
    fn drop(&mut self) {
        self.release();
    }
}

fn sample_count_u64(sample_count: usize) -> u64 {
    u64::try_from(sample_count).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use deck_audio_v1::{ApplicationAudioMessage, application_audio_channel};

    use super::*;

    fn rate() -> SampleRate {
        SampleRate::new(32_768).expect("test rate")
    }

    fn volume(percent: u8) -> Volume {
        Volume::new(percent).expect("test volume")
    }

    #[test]
    fn active_pcm_uses_the_inherited_bounded_transport() {
        let (receiver, client) = application_audio_channel().expect("audio channel");
        let audio = ApplicationPcm::with_client(client, rate(), volume(40));

        audio.submit_stereo(&[[100, 300], [-500, 100]]);
        let mut scratch = [0_i16; 4];
        assert_eq!(
            receiver.receive(&mut scratch).expect("PCM message"),
            ApplicationAudioMessage::Samples {
                sample_rate: 32_768,
                volume: WireVolume::new(40).expect("wire volume"),
                sample_count: 2,
            }
        );
        assert_eq!(&scratch[..2], &[200, -200]);
        assert_eq!(audio.stats().sent_samples, 2);

        audio.set_gate(AudioGate::Hidden);
        assert_eq!(
            receiver.receive(&mut scratch).expect("release message"),
            ApplicationAudioMessage::Release
        );
    }

    #[test]
    fn silent_and_inactive_sinks_drop_without_blocking() {
        let audio = ApplicationPcm::silent(rate(), volume(42));
        audio.submit_mono(&[1, 2, 3]);
        assert_eq!(audio.stats().disconnected_dropped_samples, 3);

        audio.set_gate(AudioGate::Paused);
        audio.submit_mono(&[4, 5]);
        assert_eq!(audio.stats().inactive_dropped_samples, 2);
    }
}
