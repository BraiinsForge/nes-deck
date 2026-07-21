//! Narrow Open Sound System PCM device adapter.
//!
//! Opening, writing, draining, and resetting an OSS device may block. Callers
//! keep this type on a dedicated audio worker, never on input or rendering
//! paths. The worker decides when to open and drop it using the lifecycle in
//! `retro-deck-audio`.

mod application_pcm;
mod cue_worker;
mod square_pcm;

pub use application_pcm::{
    ApplicationPcm, ApplicationPcmError, ApplicationPcmStartError, ApplicationPcmStats,
};
pub use cue_worker::{ToneCueEnqueue, ToneCueWorker, ToneWorkerError, ToneWorkerReport};
pub use square_pcm::{SquarePcm, SquareStream};

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use retro_deck_audio::{ReleaseReason, SampleRate};
use rustix::fs::{Mode, OFlags, fcntl_getfl, fcntl_setfl, open};
use rustix::ioctl::{Getter, NoArg, Opcode, Setter, Updater, ioctl, opcode};

/// Standard BMC audio output device.
pub const DEFAULT_OSS_DEVICE: &str = "/dev/dsp";

const OSS_GROUP: u8 = b'P';
const DSP_RESET: Opcode = opcode::none(OSS_GROUP, 0);
const DSP_SYNC: Opcode = opcode::none(OSS_GROUP, 1);
const DSP_SPEED: Opcode = opcode::read_write::<i32>(OSS_GROUP, 2);
const DSP_SET_FORMAT: Opcode = opcode::read_write::<i32>(OSS_GROUP, 5);
const DSP_CHANNELS: Opcode = opcode::read_write::<i32>(OSS_GROUP, 6);
const DSP_SET_FRAGMENT: Opcode = opcode::read_write::<i32>(OSS_GROUP, 10);
const DSP_GET_OUTPUT_SPACE: Opcode = opcode::read::<AudioBufferInfo>(OSS_GROUP, 12);
const DSP_SET_TRIGGER: Opcode = opcode::write::<i32>(OSS_GROUP, 16);
const FORMAT_S16_LE: i32 = 0x10;
const MONO_CHANNELS: i32 = 1;
const ENABLE_OUTPUT: i32 = 0x2;
const ENCODED_CHUNK_BYTES: usize = 4_096;
const SAMPLES_PER_CHUNK: usize = ENCODED_CHUNK_BYTES / size_of::<i16>();
const MAXIMUM_PRIME_BYTES: usize = 1_048_576;
pub(crate) const GATE_ACTIVE: u8 = 0;
pub(crate) const GATE_MUTED: u8 = 1;
pub(crate) const GATE_PAUSED: u8 = 2;
pub(crate) const GATE_HIDDEN: u8 = 3;
pub(crate) const GATE_SHUTDOWN: u8 = 4;

/// Reason audio is allowed or suppressed for one visible runtime.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioGate {
    /// The application is visible, unpaused, and audible.
    Active,
    /// The user muted sound.
    Muted,
    /// Application playback is paused.
    Paused,
    /// The application or widget is not visible.
    Hidden,
}

impl AudioGate {
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Active => GATE_ACTIVE,
            Self::Muted => GATE_MUTED,
            Self::Paused => GATE_PAUSED,
            Self::Hidden => GATE_HIDDEN,
        }
    }
}

pub(crate) fn load_gate(gate: &AtomicU8) -> u8 {
    gate.load(Ordering::Acquire).min(GATE_SHUTDOWN)
}

pub(crate) const fn gate_release_reason(gate: u8) -> ReleaseReason {
    match gate {
        GATE_ACTIVE | GATE_MUTED => ReleaseReason::Muted,
        GATE_PAUSED => ReleaseReason::Paused,
        GATE_HIDDEN => ReleaseReason::Hidden,
        _ => ReleaseReason::Shutdown,
    }
}

/// OSS fragment sizing appropriate to finite cues or continuous streams.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OssProfile {
    /// Four 512-byte fragments minimize finite interface-cue latency.
    Cue,
    /// Eight 1024-byte fragments tolerate emulator rendering jitter.
    Stream,
}

