//! Dedicated finite-tone worker with bounded input-side interaction.

use std::error::Error;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use retro_deck_audio::{
    AudioAction, AudioLifecycle, AudioState, CueEnqueue, CueReceive, CueReceiver, CueSender,
    ReleaseReason, SampleRate, SquareTone, ToneError, ToneNote, Volume, cue_channel,
};

use super::{
    AudioGate, GATE_ACTIVE, GATE_MUTED, GATE_SHUTDOWN, OssError, OssPcm, OssProfile,
    PcmWriteOutcome, gate_release_reason, load_gate,
};

const CUE_QUEUE_CAPACITY: usize = 4;
const ERROR_QUEUE_CAPACITY: usize = 8;
const IDLE_GRACE: Duration = Duration::from_millis(250);
const WORKER_NAME: &str = "retro-deck-audio-cues";
/// Result of trying to submit a cue without waiting for the audio thread.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ToneCueEnqueue {
    /// The bounded worker queue accepted the cue.
    Queued,
    /// Playback is currently muted, paused, hidden, or shutting down.
    DroppedInactive,
    /// The queue was full, so stale sound feedback was discarded.
    DroppedFull,
    /// The worker has exited.
    DroppedDisconnected,
}

/// Nonfatal error reported by the finite-cue worker.
#[derive(Debug)]
pub enum ToneWorkerError {
    /// Opening or configuring the OSS device failed.
    Open(OssError),
    /// The selected note sequence failed bounded rendering.
    Render(ToneError),
    /// Writing or draining a cue failed.
    Playback(OssError),
    /// Discarding queued audio during a forced release failed.
    Reset(OssError),
}

impl fmt::Display for ToneWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(source) => write!(formatter, "cannot start cue playback: {source}"),
            Self::Render(source) => write!(formatter, "cannot render cue: {source}"),
            Self::Playback(source) => write!(formatter, "cannot play cue: {source}"),
            Self::Reset(source) => write!(formatter, "cannot cancel cue: {source}"),
        }
    }
}

impl Error for ToneWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open(source) | Self::Playback(source) | Self::Reset(source) => Some(source),
            Self::Render(source) => Some(source),
        }
    }
}

/// Final diagnostics returned by an explicitly stopped cue worker.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToneWorkerReport {
    /// Number of successful device acquisitions.
    pub opened: u64,
    /// Number of complete cues drained normally.
    pub played: u64,
    /// Number of device, rendering, write, drain, or reset failures.
    pub errors: u64,
    /// Number of errors discarded because the diagnostic queue was full.
    pub dropped_errors: u64,
    /// Whether the worker thread panicked before shutdown.
    pub panicked: bool,
}

#[derive(Clone, Copy, Debug)]
enum WorkerMessage<C> {
    Play(C),
    Wake,
}

#[derive(Clone, Copy, Debug)]
enum WorkerEvent<C> {
    Idle,
    Play(C),
    Wake,
}

/// Input-side handle for one lazily owned finite-cue OSS device.
///
/// `try_play`, `set_gate`, and `set_volume` never wait for audio I/O. Dropping
/// the handle wakes and joins its worker after requesting immediate reset.
#[derive(Debug)]
pub struct ToneCueWorker<C: Copy + Send + 'static> {
    sender: Option<CueSender<WorkerMessage<C>>>,
    errors: Receiver<ToneWorkerError>,
    gate: Arc<AtomicU8>,
    volume: Arc<AtomicU8>,
    released: Arc<AtomicBool>,
    thread: Option<JoinHandle<ToneWorkerReport>>,
}

