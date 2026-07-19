//! Dedicated continuous square-wave worker with atomic source control.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use retro_deck_audio::{
    AudioAction, AudioLifecycle, AudioState, ReleaseReason, SampleRate, Volume,
};

use super::{
    AudioGate, GATE_ACTIVE, GATE_MUTED, GATE_SHUTDOWN, OssError, OssPcm, OssProfile,
    PcmWriteOutcome, gate_release_reason, load_gate,
};
use crate::time::{FrameClock, FrameRate};

const WAKE_QUEUE_CAPACITY: usize = 1;
const ERROR_QUEUE_CAPACITY: usize = 8;
const OPEN_RETRY_DELAY: Duration = Duration::from_secs(1);
const STREAM_FRAMES_PER_SECOND: u32 = 60;
const MAXIMUM_STREAM_SAMPLES: usize = 192_000 / 60;
const BASE_AMPLITUDE: i32 = 6_000;
const WORKER_NAME: &str = "retro-deck-audio-stream";

/// Validated square-wave stream requested from an OSS device.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SquareStream {
    requested_rate: SampleRate,
    frequency_hz: u32,
}

impl SquareStream {
    /// Construct a stream whose tone does not exceed the requested Nyquist
    /// frequency.
    #[must_use]
    pub const fn new(requested_rate: SampleRate, frequency_hz: u32) -> Option<Self> {
        if frequency_hz == 0 || frequency_hz > requested_rate.get() / 2 {
            None
        } else {
            Some(Self {
                requested_rate,
                frequency_hz,
            })
        }
    }

    /// Requested PCM sample rate.
    #[must_use]
    pub const fn requested_rate(self) -> SampleRate {
        self.requested_rate
    }

    /// Square-wave frequency in hertz.
    #[must_use]
    pub const fn frequency_hz(self) -> u32 {
        self.frequency_hz
    }
}

/// Nonfatal device error reported by a continuous-stream worker.
#[derive(Debug)]
pub enum StreamWorkerError {
    /// Opening or configuring the OSS device failed.
    Open(OssError),
    /// Writing a stream chunk failed.
    Playback(OssError),
    /// Discarding queued samples during release failed.
    Reset(OssError),
}

impl fmt::Display for StreamWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(source) => write!(formatter, "cannot start stream playback: {source}"),
            Self::Playback(source) => write!(formatter, "cannot play audio stream: {source}"),
            Self::Reset(source) => write!(formatter, "cannot release audio stream: {source}"),
        }
    }
}

impl Error for StreamWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open(source) | Self::Playback(source) | Self::Reset(source) => Some(source),
        }
    }
}

/// Final diagnostics returned by an explicitly stopped stream worker.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamWorkerReport {
    /// Number of successful lazy device acquisitions.
    pub opened: u64,
    /// Number of complete PCM chunks written.
    pub chunks: u64,
    /// Number of owned devices reset and released.
    pub released: u64,
    /// Number of open, write, or reset failures.
    pub errors: u64,
    /// Number of errors discarded because diagnostics were full.
    pub dropped_errors: u64,
    /// Whether the worker thread panicked before shutdown.
    pub panicked: bool,
}

/// Atomic input-side handle for one lazily owned continuous OSS stream.
///
/// Source, gate, and volume changes never perform audio I/O and never wait for
/// the worker. The OSS handle exists only while all three controls permit
/// playback and the source is actively producing sound.
#[derive(Debug)]
pub struct SquareStreamWorker {
    wake_sender: Option<SyncSender<()>>,
    errors: Receiver<StreamWorkerError>,
    source_active: Arc<AtomicBool>,
    gate: Arc<AtomicU8>,
    volume: Arc<AtomicU8>,
    thread: Option<JoinHandle<StreamWorkerReport>>,
}

impl SquareStreamWorker {
    /// Spawn a named worker without opening `/dev/dsp`.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error only when the worker thread cannot
    /// be created. Device errors are asynchronous and available through
    /// [`Self::take_errors`].
    pub fn spawn(stream: SquareStream, initial_volume: Volume) -> std::io::Result<Self> {
        let (wake_sender, wake_receiver) = mpsc::sync_channel(WAKE_QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let source_active = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(AtomicU8::new(if initial_volume.muted() {
            GATE_MUTED
        } else {
            GATE_ACTIVE
        }));
        let volume = Arc::new(AtomicU8::new(initial_volume.percent()));
        let worker_source = Arc::clone(&source_active);
        let worker_gate = Arc::clone(&gate);
        let worker_volume = Arc::clone(&volume);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || {
                run_worker(
                    &wake_receiver,
                    StreamControls {
                        errors: &error_sender,
                        source_active: worker_source.as_ref(),
                        gate: worker_gate.as_ref(),
                        volume: worker_volume.as_ref(),
                    },
                    stream,
                    |rate| OssPcm::open_mono(rate, OssProfile::Stream),
                )
            })?;
        Ok(Self {
            wake_sender: Some(wake_sender),
            errors,
            source_active,
            gate,
            volume,
            thread: Some(thread),
        })
    }