/// Result of a cancellable PCM write.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PcmWriteOutcome {
    /// Every supplied sample reached the device.
    Complete,
    /// The caller requested cancellation before the next fixed-size chunk.
    Cancelled,
}

impl OssProfile {
    const fn fragment_word(self) -> i32 {
        match self {
            Self::Cue => (4 << 16) | 9,
            Self::Stream => (8 << 16) | 0x0a,
        }
    }
}

/// One configured mono S16 little-endian OSS playback handle.
#[derive(Debug)]
pub struct OssPcm {
    file: File,
    path: PathBuf,
    rate: SampleRate,
    output_held: bool,
}

impl OssPcm {
    /// Acquire and configure `/dev/dsp` without waiting for another owner.
    ///
    /// The driver may negotiate a nearby positive sample rate, available
    /// through [`Self::sample_rate`]. Format and channel count must match
    /// exactly.
    ///
    /// # Errors
    ///
    /// Returns [`OssError`] when the device is busy or unavailable, flags
    /// cannot be restored after acquisition, or format negotiation fails.
    pub fn open_mono(rate: SampleRate, profile: OssProfile) -> Result<Self, OssError> {
        Self::open_mono_at(DEFAULT_OSS_DEVICE, rate, profile)
    }

    /// Acquire and configure an OSS device at an explicit platform path.
    ///
    /// This is useful on systems whose OSS compatibility device is not named
    /// `/dev/dsp`. It has the same nonblocking acquisition contract as
    /// [`Self::open_mono`].
    ///
    /// # Errors
    ///
    /// Returns [`OssError`] for open, flag, or negotiation failure.
    pub fn open_mono_at(
        path: impl AsRef<Path>,
        rate: SampleRate,
        profile: OssProfile,
    ) -> Result<Self, OssError> {
        let path = path.as_ref().to_path_buf();
        let descriptor = open(
            &path,
            OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|source| OssError::Open {
            path: path.clone(),
            source: source.into(),
        })?;
        let mut flags = fcntl_getfl(&descriptor).map_err(|source| OssError::Configure {
            operation: "read device flags",
            source: source.into(),
        })?;
        flags.remove(OFlags::NONBLOCK);
        fcntl_setfl(&descriptor, flags).map_err(|source| OssError::Configure {
            operation: "restore blocking writes",
            source: source.into(),
        })?;
        let file = File::from(descriptor);

        let mut fragments = profile.fragment_word();
        let _ = update_i32::<DSP_SET_FRAGMENT>(&file, &mut fragments);

        let mut format = FORMAT_S16_LE;
        update_i32::<DSP_SET_FORMAT>(&file, &mut format).map_err(|source| OssError::Configure {
            operation: "set S16 little-endian format",
            source,
        })?;
        if format != FORMAT_S16_LE {
            return Err(OssError::RejectedFormat(format));
        }

        let mut channels = MONO_CHANNELS;
        update_i32::<DSP_CHANNELS>(&file, &mut channels).map_err(|source| OssError::Configure {
            operation: "set mono channels",
            source,
        })?;
        if channels != MONO_CHANNELS {
            return Err(OssError::RejectedChannels(channels));
        }

        let mut negotiated_rate =
            i32::try_from(rate.get()).map_err(|_| OssError::RejectedRate(0))?;
        update_i32::<DSP_SPEED>(&file, &mut negotiated_rate).map_err(|source| {
            OssError::Configure {
                operation: "set sample rate",
                source,
            }
        })?;
        let negotiated_rate = u32::try_from(negotiated_rate)
            .ok()
            .and_then(SampleRate::new)
            .ok_or(OssError::RejectedRate(negotiated_rate))?;

        Ok(Self {
            file,
            path,
            rate: negotiated_rate,
            output_held: false,
        })
    }