impl<C: Copy + Send + 'static> ToneCueWorker<C> {
    /// Spawn a named worker using `/dev/dsp` and the finite-cue fragment ring.
    ///
    /// `notes` maps cheap application cue identifiers to static, validated at
    /// render time note slices. The device remains closed until the first cue
    /// and closes 250 ms after a completed cue unless another cue arrives.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error only when the worker thread cannot be
    /// created. Audio device failures are nonfatal and available through
    /// [`Self::take_errors`].
    pub fn spawn(
        requested_rate: SampleRate,
        initial_volume: Volume,
        notes: fn(C) -> &'static [ToneNote],
    ) -> std::io::Result<Self> {
        let (sender, receiver) = cue_channel(cue_queue_capacity());
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let gate = Arc::new(AtomicU8::new(if initial_volume.muted() {
            GATE_MUTED
        } else {
            GATE_ACTIVE
        }));
        let volume = Arc::new(AtomicU8::new(initial_volume.percent()));
        let released = Arc::new(AtomicBool::new(true));
        let worker_gate = Arc::clone(&gate);
        let worker_volume = Arc::clone(&volume);
        let worker_released = Arc::clone(&released);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || {
                run_worker(
                    &receiver,
                    WorkerControls {
                        errors: &error_sender,
                        gate: worker_gate.as_ref(),
                        volume: worker_volume.as_ref(),
                        released: worker_released.as_ref(),
                    },
                    requested_rate,
                    IDLE_GRACE,
                    notes,
                    |rate| OssPcm::open_mono(rate, OssProfile::Cue),
                )
            })?;
        Ok(Self {
            sender: Some(sender),
            errors,
            gate,
            volume,
            released,
            thread: Some(thread),
        })
    }

    /// Try to queue one cue without waiting.
    pub fn try_play(&self, cue: C) -> ToneCueEnqueue {
        if load_gate(&self.gate) != GATE_ACTIVE || self.volume.load(Ordering::Acquire) == 0 {
            return ToneCueEnqueue::DroppedInactive;
        }
        let Some(sender) = &self.sender else {
            return ToneCueEnqueue::DroppedDisconnected;
        };
        match sender.try_enqueue(WorkerMessage::Play(cue)) {
            CueEnqueue::Queued => ToneCueEnqueue::Queued,
            CueEnqueue::DroppedFull => ToneCueEnqueue::DroppedFull,
            CueEnqueue::DroppedDisconnected => ToneCueEnqueue::DroppedDisconnected,
        }
    }

    /// Change playback eligibility and wake the worker to release promptly.
    pub fn set_gate(&self, gate: AudioGate) {
        if gate != AudioGate::Active {
            self.released.store(false, Ordering::Release);
        }
        self.gate.store(gate.code(), Ordering::Release);
        self.wake();
    }

    /// Change volume without waiting for playback.
    ///
    /// Zero volume enters the muted gate. Raising volume leaves hidden and
    /// paused state intact, but reactivates a worker muted by this method.
    pub fn set_volume(&self, volume: Volume) {
        self.volume.store(volume.percent(), Ordering::Release);
        if volume.muted() {
            self.released.store(false, Ordering::Release);
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

    /// Whether the worker has observed its gate and closed the OSS device.
    ///
    /// This is a nonblocking acknowledgement intended for process handoff.
    #[must_use]
    pub fn device_released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    /// Drain currently reported worker errors without waiting.
    #[must_use]
    pub fn take_errors(&self) -> Vec<ToneWorkerError> {
        let mut errors = Vec::new();
        loop {
            match self.errors.try_recv() {
                Ok(error) => errors.push(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return errors,
            }
        }
    }

    /// Request immediate release, stop the worker, and return diagnostics.
    #[must_use]
    pub fn shutdown(mut self) -> ToneWorkerReport {
        self.stop_and_join()
    }

    fn wake(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.try_enqueue(WorkerMessage::Wake);
        }
    }

    fn stop_and_join(&mut self) -> ToneWorkerReport {
        self.gate.store(GATE_SHUTDOWN, Ordering::Release);
        self.wake();
        let _ = self.sender.take();
        let Some(thread) = self.thread.take() else {
            return ToneWorkerReport::default();
        };
        match thread.join() {
            Ok(report) => report,
            Err(_) => ToneWorkerReport {
                panicked: true,
                ..ToneWorkerReport::default()
            },
        }
    }
}

impl<C: Copy + Send + 'static> Drop for ToneCueWorker<C> {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

trait CueDevice {
    fn sample_rate(&self) -> SampleRate;
    fn write_while(
        &mut self,
        samples: &[i16],
        continue_playback: &mut dyn FnMut() -> bool,
    ) -> Result<PcmWriteOutcome, OssError>;
    fn drain(&self) -> Result<(), OssError>;
    fn reset(&self) -> Result<(), OssError>;
}

impl CueDevice for OssPcm {
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

    fn drain(&self) -> Result<(), OssError> {
        self.drain()
    }

    fn reset(&self) -> Result<(), OssError> {
        self.reset()
    }
}

#[derive(Clone, Copy)]
struct WorkerControls<'a> {
    errors: &'a SyncSender<ToneWorkerError>,
    gate: &'a AtomicU8,
    volume: &'a AtomicU8,
    released: &'a AtomicBool,
}