    /// Change whether the emulated source currently needs continuous sound.
    ///
    /// This is one atomic swap and a best-effort bounded wakeup. Repeating the
    /// current state performs no channel operation.
    pub fn set_source_active(&self, active: bool) {
        if self.source_active.swap(active, Ordering::AcqRel) != active {
            self.wake();
        }
    }

    /// Change playback eligibility and wake the worker to release promptly.
    pub fn set_gate(&self, gate: AudioGate) {
        self.gate.store(gate.code(), Ordering::Release);
        self.wake();
    }

    /// Change volume without waiting for playback or discarding hidden state.
    pub fn set_volume(&self, volume: Volume) {
        self.volume.store(volume.percent(), Ordering::Release);
        if volume.muted() {
            let _ = self.gate.compare_exchange(
                GATE_ACTIVE,
                GATE_MUTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        } else {
            let _ = self.gate.compare_exchange(
                GATE_MUTED,
                GATE_ACTIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        self.wake();
    }

    /// Drain currently reported worker errors without waiting.
    #[must_use]
    pub fn take_errors(&self) -> Vec<StreamWorkerError> {
        let mut errors = Vec::new();
        loop {
            match self.errors.try_recv() {
                Ok(error) => errors.push(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return errors,
            }
        }
    }

    /// Request immediate reset and release, then return worker diagnostics.
    #[must_use]
    pub fn shutdown(mut self) -> StreamWorkerReport {
        self.stop_and_join()
    }

    fn wake(&self) {
        if let Some(sender) = &self.wake_sender {
            match sender.try_send(()) {
                Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => {}
            }
        }
    }

    fn stop_and_join(&mut self) -> StreamWorkerReport {
        self.gate.store(GATE_SHUTDOWN, Ordering::Release);
        self.wake();
        let _ = self.wake_sender.take();
        let Some(thread) = self.thread.take() else {
            return StreamWorkerReport::default();
        };
        match thread.join() {
            Ok(report) => report,
            Err(_) => StreamWorkerReport {
                panicked: true,
                ..StreamWorkerReport::default()
            },
        }
    }
}

impl Drop for SquareStreamWorker {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

trait StreamDevice {
    fn sample_rate(&self) -> SampleRate;
    fn write_while(
        &mut self,
        samples: &[i16],
        continue_playback: &mut dyn FnMut() -> bool,
    ) -> Result<PcmWriteOutcome, OssError>;
    fn reset(&self) -> Result<(), OssError>;
}

impl StreamDevice for OssPcm {
    fn sample_rate(&self) -> SampleRate {
        self.sample_rate()
    }

    fn write_while(
        &mut self,
        samples: &[i16],
        continue_playback: &mut dyn FnMut() -> bool,
    ) -> Result<PcmWriteOutcome, OssError> {
        self.write_mono_while(samples, continue_playback)
    }

    fn reset(&self) -> Result<(), OssError> {
        self.reset()
    }
}

#[derive(Clone, Copy)]
struct StreamControls<'a> {
    errors: &'a SyncSender<StreamWorkerError>,
    source_active: &'a AtomicBool,
    gate: &'a AtomicU8,
    volume: &'a AtomicU8,
}

impl StreamControls<'_> {
    fn playback_active(self) -> bool {
        load_gate(self.gate) == GATE_ACTIVE
            && self.volume.load(Ordering::Acquire) != 0
            && self.source_active.load(Ordering::Acquire)
    }

    fn shutdown_requested(self) -> bool {
        load_gate(self.gate) == GATE_SHUTDOWN
    }

    fn release_reason(self) -> ReleaseReason {
        let gate = load_gate(self.gate);
        if gate != GATE_ACTIVE {
            gate_release_reason(gate)
        } else if self.volume.load(Ordering::Acquire) == 0 {
            ReleaseReason::Muted
        } else {
            ReleaseReason::Silent
        }
    }
}

struct StreamState<D> {
    lifecycle: AudioLifecycle,
    device: Option<D>,
    phase: u32,
    report: StreamWorkerReport,
}

impl<D: StreamDevice> StreamState<D> {
    fn new() -> Self {
        Self {
            lifecycle: AudioLifecycle::new(Duration::ZERO),
            device: None,
            phase: 0,
            report: StreamWorkerReport::default(),
        }
    }

    fn ensure_device(
        &mut self,
        stream: SquareStream,
        open_device: &mut impl FnMut(SampleRate) -> Result<D, OssError>,
        controls: StreamControls<'_>,
    ) -> bool {
        if !matches!(self.lifecycle.request_playback(), AudioAction::OpenDevice) {
            return self.device.is_some();
        }
        match open_device(stream.requested_rate()) {
            Ok(device) => {
                self.lifecycle.opened(true);
                self.device = Some(device);
                self.phase = 0;
                self.report.opened = self.report.opened.saturating_add(1);
                true
            }
            Err(error) => {
                self.lifecycle.opened(false);
                record_error(
                    StreamWorkerError::Open(error),
                    controls.errors,
                    &mut self.report,
                );
                false
            }
        }
    }

    fn release(&mut self, reason: ReleaseReason, controls: StreamControls<'_>) {
        let owns_device = self.device.is_some()
            && matches!(
                self.lifecycle.state(),
                AudioState::Priming | AudioState::Active | AudioState::Draining | AudioState::Idle
            );
        let _ = self.lifecycle.release(reason);
        if owns_device {
            self.reset_and_drop(controls);
        } else {
            let _ = self.device.take();
        }
    }

    fn reset_and_drop(&mut self, controls: StreamControls<'_>) {
        if let Some(device) = self.device.take() {
            self.report.released = self.report.released.saturating_add(1);
            if let Err(error) = device.reset() {
                record_error(
                    StreamWorkerError::Reset(error),
                    controls.errors,
                    &mut self.report,
                );
            }
        }
    }

    fn write_chunk(
        &mut self,
        stream: SquareStream,
        controls: StreamControls<'_>,
        buffer: &mut [i16; MAXIMUM_STREAM_SAMPLES],
    ) {
        let Some(rate) = self.device.as_ref().map(StreamDevice::sample_rate) else {
            let _ = self.lifecycle.failed();
            return;
        };
        let Some(volume) = Volume::new(controls.volume.load(Ordering::Acquire)) else {
            self.release(ReleaseReason::Muted, controls);
            return;
        };
        let sample_count = stream_sample_count(rate);
        let Some(samples) = buffer.get_mut(..sample_count) else {
            let _ = self.lifecycle.failed();
            self.reset_and_drop(controls);
            return;
        };
        fill_square_samples(
            samples,
            &mut self.phase,
            rate,
            stream.frequency_hz(),
            volume,
        );

        let result = self.device.as_mut().map(|device| {
            let mut continue_playback = || controls.playback_active();
            device.write_while(samples, &mut continue_playback)
        });
        match result {
            Some(Ok(PcmWriteOutcome::Complete)) => {
                self.report.chunks = self.report.chunks.saturating_add(1);
                if !controls.playback_active() {
                    self.release(controls.release_reason(), controls);
                }
            }
            Some(Ok(PcmWriteOutcome::Cancelled)) => {
                self.release(controls.release_reason(), controls);
            }
            Some(Err(error)) => {
                record_error(
                    StreamWorkerError::Playback(error),
                    controls.errors,
                    &mut self.report,
                );
                let _ = self.lifecycle.failed();
                self.reset_and_drop(controls);
            }
            None => {
                let _ = self.lifecycle.failed();
            }
        }
    }
}

fn run_worker<D: StreamDevice>(
    wake_receiver: &Receiver<()>,
    controls: StreamControls<'_>,
    stream: SquareStream,
    mut open_device: impl FnMut(SampleRate) -> Result<D, OssError>,
) -> StreamWorkerReport {
    let mut state = StreamState::new();
    let mut buffer = [0_i16; MAXIMUM_STREAM_SAMPLES];
    let Some(rate) = FrameRate::new(STREAM_FRAMES_PER_SECOND) else {
        return StreamWorkerReport {
            panicked: true,
            ..StreamWorkerReport::default()
        };
    };
    let mut clock = FrameClock::start(rate);

    loop {
        if controls.shutdown_requested() {
            state.release(ReleaseReason::Shutdown, controls);
            return state.report;
        }
        if !controls.playback_active() {
            state.release(controls.release_reason(), controls);
            if wake_receiver.recv().is_err() {
                state.release(ReleaseReason::Shutdown, controls);
                return state.report;
            }
            continue;
        }
        if !state.ensure_device(stream, &mut open_device, controls) {
            match wake_receiver.recv_timeout(OPEN_RETRY_DELAY) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    state.release(ReleaseReason::Shutdown, controls);
                    return state.report;
                }
            }
        }
        let wait = clock.wait_duration();
        if !wait.is_zero() {
            match wake_receiver.recv_timeout(wait) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    state.release(ReleaseReason::Shutdown, controls);
                    return state.report;
                }
            }
        }
        state.write_chunk(stream, controls, &mut buffer);
        clock.complete_frame();
        if state.device.is_none() && controls.playback_active() {
            match wake_receiver.recv_timeout(OPEN_RETRY_DELAY) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    state.release(ReleaseReason::Shutdown, controls);
                    return state.report;
                }
            }
        }
    }
}