    /// Negotiated samples per second.
    #[must_use]
    pub const fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    /// Configured device path for diagnostics.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf-to-Path dereference is not const on the supported Rust toolchain"
    )]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write a complete sequence of native mono samples.
    ///
    /// Conversion uses a fixed stack buffer, so the playback path performs no
    /// allocation and is correct on both little- and big-endian Rust hosts.
    ///
    /// # Errors
    ///
    /// Returns [`OssError::Write`] if the driver cannot accept the complete
    /// sequence.
    pub fn write_mono(&mut self, samples: &[i16]) -> Result<(), OssError> {
        let _ = self.write_mono_while(samples, || true)?;
        Ok(())
    }

    /// Write mono samples while a cheap cancellation predicate remains true.
    ///
    /// The predicate runs before every chunk of at most 2048 samples. It must
    /// not block. This lets an audio worker observe atomic mute, hide, and
    /// shutdown state without putting device operations on an input path.
    ///
    /// # Errors
    ///
    /// Returns [`OssError::Write`] if the driver cannot accept a chunk.
    pub fn write_mono_while(
        &mut self,
        samples: &[i16],
        mut continue_playback: impl FnMut() -> bool,
    ) -> Result<PcmWriteOutcome, OssError> {
        let mut encoded = [0_u8; ENCODED_CHUNK_BYTES];
        for chunk in samples.chunks(SAMPLES_PER_CHUNK) {
            if !continue_playback() {
                return Ok(PcmWriteOutcome::Cancelled);
            }
            for (output, sample) in encoded.chunks_exact_mut(size_of::<i16>()).zip(chunk) {
                output.copy_from_slice(&sample.to_le_bytes());
            }
            let byte_count = chunk
                .len()
                .checked_mul(size_of::<i16>())
                .ok_or_else(encoding_bounds_error)?;
            let bytes = encoded
                .get(..byte_count)
                .ok_or_else(encoding_bounds_error)?;
            self.file.write_all(bytes).map_err(OssError::Write)?;
        }
        Ok(PcmWriteOutcome::Complete)
    }

    /// Prime the negotiated stream ring with silence while output is held.
    ///
    /// Holding and querying the ring are optional OSS capabilities. If either
    /// ioctl is unavailable, playback remains usable and this returns zero.
    /// A discovered ring is bounded to one MiB before any write. This mirrors
    /// the live-proven startup sequence that prevents a first-callback XRUN
    /// on GB and GBC cores.
    ///
    /// # Errors
    ///
    /// Returns [`OssError::Write`] if a validated ring cannot be filled.
    pub fn prime_stream(&mut self) -> Result<usize, OssError> {
        self.output_held = set_i32::<DSP_SET_TRIGGER>(&self.file, 0).is_ok();
        let samples = get_value::<DSP_GET_OUTPUT_SPACE, AudioBufferInfo>(&self.file)
            .ok()
            .and_then(prime_sample_count)
            .unwrap_or(0);
        let silence = [0_i16; SAMPLES_PER_CHUNK];
        let mut remaining = samples;
        while remaining != 0 {
            let count = remaining.min(silence.len());
            let Some(chunk) = silence.get(..count) else {
                break;
            };
            self.write_mono(chunk)?;
            remaining -= count;
        }
        Ok(samples - remaining)
    }

    /// Start output after [`Self::prime_stream`] successfully held it.
    ///
    /// Calling this without a held trigger is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`OssError::Configure`] if OSS accepted the hold but rejects
    /// the matching start request.
    pub fn start_output(&mut self) -> Result<(), OssError> {
        if !self.output_held {
            return Ok(());
        }
        set_i32::<DSP_SET_TRIGGER>(&self.file, ENABLE_OUTPUT).map_err(|source| {
            OssError::Configure {
                operation: "start PCM output",
                source,
            }
        })?;
        self.output_held = false;
        Ok(())
    }

    /// Wait until every queued sample has played.
    ///
    /// # Errors
    ///
    /// Returns [`OssError::Drain`] when the OSS synchronization request fails.
    pub fn drain(&self) -> Result<(), OssError> {
        no_arg::<DSP_SYNC>(&self.file).map_err(OssError::Drain)
    }

    /// Discard queued samples before dropping the device after mute, hide, or
    /// shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`OssError::Reset`] when the OSS reset request fails. Dropping
    /// the handle still releases its file descriptor after an error.
    pub fn reset(&self) -> Result<(), OssError> {
        no_arg::<DSP_RESET>(&self.file).map_err(OssError::Reset)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AudioBufferInfo {
    fragments: i32,
    fragment_total: i32,
    fragment_size: i32,
    bytes: i32,
}

fn prime_sample_count(info: AudioBufferInfo) -> Option<usize> {
    let bytes = usize::try_from(info.bytes).ok()?;
    if bytes == 0 || bytes > MAXIMUM_PRIME_BYTES || bytes % size_of::<i16>() != 0 {
        return None;
    }
    Some(bytes / size_of::<i16>())
}

fn update_i32<const REQUEST: Opcode>(file: &File, value: &mut i32) -> io::Result<()> {
    loop {
        // SAFETY: each caller binds REQUEST to an OSS ioctl declared with an
        // int pointer in linux/soundcard.h. `value` remains valid and
        // exclusively borrowed for the complete synchronous kernel call.
        let operation = unsafe { Updater::<REQUEST, i32>::new(value) };
        // SAFETY: the file is an owned open descriptor and the operation above
        // exactly describes this request's in/out integer operand.
        match unsafe { ioctl(file, operation) } {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Err(source) => return Err(source.into()),
        }
    }
}

fn no_arg<const REQUEST: Opcode>(file: &File) -> io::Result<()> {
    loop {
        // SAFETY: REQUEST is one of the OSS no-argument reset or sync
        // operations.
        let operation = unsafe { NoArg::<REQUEST>::new() };
        // SAFETY: the file is an owned open descriptor and this request has no
        // userspace pointer operand.
        match unsafe { ioctl(file, operation) } {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Err(source) => return Err(source.into()),
        }
    }
}

fn get_value<const REQUEST: Opcode, T>(file: &File) -> io::Result<T> {
    loop {
        // SAFETY: callers bind REQUEST and T to one exact read-only OSS ioctl
        // declaration. The kernel initializes the complete returned value.
        let operation = unsafe { Getter::<REQUEST, T>::new() };
        // SAFETY: the file is an owned open descriptor and the operation above
        // provides storage with the exact type encoded in REQUEST.
        match unsafe { ioctl(file, operation) } {
            Ok(value) => return Ok(value),
            Err(rustix::io::Errno::INTR) => {}
            Err(source) => return Err(source.into()),
        }
    }
}

fn set_i32<const REQUEST: Opcode>(file: &File, value: i32) -> io::Result<()> {
    loop {
        // SAFETY: callers bind REQUEST to an OSS ioctl declared with a const
        // int pointer in linux/soundcard.h.
        let operation = unsafe { Setter::<REQUEST, i32>::new(value) };
        // SAFETY: the file is an owned open descriptor and the operation above
        // exactly describes this request's input integer.
        match unsafe { ioctl(file, operation) } {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Err(source) => return Err(source.into()),
        }
    }
}

fn encoding_bounds_error() -> OssError {
    OssError::Write(io::Error::new(
        io::ErrorKind::InvalidData,
        "PCM encoding exceeded its fixed chunk",
    ))
}

/// OSS acquisition, negotiation, or playback failure.
#[derive(Debug)]
pub enum OssError {
    /// The configured path could not be opened for playback.
    Open {
        /// Attempted device path.
        path: PathBuf,
        /// Operating-system error.
        source: io::Error,
    },
    /// A device flag or ioctl operation failed.
    Configure {
        /// Failed configuration stage.
        operation: &'static str,
        /// Operating-system error.
        source: io::Error,
    },
    /// The driver selected a PCM format other than S16 little-endian.
    RejectedFormat(i32),
    /// The driver selected a channel count other than one.
    RejectedChannels(i32),
    /// The driver selected a nonpositive or excessive sample rate.
    RejectedRate(i32),
    /// A PCM write failed.
    Write(io::Error),
    /// Waiting for queued samples failed.
    Drain(io::Error),
    /// Discarding queued samples failed.
    Reset(io::Error),
}

impl fmt::Display for OssError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(
                    formatter,
                    "cannot open OSS device {}: {source}",
                    path.display()
                )
            }
            Self::Configure { operation, source } => {
                write!(formatter, "cannot {operation} on OSS device: {source}")
            }
            Self::RejectedFormat(format) => {
                write!(
                    formatter,
                    "OSS device rejected S16 little-endian as {format:#x}"
                )
            }
            Self::RejectedChannels(channels) => {
                write!(formatter, "OSS device rejected mono as {channels} channels")
            }
            Self::RejectedRate(rate) => {
                write!(
                    formatter,
                    "OSS device negotiated invalid sample rate {rate}"
                )
            }
            Self::Write(source) => write!(formatter, "cannot write OSS audio: {source}"),
            Self::Drain(source) => write!(formatter, "cannot drain OSS audio: {source}"),
            Self::Reset(source) => write!(formatter, "cannot reset OSS audio: {source}"),
        }
    }
}