impl WorkerControls<'_> {
    fn playback_active(self) -> bool {
        load_gate(self.gate) == GATE_ACTIVE && self.volume.load(Ordering::Acquire) != 0
    }

    fn release_reason(self) -> ReleaseReason {
        if self.volume.load(Ordering::Acquire) == 0 {
            ReleaseReason::Muted
        } else {
            gate_release_reason(load_gate(self.gate))
        }
    }
}

struct WorkerState<D> {
    origin: Instant,
    idle_grace: Duration,
    lifecycle: AudioLifecycle,
    device: Option<D>,
    idle_deadline: Option<Instant>,
    report: ToneWorkerReport,
}

impl<D: CueDevice> WorkerState<D> {
    fn new(idle_grace: Duration) -> Self {
        Self {
            origin: Instant::now(),
            idle_grace,
            lifecycle: AudioLifecycle::new(idle_grace),
            device: None,
            idle_deadline: None,
            report: ToneWorkerReport::default(),
        }
    }

    fn wait_for<C>(
        &self,
        receiver: &CueReceiver<WorkerMessage<C>>,
    ) -> CueReceive<WorkerMessage<C>> {
        if let Some(deadline) = self.idle_deadline {
            receiver.wait_latest_timeout(deadline.saturating_duration_since(Instant::now()))
        } else {
            receiver.wait_latest()
        }
    }

    fn expire_idle(&mut self, controls: WorkerControls<'_>) {
        if matches!(
            self.lifecycle.tick(monotonic_milliseconds(self.origin)),
            AudioAction::CloseDevice
        ) {
            let _ = self.device.take();
            controls.released.store(true, Ordering::Release);
        }
        self.idle_deadline = None;
    }

    fn force_release(&mut self, reason: ReleaseReason, controls: WorkerControls<'_>) {
        let needs_reset = matches!(
            self.lifecycle.state(),
            AudioState::Priming | AudioState::Active | AudioState::Draining
        );
        let _ = self.lifecycle.release(reason);
        self.idle_deadline = None;
        if needs_reset {
            self.reset_and_drop(controls);
        } else {
            let _ = self.device.take();
            controls.released.store(true, Ordering::Release);
        }
    }

    fn reset_and_drop(&mut self, controls: WorkerControls<'_>) {
        if let Some(output) = self.device.take() {
            if let Err(error) = output.reset() {
                record_error(
                    ToneWorkerError::Reset(error),
                    controls.errors,
                    &mut self.report,
                );
            }
        }
        controls.released.store(true, Ordering::Release);
    }

    fn ensure_device(
        &mut self,
        requested_rate: SampleRate,
        open_device: &mut impl FnMut(SampleRate) -> Result<D, OssError>,
        controls: WorkerControls<'_>,
    ) -> bool {
        if !matches!(self.lifecycle.request_playback(), AudioAction::OpenDevice) {
            return self.device.is_some();
        }
        controls.released.store(false, Ordering::Release);
        match open_device(requested_rate) {
            Ok(opened) => {
                self.lifecycle.opened(true);
                self.device = Some(opened);
                self.report.opened = self.report.opened.saturating_add(1);
                true
            }
            Err(error) => {
                self.lifecycle.opened(false);
                controls.released.store(true, Ordering::Release);
                record_error(
                    ToneWorkerError::Open(error),
                    controls.errors,
                    &mut self.report,
                );
                false
            }
        }
    }