fn stream_sample_count(rate: SampleRate) -> usize {
    usize::try_from(rate.get() / STREAM_FRAMES_PER_SECOND)
        .unwrap_or(MAXIMUM_STREAM_SAMPLES)
        .max(1)
}

fn fill_square_samples(
    samples: &mut [i16],
    phase: &mut u32,
    rate: SampleRate,
    frequency_hz: u32,
    volume: Volume,
) {
    let period = (rate.get() / frequency_hz).max(2);
    let amplitude = BASE_AMPLITUDE * i32::from(volume.percent()) / 100;
    let amplitude = i16::try_from(amplitude).unwrap_or(i16::MAX);
    for sample in samples {
        *sample = if *phase < period / 2 {
            amplitude
        } else {
            -amplitude
        };
        *phase += 1;
        if *phase == period {
            *phase = 0;
        }
    }
}

fn record_error(
    error: StreamWorkerError,
    sender: &SyncSender<StreamWorkerError>,
    report: &mut StreamWorkerReport,
) {
    report.errors = report.errors.saturating_add(1);
    match sender.try_send(error) {
        Ok(()) => {}
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
            report.dropped_errors = report.dropped_errors.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum DeviceEvent {
        Open,
        Write,
        Reset,
    }

    #[derive(Debug)]
    struct FakeDevice {
        rate: SampleRate,
        events: Arc<Mutex<Vec<DeviceEvent>>>,
        deactivate_after_write: Option<Arc<AtomicBool>>,
        fail_write: bool,
    }

    impl StreamDevice for FakeDevice {
        fn sample_rate(&self) -> SampleRate {
            self.rate
        }

        fn write_while(
            &mut self,
            _samples: &[i16],
            continue_playback: &mut dyn FnMut() -> bool,
        ) -> Result<PcmWriteOutcome, OssError> {
            if let Ok(mut events) = self.events.lock() {
                events.push(DeviceEvent::Write);
            }
            if self.fail_write {
                return Err(OssError::Write(io::Error::other("fixture write failure")));
            }
            let outcome = if continue_playback() {
                PcmWriteOutcome::Complete
            } else {
                PcmWriteOutcome::Cancelled
            };
            if let Some(source) = &self.deactivate_after_write {
                source.store(false, Ordering::Release);
            }
            Ok(outcome)
        }

        fn reset(&self) -> Result<(), OssError> {
            if let Ok(mut events) = self.events.lock() {
                events.push(DeviceEvent::Reset);
            }
            Ok(())
        }
    }

    #[test]
    fn stream_description_enforces_frequency_bounds() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        assert_eq!(SquareStream::new(rate, 0), None);
        assert_eq!(SquareStream::new(rate, 22_051), None);
        assert_eq!(
            SquareStream::new(rate, 440).map(SquareStream::frequency_hz),
            Some(440)
        );
    }

    #[test]
    fn square_chunks_match_the_legacy_amplitude_and_keep_phase() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(volume) = Volume::new(80) else {
            return;
        };
        let mut samples = [0_i16; 120];
        let mut phase = 0;
        fill_square_samples(&mut samples, &mut phase, rate, 440, volume);
        assert_eq!(samples.first(), Some(&4_800));
        assert_eq!(samples.get(49), Some(&4_800));
        assert_eq!(samples.get(50), Some(&-4_800));
        assert_eq!(samples.get(99), Some(&-4_800));
        assert_eq!(samples.get(100), Some(&4_800));
        assert_eq!(phase, 20);
    }

    #[test]
    fn inactive_worker_never_opens_a_device() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(volume) = Volume::new(80) else {
            return;
        };
        let Some(stream) = SquareStream::new(rate, 440) else {
            return;
        };
        let (wake_sender, wake_receiver) = mpsc::sync_channel(WAKE_QUEUE_CAPACITY);
        drop(wake_sender);
        let (error_sender, _errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let source_active = AtomicBool::new(false);
        let gate = AtomicU8::new(GATE_ACTIVE);
        let volume = AtomicU8::new(volume.percent());
        let mut open_calls = 0;
        let report = run_worker::<FakeDevice>(
            &wake_receiver,
            StreamControls {
                errors: &error_sender,
                source_active: &source_active,
                gate: &gate,
                volume: &volume,
            },
            stream,
            |_rate| {
                open_calls += 1;
                Err(OssError::Open {
                    path: PathBuf::from("/dev/dsp"),
                    source: io::Error::other("must not open"),
                })
            },
        );
        assert_eq!(open_calls, 0);
        assert_eq!(report, StreamWorkerReport::default());
    }

    #[test]
    fn active_source_opens_writes_and_releases_on_first_silence() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(volume) = Volume::new(80) else {
            return;
        };
        let Some(stream) = SquareStream::new(rate, 440) else {
            return;
        };
        let (wake_sender, wake_receiver) = mpsc::sync_channel(WAKE_QUEUE_CAPACITY);
        drop(wake_sender);
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let source_active = Arc::new(AtomicBool::new(true));
        let gate = AtomicU8::new(GATE_ACTIVE);
        let volume = AtomicU8::new(volume.percent());
        let events = Arc::new(Mutex::new(Vec::new()));
        let device_events = Arc::clone(&events);
        let device_source = Arc::clone(&source_active);

        let report = run_worker(
            &wake_receiver,
            StreamControls {
                errors: &error_sender,
                source_active: source_active.as_ref(),
                gate: &gate,
                volume: &volume,
            },
            stream,
            move |rate| {
                if let Ok(mut events) = device_events.lock() {
                    events.push(DeviceEvent::Open);
                }
                Ok(FakeDevice {
                    rate,
                    events: Arc::clone(&device_events),
                    deactivate_after_write: Some(Arc::clone(&device_source)),
                    fail_write: false,
                })
            },
        );

        assert_eq!(
            report,
            StreamWorkerReport {
                opened: 1,
                chunks: 1,
                released: 1,
                ..StreamWorkerReport::default()
            }
        );
        assert!(errors.try_recv().is_err());
        let Ok(events) = events.lock() else {
            return;
        };
        assert_eq!(
            events.as_slice(),
            &[DeviceEvent::Open, DeviceEvent::Write, DeviceEvent::Reset]
        );
    }

    #[test]
    fn muted_public_worker_starts_and_stops_without_opening_audio() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(stream) = SquareStream::new(rate, 440) else {
            return;
        };
        let worker = SquareStreamWorker::spawn(stream, Volume::MUTED);
        assert!(worker.is_ok());
        let Ok(worker) = worker else {
            return;
        };
        worker.set_source_active(true);
        worker.set_gate(AudioGate::Hidden);
        assert_eq!(worker.shutdown(), StreamWorkerReport::default());
    }

    #[test]
    fn volume_changes_do_not_discard_hidden_or_paused_state() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(volume) = Volume::new(80) else {
            return;
        };
        let Some(stream) = SquareStream::new(rate, 440) else {
            return;
        };
        let Ok(worker) = SquareStreamWorker::spawn(stream, volume) else {
            return;
        };

        worker.set_gate(AudioGate::Hidden);
        worker.set_volume(Volume::MUTED);
        worker.set_volume(volume);
        assert_eq!(load_gate(worker.gate.as_ref()), AudioGate::Hidden.code());

        worker.set_gate(AudioGate::Paused);
        worker.set_volume(Volume::MUTED);
        worker.set_volume(volume);
        assert_eq!(load_gate(worker.gate.as_ref()), AudioGate::Paused.code());

        assert_eq!(worker.shutdown(), StreamWorkerReport::default());
    }
}