impl Error for OssError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open { source, .. }
            | Self::Configure { source, .. }
            | Self::Write(source)
            | Self::Drain(source)
            | Self::Reset(source) => Some(source),
            Self::RejectedFormat(_) | Self::RejectedChannels(_) | Self::RejectedRate(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;

    #[test]
    fn profiles_preserve_the_live_validated_fragment_rings() {
        assert_eq!(OssProfile::Cue.fragment_word(), 0x0004_0009);
        assert_eq!(OssProfile::Stream.fragment_word(), 0x0008_000a);
    }

    #[test]
    fn ordinary_files_fail_at_the_first_required_oss_operation() {
        let rate = SampleRate::new(44_100);
        assert!(rate.is_some());
        let result = rate.map(|rate| OssPcm::open_mono_at("/dev/null", rate, OssProfile::Cue));
        assert!(matches!(
            result,
            Some(Err(OssError::Configure {
                operation: "set S16 little-endian format",
                ..
            }))
        ));
    }

    #[test]
    fn encoded_chunk_geometry_is_exact() {
        assert_eq!(ENCODED_CHUNK_BYTES, 4_096);
        assert_eq!(SAMPLES_PER_CHUNK, 2_048);
        assert_eq!(SAMPLES_PER_CHUNK * size_of::<i16>(), ENCODED_CHUNK_BYTES);
    }

    #[test]
    fn cancellable_writes_stop_before_the_next_fixed_chunk() {
        let file = OpenOptions::new().write(true).open("/dev/null");
        assert!(file.is_ok());
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Ok(file) = file else {
            return;
        };
        let mut device = OssPcm {
            file,
            path: PathBuf::from("/dev/null"),
            rate,
            output_held: false,
        };
        let samples = vec![1_i16; SAMPLES_PER_CHUNK * 3];
        let mut checks = 0;
        let outcome = device.write_mono_while(&samples, || {
            checks += 1;
            checks < 2
        });

        assert!(matches!(outcome, Ok(PcmWriteOutcome::Cancelled)));
        assert_eq!(checks, 2);
    }

    #[test]
    fn stream_priming_accepts_only_bounded_whole_samples() {
        let valid = AudioBufferInfo {
            bytes: 8_192,
            ..AudioBufferInfo::default()
        };
        assert_eq!(prime_sample_count(valid), Some(4_096));
        assert_eq!(
            prime_sample_count(AudioBufferInfo { bytes: 0, ..valid }),
            None
        );
        assert_eq!(
            prime_sample_count(AudioBufferInfo { bytes: 3, ..valid }),
            None
        );
        assert_eq!(
            prime_sample_count(AudioBufferInfo {
                bytes: 1_048_578,
                ..valid
            }),
            None
        );
        assert_eq!(
            prime_sample_count(AudioBufferInfo { bytes: -1, ..valid }),
            None
        );
    }
}