    fn play<C: Copy>(
        &mut self,
        cue: C,
        requested_rate: SampleRate,
        notes: fn(C) -> &'static [ToneNote],
        open_device: &mut impl FnMut(SampleRate) -> Result<D, OssError>,
        controls: WorkerControls<'_>,
    ) {
        self.idle_deadline = None;
        if !self.ensure_device(requested_rate, open_device, controls) {
            return;
        }
        let Some(current_volume) = Volume::new(controls.volume.load(Ordering::Acquire)) else {
            self.force_release(ReleaseReason::Muted, controls);
            return;
        };
        let Some(rate) = self.device.as_ref().map(CueDevice::sample_rate) else {
            let _ = self.lifecycle.failed();
            return;
        };
        let tone = match SquareTone::render(notes(cue), rate, current_volume) {
            Ok(tone) if !tone.is_empty() => tone,
            Ok(_) => {
                self.force_release(ReleaseReason::Muted, controls);
                return;
            }
            Err(error) => {
                record_error(
                    ToneWorkerError::Render(error),
                    controls.errors,
                    &mut self.report,
                );
                self.force_release(ReleaseReason::Shutdown, controls);
                return;
            }
        };

        let write_result = self.device.as_mut().map(|output| {
            let mut continue_playback = || controls.playback_active();
            output.write_while(tone.samples(), &mut continue_playback)
        });
        match write_result {
            Some(Ok(PcmWriteOutcome::Complete)) if controls.playback_active() => {}
            Some(Ok(PcmWriteOutcome::Complete | PcmWriteOutcome::Cancelled)) => {
                self.force_release(controls.release_reason(), controls);
                return;
            }
            Some(Err(error)) => {
                record_error(
                    ToneWorkerError::Playback(error),
                    controls.errors,
                    &mut self.report,
                );
                let _ = self.lifecycle.failed();
                self.reset_and_drop(controls);
                return;
            }
            None => {
                let _ = self.lifecycle.failed();
                return;
            }
        }

        let _ = self.lifecycle.release(ReleaseReason::Finished);
        match self.device.as_ref().map(CueDevice::drain) {
            Some(Ok(())) => {
                self.lifecycle.drained(monotonic_milliseconds(self.origin));
                let now = Instant::now();
                self.idle_deadline = Some(now.checked_add(self.idle_grace).unwrap_or(now));
                self.report.played = self.report.played.saturating_add(1);
            }
            Some(Err(error)) => {
                record_error(
                    ToneWorkerError::Playback(error),
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

fn run_worker<C, D>(
    receiver: &CueReceiver<WorkerMessage<C>>,
    controls: WorkerControls<'_>,
    requested_rate: SampleRate,
    idle_grace: Duration,
    notes: fn(C) -> &'static [ToneNote],
    mut open_device: impl FnMut(SampleRate) -> Result<D, OssError>,
) -> ToneWorkerReport
where
    C: Copy,
    D: CueDevice,
{
    let mut state = WorkerState::new(idle_grace);

    loop {
        let event = match state.wait_for(receiver) {
            CueReceive::Empty => WorkerEvent::Idle,
            CueReceive::Disconnected => {
                state.force_release(ReleaseReason::Shutdown, controls);
                return state.report;
            }
            CueReceive::Latest(WorkerMessage::Wake) => WorkerEvent::Wake,
            CueReceive::Latest(WorkerMessage::Play(cue)) => WorkerEvent::Play(cue),
        };
        let gate_state = load_gate(controls.gate);

        if gate_state != GATE_ACTIVE {
            state.force_release(gate_release_reason(gate_state), controls);
            if gate_state == GATE_SHUTDOWN {
                return state.report;
            }
            continue;
        }

        match event {
            WorkerEvent::Idle => state.expire_idle(controls),
            WorkerEvent::Wake => {}
            WorkerEvent::Play(cue) => {
                state.play(cue, requested_rate, notes, &mut open_device, controls);
                if load_gate(controls.gate) == GATE_SHUTDOWN {
                    state.force_release(ReleaseReason::Shutdown, controls);
                    return state.report;
                }
            }
        }
    }
}

fn record_error(
    error: ToneWorkerError,
    sender: &SyncSender<ToneWorkerError>,
    report: &mut ToneWorkerReport,
) {
    report.errors = report.errors.saturating_add(1);
    match sender.try_send(error) {
        Ok(()) => {}
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
            report.dropped_errors = report.dropped_errors.saturating_add(1);
        }
    }
}

fn monotonic_milliseconds(origin: Instant) -> u64 {
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[allow(
    clippy::missing_const_for_fn,
    reason = "NonZeroUsize::unwrap_or is not const on the supported Rust toolchain"
)]
fn cue_queue_capacity() -> NonZeroUsize {
    NonZeroUsize::new(CUE_QUEUE_CAPACITY).unwrap_or(NonZeroUsize::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::GATE_HIDDEN;
    use std::io;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    #[derive(Clone, Copy, Debug)]
    enum TestCue {
        Confirm,
    }

    fn test_notes(_cue: TestCue) -> &'static [ToneNote] {
        static NOTES: OnceLock<Vec<ToneNote>> = OnceLock::new();
        NOTES
            .get_or_init(|| ToneNote::new(440, 20).into_iter().collect())
            .as_slice()
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum DeviceEvent {
        Write,
        Drain,
        Reset,
    }

    #[derive(Debug)]
    struct FakeDevice {
        rate: SampleRate,
        events: Arc<Mutex<Vec<DeviceEvent>>>,
    }

    impl CueDevice for FakeDevice {
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
            Ok(if continue_playback() {
                PcmWriteOutcome::Complete
            } else {
                PcmWriteOutcome::Cancelled
            })
        }

        fn drain(&self) -> Result<(), OssError> {
            if let Ok(mut events) = self.events.lock() {
                events.push(DeviceEvent::Drain);
            }
            Ok(())
        }

        fn reset(&self) -> Result<(), OssError> {
            if let Ok(mut events) = self.events.lock() {
                events.push(DeviceEvent::Reset);
            }
            Ok(())
        }
    }

    #[test]
    fn worker_opens_lazily_drains_and_releases_on_disconnect() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(volume) = Volume::new(80) else {
            return;
        };
        let (sender, receiver) = cue_channel(cue_queue_capacity());
        assert_eq!(
            sender.try_enqueue(WorkerMessage::Play(TestCue::Confirm)),
            CueEnqueue::Queued
        );
        drop(sender);
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let gate = Arc::new(AtomicU8::new(GATE_ACTIVE));
        let volume = Arc::new(AtomicU8::new(volume.percent()));
        let released = Arc::new(AtomicBool::new(true));
        let events = Arc::new(Mutex::new(Vec::new()));
        let device_events = Arc::clone(&events);
        let open_released = Arc::clone(&released);

        let report = run_worker(
            &receiver,
            WorkerControls {
                errors: &error_sender,
                gate: gate.as_ref(),
                volume: volume.as_ref(),
                released: released.as_ref(),
            },
            rate,
            Duration::ZERO,
            test_notes,
            move |opened_rate| {
                assert!(!open_released.load(Ordering::Acquire));
                Ok(FakeDevice {
                    rate: opened_rate,
                    events: Arc::clone(&device_events),
                })
            },
        );

        assert_eq!(report.opened, 1);
        assert_eq!(report.played, 1);
        assert_eq!(report.errors, 0);
        assert!(released.load(Ordering::Acquire));
        assert!(errors.try_recv().is_err());
        let Ok(events) = events.lock() else {
            return;
        };
        assert_eq!(events.as_slice(), &[DeviceEvent::Write, DeviceEvent::Drain]);
    }

    #[test]
    fn muted_gate_discards_cues_without_opening_a_device() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let (sender, receiver) = cue_channel(cue_queue_capacity());
        assert_eq!(
            sender.try_enqueue(WorkerMessage::Play(TestCue::Confirm)),
            CueEnqueue::Queued
        );
        drop(sender);
        let (error_sender, _errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let gate = Arc::new(AtomicU8::new(GATE_MUTED));
        let volume = Arc::new(AtomicU8::new(0));
        let released = Arc::new(AtomicBool::new(true));
        let mut open_calls = 0;

        let report = run_worker::<_, FakeDevice>(
            &receiver,
            WorkerControls {
                errors: &error_sender,
                gate: gate.as_ref(),
                volume: volume.as_ref(),
                released: released.as_ref(),
            },
            rate,
            Duration::ZERO,
            test_notes,
            |_opened_rate| {
                open_calls += 1;
                Err(OssError::Open {
                    path: PathBuf::from("/dev/dsp"),
                    source: io::Error::other("must not open"),
                })
            },
        );

        assert_eq!(open_calls, 0);
        assert_eq!(report, ToneWorkerReport::default());
    }

    #[test]
    fn idle_expiration_closes_a_drained_device_without_reset() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let (error_sender, _errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let gate = AtomicU8::new(GATE_ACTIVE);
        let volume = AtomicU8::new(80);
        let released = AtomicBool::new(false);
        let controls = WorkerControls {
            errors: &error_sender,
            gate: &gate,
            volume: &volume,
            released: &released,
        };
        let mut state = WorkerState::new(Duration::ZERO);
        assert_eq!(state.lifecycle.request_playback(), AudioAction::OpenDevice);
        state.lifecycle.opened(true);
        state.device = Some(FakeDevice {
            rate,
            events: Arc::clone(&events),
        });
        assert_eq!(
            state.lifecycle.release(ReleaseReason::Finished),
            AudioAction::None
        );
        state.lifecycle.drained(0);

        state.expire_idle(controls);

        assert_eq!(state.lifecycle.state(), AudioState::Closed);
        assert!(state.device.is_none());
        assert!(released.load(Ordering::Acquire));
        let Ok(events) = events.lock() else {
            return;
        };
        assert!(events.is_empty());
    }

    #[test]
    fn hidden_release_resets_active_audio_before_drop() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let (error_sender, _errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let gate = AtomicU8::new(GATE_HIDDEN);
        let volume = AtomicU8::new(80);
        let released = AtomicBool::new(false);
        let controls = WorkerControls {
            errors: &error_sender,
            gate: &gate,
            volume: &volume,
            released: &released,
        };
        let mut state = WorkerState::new(IDLE_GRACE);
        assert_eq!(state.lifecycle.request_playback(), AudioAction::OpenDevice);
        state.lifecycle.opened(true);
        state.device = Some(FakeDevice {
            rate,
            events: Arc::clone(&events),
        });

        state.force_release(ReleaseReason::Hidden, controls);

        assert_eq!(state.lifecycle.state(), AudioState::Closed);
        assert!(state.device.is_none());
        assert!(released.load(Ordering::Acquire));
        let Ok(events) = events.lock() else {
            return;
        };
        assert_eq!(events.as_slice(), &[DeviceEvent::Reset]);
    }

    #[test]
    fn public_handle_never_enqueues_while_muted() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let worker = ToneCueWorker::spawn(rate, Volume::MUTED, test_notes);
        assert!(worker.is_ok());
        let Ok(worker) = worker else {
            return;
        };
        assert!(worker.device_released());
        assert_eq!(
            worker.try_play(TestCue::Confirm),
            ToneCueEnqueue::DroppedInactive
        );
        assert_eq!(worker.shutdown(), ToneWorkerReport::default());
    }

    #[test]
    fn volume_changes_do_not_discard_hidden_or_paused_state() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Some(audible) = Volume::new(80) else {
            return;
        };
        let worker = ToneCueWorker::spawn(rate, audible, test_notes);
        assert!(worker.is_ok());
        let Ok(worker) = worker else {
            return;
        };
        for gate in [AudioGate::Hidden, AudioGate::Paused] {
            worker.set_gate(gate);
            worker.set_volume(Volume::MUTED);
            worker.set_volume(audible);
            assert_eq!(load_gate(&worker.gate), gate.code());
        }
        let _ = worker.shutdown();
    }
}
