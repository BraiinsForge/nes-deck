//! Bounded producer and lazy OSS worker for emulator-style PCM streams.

use std::collections::TryReserveError;
use std::error::Error;
use std::fmt;
use std::io;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use retro_deck_audio::{
    AudioAction, AudioLifecycle, LinearResampler, MonoPcmQueue, PcmPushReport, ReleaseReason,
    SampleRate, Volume, apply_volume,
};

use super::{
    AudioGate, GATE_ACTIVE, GATE_MUTED, GATE_SHUTDOWN, OssError, OssPcm, OssProfile,
    PcmWriteOutcome, gate_release_reason, load_gate,
};

const PCM_QUEUE_FRAMES: usize = 16_384;
const PCM_WORK_FRAMES: usize = 2_048;
const WAKE_QUEUE_CAPACITY: usize = 1;
const ERROR_QUEUE_CAPACITY: usize = 8;
const IDLE_RELEASE_DELAY: Duration = Duration::from_millis(100);
const OPEN_RETRY_DELAY: Duration = Duration::from_secs(1);
const WORKER_NAME: &str = "retro-deck-pcm-stream";
const GAMBATTE_SOURCE_RATE: u32 = 32_768;
const GAMBATTE_DEVICE_RATE: u32 = 32_000;
const FCEUMM_NOMINAL_RATE: u32 = 48_000;
const FCEUMM_EFFECTIVE_RATE: u32 = 47_328;

/// Result of submitting one PCM callback without waiting for audio work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcmSubmit {
    /// The bounded queue retained some or all of the callback.
    Queued(PcmPushReport),
    /// Playback is muted, paused, hidden, or shutting down.
    DroppedInactive,
    /// The worker held the queue for a brief copy, so this callback was lost.
    DroppedContended,
    /// The worker thread has exited.
    DroppedDisconnected,
}

/// Nonfatal device error reported by a PCM stream worker.
#[derive(Debug)]
pub enum PcmWorkerError {
    /// Opening or configuring the OSS device failed.
    Open(OssError),
    /// Filling the stream ring before first playback failed.
    Prime(OssError),
    /// Starting or writing stream output failed.
    Playback(OssError),
    /// Draining an idle stream before release failed.
    Drain(OssError),
    /// Discarding queued device audio during forced release failed.
    Reset(OssError),
}

impl fmt::Display for PcmWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(source) => write!(formatter, "cannot open PCM stream: {source}"),
            Self::Prime(source) => write!(formatter, "cannot prime PCM stream: {source}"),
            Self::Playback(source) => write!(formatter, "cannot play PCM stream: {source}"),
            Self::Drain(source) => write!(formatter, "cannot drain idle PCM stream: {source}"),
            Self::Reset(source) => write!(formatter, "cannot release PCM stream: {source}"),
        }
    }
}

impl Error for PcmWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open(source)
            | Self::Prime(source)
            | Self::Playback(source)
            | Self::Drain(source)
            | Self::Reset(source) => Some(source),
        }
    }
}

/// Failure before a PCM worker thread becomes available.
#[derive(Debug)]
pub enum PcmWorkerStartError {
    /// The fixed PCM queue could not be allocated.
    Queue(TryReserveError),
    /// The operating system could not create the dedicated worker thread.
    Thread(io::Error),
}

impl fmt::Display for PcmWorkerStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queue(source) => write!(formatter, "cannot allocate PCM queue: {source}"),
            Self::Thread(source) => write!(formatter, "cannot start PCM worker: {source}"),
        }
    }
}

impl Error for PcmWorkerStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Queue(source) => Some(source),
            Self::Thread(source) => Some(source),
        }
    }
}

/// Nonblocking producer diagnostics available while playback is running.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PcmStreamStats {
    /// Frames currently waiting in the bounded source queue.
    pub queued_frames: usize,
    /// Newly submitted frames retained by successful queue operations.
    pub accepted_frames: u64,
    /// Old or oversized frames discarded to keep audio recent.
    pub overflow_frames: u64,
    /// Callback frames discarded instead of waiting for the queue lock.
    pub contended_frames: u64,
    /// Callback frames discarded while playback was ineligible.
    pub inactive_frames: u64,
    /// Callback frames discarded after the worker exited.
    pub disconnected_frames: u64,
}

/// Final diagnostics returned by an explicitly stopped PCM worker.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PcmWorkerReport {
    /// Final producer-side queue statistics.
    pub stream: PcmStreamStats,
    /// Successful lazy OSS acquisitions.
    pub opened: u64,
    /// Silence frames written while holding the initial stream trigger.
    pub primed_frames: u64,
    /// Resampled mono frames written after gain.
    pub written_frames: u64,
    /// Devices drained normally after the idle deadline.
    pub drained: u64,
    /// Owned devices dropped after a drain or reset.
    pub released: u64,
    /// Queued source frames discarded on mute, pause, hide, or shutdown.
    pub discarded_frames: u64,
    /// Open, prime, playback, drain, or reset failures.
    pub errors: u64,
    /// Errors discarded because the diagnostic queue was full.
    pub dropped_errors: u64,
    /// Whether the worker thread panicked before shutdown.
    pub panicked: bool,
}

/// Producer handle for one lazily owned continuous OSS PCM stream.
///
/// Callback submission, gate changes, and volume changes never perform device
/// I/O and never wait for the worker. Callback contention drops sound rather
/// than delaying emulator, touch, or controller processing.
#[derive(Debug)]
pub struct PcmStreamWorker {
    wake_sender: Option<SyncSender<()>>,
    errors: Receiver<PcmWorkerError>,
    queue: Arc<Mutex<MonoPcmQueue>>,
    queue_len: Arc<AtomicUsize>,
    gate: Arc<AtomicU8>,
    volume: Arc<AtomicU8>,
    alive: Arc<AtomicBool>,
    counters: Arc<ProducerCounters>,
    thread: Option<JoinHandle<PcmWorkerReport>>,
}

impl PcmStreamWorker {
    /// Spawn a worker without opening `/dev/dsp`.
    ///
    /// # Errors
    ///
    /// Returns [`PcmWorkerStartError`] if fixed queue allocation or worker
    /// thread creation fails. Device errors are asynchronous and available
    /// through [`Self::take_errors`].
    pub fn spawn(
        source_rate: SampleRate,
        initial_volume: Volume,
    ) -> Result<Self, PcmWorkerStartError> {
        let queue =
            MonoPcmQueue::try_new(pcm_queue_capacity()).map_err(PcmWorkerStartError::Queue)?;
        let queue = Arc::new(Mutex::new(queue));
        let queue_len = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(AtomicU8::new(if initial_volume.muted() {
            GATE_MUTED
        } else {
            GATE_ACTIVE
        }));
        let volume = Arc::new(AtomicU8::new(initial_volume.percent()));
        let alive = Arc::new(AtomicBool::new(true));
        let counters = Arc::new(ProducerCounters::default());
        let (wake_sender, wake_receiver) = mpsc::sync_channel(WAKE_QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);

        let worker_queue = Arc::clone(&queue);
        let worker_queue_len = Arc::clone(&queue_len);
        let worker_gate = Arc::clone(&gate);
        let worker_volume = Arc::clone(&volume);
        let worker_alive = Arc::clone(&alive);
        let worker_counters = Arc::clone(&counters);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || {
                let _alive_guard = AliveGuard(worker_alive);
                run_worker(
                    &wake_receiver,
                    WorkerControls {
                        errors: &error_sender,
                        queue: worker_queue.as_ref(),
                        queue_len: worker_queue_len.as_ref(),
                        gate: worker_gate.as_ref(),
                        volume: worker_volume.as_ref(),
                        counters: worker_counters.as_ref(),
                    },
                    source_rate,
                    IDLE_RELEASE_DELAY,
                    OPEN_RETRY_DELAY,
                    |rate| OssPcm::open_mono(rate, OssProfile::Stream),
                )
            })
            .map_err(PcmWorkerStartError::Thread)?;

        Ok(Self {
            wake_sender: Some(wake_sender),
            errors,
            queue,
            queue_len,
            gate,
            volume,
            alive,
            counters,
            thread: Some(thread),
        })
    }

    /// Try to downmix and queue one stereo callback without waiting.
    #[must_use]
    pub fn try_push_stereo(&self, frames: &[[i16; 2]]) -> PcmSubmit {
        self.try_submit(frames.len(), |queue| queue.push_stereo_latest(frames))
    }

    /// Try to queue one mono callback without waiting.
    #[must_use]
    pub fn try_push_mono(&self, frames: &[i16]) -> PcmSubmit {
        self.try_submit(frames.len(), |queue| queue.push_mono_latest(frames))
    }

    /// Change playback eligibility and wake the worker for prompt release.
    pub fn set_gate(&self, gate: AudioGate) {
        self.gate.store(gate.code(), Ordering::Release);
        self.wake();
    }

    /// Change gain without waiting for queued or device audio.
    ///
    /// Muting enters the muted gate. Raising volume preserves hidden and
    /// paused state, but reactivates a worker muted through this method.
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

    /// Snapshot producer counters without locking the PCM queue.
    #[must_use]
    pub fn stats(&self) -> PcmStreamStats {
        producer_stats(self.queue_len.as_ref(), self.counters.as_ref())
    }

    /// Drain currently reported device errors without waiting.
    #[must_use]
    pub fn take_errors(&self) -> Vec<PcmWorkerError> {
        let mut errors = Vec::new();
        loop {
            match self.errors.try_recv() {
                Ok(error) => errors.push(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return errors,
            }
        }
    }

    /// Request immediate reset and release, then return final diagnostics.
    #[must_use]
    pub fn shutdown(mut self) -> PcmWorkerReport {
        self.stop_and_join()
    }

    fn try_submit(
        &self,
        frame_count: usize,
        submit: impl FnOnce(&mut MonoPcmQueue) -> PcmPushReport,
    ) -> PcmSubmit {
        if frame_count == 0 {
            return PcmSubmit::Queued(PcmPushReport::default());
        }
        if !self.alive.load(Ordering::Acquire) {
            add_frames(&self.counters.disconnected, frame_count);
            return PcmSubmit::DroppedDisconnected;
        }
        if !self.playback_active() {
            add_frames(&self.counters.inactive, frame_count);
            return PcmSubmit::DroppedInactive;
        }

        let mut queue = match self.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::WouldBlock) => {
                add_frames(&self.counters.contended, frame_count);
                return PcmSubmit::DroppedContended;
            }
            Err(TryLockError::Poisoned(_)) => {
                self.alive.store(false, Ordering::Release);
                add_frames(&self.counters.disconnected, frame_count);
                return PcmSubmit::DroppedDisconnected;
            }
        };
        if !self.playback_active() {
            add_frames(&self.counters.inactive, frame_count);
            return PcmSubmit::DroppedInactive;
        }
        let report = submit(&mut queue);
        self.queue_len.store(queue.len(), Ordering::Release);
        drop(queue);

        let Some(sender) = &self.wake_sender else {
            add_frames(&self.counters.disconnected, frame_count);
            return PcmSubmit::DroppedDisconnected;
        };
        match sender.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {
                add_frames(&self.counters.accepted, report.accepted_frames);
                add_frames(&self.counters.overflow, report.dropped_frames);
                PcmSubmit::Queued(report)
            }
            Err(TrySendError::Disconnected(())) => {
                self.alive.store(false, Ordering::Release);
                add_frames(&self.counters.disconnected, frame_count);
                PcmSubmit::DroppedDisconnected
            }
        }
    }

    fn playback_active(&self) -> bool {
        load_gate(self.gate.as_ref()) == GATE_ACTIVE && self.volume.load(Ordering::Acquire) != 0
    }

    fn wake(&self) {
        if let Some(sender) = &self.wake_sender {
            match sender.try_send(()) {
                Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => {}
            }
        }
    }

    fn stop_and_join(&mut self) -> PcmWorkerReport {
        self.gate.store(GATE_SHUTDOWN, Ordering::Release);
        self.wake();
        let _ = self.wake_sender.take();
        let Some(thread) = self.thread.take() else {
            return PcmWorkerReport {
                stream: self.stats(),
                ..PcmWorkerReport::default()
            };
        };
        match thread.join() {
            Ok(report) => report,
            Err(_) => PcmWorkerReport {
                stream: self.stats(),
                panicked: true,
                ..PcmWorkerReport::default()
            },
        }
    }
}

impl Drop for PcmStreamWorker {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

#[derive(Debug, Default)]
struct ProducerCounters {
    accepted: AtomicU64,
    overflow: AtomicU64,
    contended: AtomicU64,
    inactive: AtomicU64,
    disconnected: AtomicU64,
}

struct AliveGuard(Arc<AtomicBool>);

impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

trait PcmDevice {
    fn sample_rate(&self) -> SampleRate;
    fn prime(&mut self) -> Result<usize, OssError>;
    fn start(&mut self) -> Result<(), OssError>;
    fn write_while(
        &mut self,
        samples: &[i16],
        continue_playback: &mut dyn FnMut() -> bool,
    ) -> Result<PcmWriteOutcome, OssError>;
    fn drain(&self) -> Result<(), OssError>;
    fn reset(&self) -> Result<(), OssError>;
}

impl PcmDevice for OssPcm {
    fn sample_rate(&self) -> SampleRate {
        self.sample_rate()
    }

    fn prime(&mut self) -> Result<usize, OssError> {
        self.prime_stream()
    }

    fn start(&mut self) -> Result<(), OssError> {
        self.start_output()
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
    errors: &'a SyncSender<PcmWorkerError>,
    queue: &'a Mutex<MonoPcmQueue>,
    queue_len: &'a AtomicUsize,
    gate: &'a AtomicU8,
    volume: &'a AtomicU8,
    counters: &'a ProducerCounters,
}

impl WorkerControls<'_> {
    fn playback_active(self) -> bool {
        load_gate(self.gate) == GATE_ACTIVE && self.volume.load(Ordering::Acquire) != 0
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

struct WorkerState<D> {
    lifecycle: AudioLifecycle,
    device: Option<D>,
    resampler: Option<LinearResampler>,
    output_started: bool,
    report: PcmWorkerReport,
}

impl<D: PcmDevice> WorkerState<D> {
    fn new() -> Self {
        Self {
            lifecycle: AudioLifecycle::new(Duration::ZERO),
            device: None,
            resampler: None,
            output_started: false,
            report: PcmWorkerReport::default(),
        }
    }

    fn ensure_device(
        &mut self,
        source_rate: SampleRate,
        open_device: &mut impl FnMut(SampleRate) -> Result<D, OssError>,
        controls: WorkerControls<'_>,
    ) -> bool {
        if !matches!(self.lifecycle.request_playback(), AudioAction::OpenDevice) {
            return self.device.is_some();
        }
        let requested_rate = requested_device_rate(source_rate);
        let mut device = match open_device(requested_rate) {
            Ok(device) => device,
            Err(error) => {
                self.lifecycle.opened(false);
                record_error(PcmWorkerError::Open(error), controls, &mut self.report);
                return false;
            }
        };
        let primed = match device.prime() {
            Ok(primed) => primed,
            Err(error) => {
                self.lifecycle.opened(false);
                record_error(PcmWorkerError::Prime(error), controls, &mut self.report);
                return false;
            }
        };
        let output_rate = effective_output_rate(source_rate, device.sample_rate());
        self.lifecycle.opened(true);
        self.device = Some(device);
        self.resampler = Some(LinearResampler::new(source_rate, output_rate));
        self.output_started = false;
        self.report.opened = self.report.opened.saturating_add(1);
        self.report.primed_frames = self
            .report
            .primed_frames
            .saturating_add(u64::try_from(primed).unwrap_or(u64::MAX));
        true
    }

    fn play_source(
        &mut self,
        samples: &[i16],
        controls: WorkerControls<'_>,
        output: &mut [i16; PCM_WORK_FRAMES],
    ) {
        let mut remaining = samples;
        loop {
            if !controls.playback_active() {
                self.force_release(controls.release_reason(), controls);
                return;
            }
            let Some(resampler) = self.resampler.as_mut() else {
                let _ = self.lifecycle.failed();
                self.reset_and_drop(controls);
                return;
            };
            if remaining.is_empty() && !resampler.has_pending_output() {
                return;
            }
            let progress = resampler.process(remaining, output);
            let (_, unconsumed) = remaining.split_at(progress.consumed_frames);
            remaining = unconsumed;
            if progress.produced_frames == 0 {
                if progress.consumed_frames == 0 {
                    return;
                }
                continue;
            }
            let Some(chunk) = output.get_mut(..progress.produced_frames) else {
                return;
            };
            let Some(volume) = Volume::new(controls.volume.load(Ordering::Acquire)) else {
                self.force_release(ReleaseReason::Muted, controls);
                return;
            };
            apply_volume(chunk, volume);

            if !self.output_started {
                match self.device.as_mut().map(PcmDevice::start) {
                    Some(Ok(())) => self.output_started = true,
                    Some(Err(error)) => {
                        record_error(PcmWorkerError::Playback(error), controls, &mut self.report);
                        let _ = self.lifecycle.failed();
                        self.reset_and_drop(controls);
                        return;
                    }
                    None => {
                        let _ = self.lifecycle.failed();
                        return;
                    }
                }
            }

            let write = self.device.as_mut().map(|device| {
                let mut continue_playback = || controls.playback_active();
                device.write_while(chunk, &mut continue_playback)
            });
            match write {
                Some(Ok(PcmWriteOutcome::Complete)) => {
                    self.report.written_frames = self.report.written_frames.saturating_add(
                        u64::try_from(progress.produced_frames).unwrap_or(u64::MAX),
                    );
                }
                Some(Ok(PcmWriteOutcome::Cancelled)) => {
                    self.force_release(controls.release_reason(), controls);
                    return;
                }
                Some(Err(error)) => {
                    record_error(PcmWorkerError::Playback(error), controls, &mut self.report);
                    let _ = self.lifecycle.failed();
                    self.reset_and_drop(controls);
                    return;
                }
                None => {
                    let _ = self.lifecycle.failed();
                    return;
                }
            }
        }
    }

    const fn reset_resampler(&mut self) {
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
    }

    fn drain_and_release(&mut self, controls: WorkerControls<'_>) {
        if self.device.is_none() {
            return;
        }
        let _ = self.lifecycle.release(ReleaseReason::Finished);
        match self.device.as_ref().map(PcmDevice::drain) {
            Some(Ok(())) => {
                self.lifecycle.drained(0);
                let _ = self.lifecycle.tick(0);
                let _ = self.device.take();
                self.resampler = None;
                self.output_started = false;
                self.report.drained = self.report.drained.saturating_add(1);
                self.report.released = self.report.released.saturating_add(1);
            }
            Some(Err(error)) => {
                record_error(PcmWorkerError::Drain(error), controls, &mut self.report);
                let _ = self.lifecycle.failed();
                self.reset_and_drop(controls);
            }
            None => {
                let _ = self.lifecycle.failed();
            }
        }
    }

    fn force_release(&mut self, reason: ReleaseReason, controls: WorkerControls<'_>) {
        let discarded = clear_queue_for_release(reason, controls);
        self.report.discarded_frames = self
            .report
            .discarded_frames
            .saturating_add(u64::try_from(discarded).unwrap_or(u64::MAX));
        let _ = self.lifecycle.release(reason);
        self.reset_resampler();
        self.reset_and_drop(controls);
    }

    fn reset_and_drop(&mut self, controls: WorkerControls<'_>) {
        if let Some(device) = self.device.take() {
            self.report.released = self.report.released.saturating_add(1);
            if let Err(error) = device.reset() {
                record_error(PcmWorkerError::Reset(error), controls, &mut self.report);
            }
        }
        self.resampler = None;
        self.output_started = false;
    }

    fn finish(mut self, controls: WorkerControls<'_>) -> PcmWorkerReport {
        self.report.stream = producer_stats(controls.queue_len, controls.counters);
        self.report
    }
}

fn run_worker<D: PcmDevice>(
    wake_receiver: &Receiver<()>,
    controls: WorkerControls<'_>,
    source_rate: SampleRate,
    idle_release_delay: Duration,
    open_retry_delay: Duration,
    mut open_device: impl FnMut(SampleRate) -> Result<D, OssError>,
) -> PcmWorkerReport {
    let mut state = WorkerState::new();
    let mut source = [0_i16; PCM_WORK_FRAMES];
    let mut output = [0_i16; PCM_WORK_FRAMES];

    loop {
        if controls.shutdown_requested() {
            state.force_release(ReleaseReason::Shutdown, controls);
            return state.finish(controls);
        }
        if !controls.playback_active() {
            state.force_release(controls.release_reason(), controls);
            if wake_receiver.recv().is_err() {
                state.force_release(ReleaseReason::Shutdown, controls);
                return state.finish(controls);
            }
            continue;
        }

        if controls.queue_len.load(Ordering::Acquire) == 0 {
            if state.device.is_none() {
                if wake_receiver.recv().is_err() {
                    state.force_release(ReleaseReason::Shutdown, controls);
                    return state.finish(controls);
                }
                continue;
            }
            match wake_receiver.recv_timeout(idle_release_delay) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => {
                    if controls.queue_len.load(Ordering::Acquire) == 0 && controls.playback_active()
                    {
                        state.drain_and_release(controls);
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    state.force_release(ReleaseReason::Shutdown, controls);
                    return state.finish(controls);
                }
            }
        }

        if !state.ensure_device(source_rate, &mut open_device, controls) {
            if !wait_for_retry(wake_receiver, controls, open_retry_delay) {
                state.force_release(ReleaseReason::Shutdown, controls);
                return state.finish(controls);
            }
            continue;
        }
        if !controls.playback_active() {
            state.force_release(controls.release_reason(), controls);
            continue;
        }

        let (count, discontinuities) = pop_source(controls, &mut source);
        if discontinuities != 0 {
            state.reset_resampler();
        }
        if count != 0 {
            let Some(samples) = source.get(..count) else {
                continue;
            };
            state.play_source(samples, controls, &mut output);
        }
    }
}

fn wait_for_retry(
    wake_receiver: &Receiver<()>,
    controls: WorkerControls<'_>,
    delay: Duration,
) -> bool {
    let started = std::time::Instant::now();
    loop {
        if controls.shutdown_requested() {
            return false;
        }
        if !controls.playback_active() {
            return true;
        }
        let remaining = delay.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return true;
        }
        match wake_receiver.recv_timeout(remaining) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => return true,
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn pop_source(controls: WorkerControls<'_>, output: &mut [i16; PCM_WORK_FRAMES]) -> (usize, u64) {
    let mut queue = match controls.queue.lock() {
        Ok(queue) => queue,
        Err(poisoned) => poisoned.into_inner(),
    };
    let discontinuities = queue.take_discontinuities();
    let count = queue.pop_into(output);
    controls.queue_len.store(queue.len(), Ordering::Release);
    (count, discontinuities)
}

fn clear_queue_for_release(reason: ReleaseReason, controls: WorkerControls<'_>) -> usize {
    let mut queue = match controls.queue.lock() {
        Ok(queue) => queue,
        Err(poisoned) => poisoned.into_inner(),
    };
    if !matches!(reason, ReleaseReason::Shutdown) && controls.playback_active() {
        controls.queue_len.store(queue.len(), Ordering::Release);
        return 0;
    }
    let discarded = queue.clear();
    let _ = queue.take_discontinuities();
    controls.queue_len.store(0, Ordering::Release);
    discarded
}

fn requested_device_rate(source_rate: SampleRate) -> SampleRate {
    if source_rate.get() == GAMBATTE_SOURCE_RATE {
        SampleRate::new(GAMBATTE_DEVICE_RATE).unwrap_or(source_rate)
    } else {
        source_rate
    }
}

fn effective_output_rate(source_rate: SampleRate, negotiated_rate: SampleRate) -> SampleRate {
    if source_rate.get() == FCEUMM_NOMINAL_RATE && negotiated_rate.get() == FCEUMM_NOMINAL_RATE {
        SampleRate::new(FCEUMM_EFFECTIVE_RATE).unwrap_or(negotiated_rate)
    } else {
        negotiated_rate
    }
}

fn record_error(error: PcmWorkerError, controls: WorkerControls<'_>, report: &mut PcmWorkerReport) {
    report.errors = report.errors.saturating_add(1);
    match controls.errors.try_send(error) {
        Ok(()) => {}
        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
            report.dropped_errors = report.dropped_errors.saturating_add(1);
        }
    }
}

fn producer_stats(queue_len: &AtomicUsize, counters: &ProducerCounters) -> PcmStreamStats {
    PcmStreamStats {
        queued_frames: queue_len.load(Ordering::Acquire),
        accepted_frames: counters.accepted.load(Ordering::Relaxed),
        overflow_frames: counters.overflow.load(Ordering::Relaxed),
        contended_frames: counters.contended.load(Ordering::Relaxed),
        inactive_frames: counters.inactive.load(Ordering::Relaxed),
        disconnected_frames: counters.disconnected.load(Ordering::Relaxed),
    }
}

fn add_frames(counter: &AtomicU64, frames: usize) {
    let amount = u64::try_from(frames).unwrap_or(u64::MAX);
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(amount))
    });
}

#[allow(
    clippy::missing_const_for_fn,
    reason = "NonZeroUsize::unwrap_or is not const on the supported Rust toolchain"
)]
fn pcm_queue_capacity() -> NonZeroUsize {
    NonZeroUsize::new(PCM_QUEUE_FRAMES).unwrap_or(NonZeroUsize::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::GATE_HIDDEN;
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum DeviceEvent {
        Open(SampleRate),
        Prime,
        Start,
        Write(usize),
        Drain,
        Reset,
    }

    #[derive(Debug)]
    struct FakeDevice {
        rate: SampleRate,
        events: Sender<DeviceEvent>,
        written: Arc<Mutex<Vec<i16>>>,
        cancel_on_write: Option<Arc<AtomicU8>>,
    }

    impl PcmDevice for FakeDevice {
        fn sample_rate(&self) -> SampleRate {
            self.rate
        }

        fn prime(&mut self) -> Result<usize, OssError> {
            let _ = self.events.send(DeviceEvent::Prime);
            Ok(4_096)
        }

        fn start(&mut self) -> Result<(), OssError> {
            let _ = self.events.send(DeviceEvent::Start);
            Ok(())
        }

        fn write_while(
            &mut self,
            samples: &[i16],
            continue_playback: &mut dyn FnMut() -> bool,
        ) -> Result<PcmWriteOutcome, OssError> {
            if let Ok(mut written) = self.written.lock() {
                written.extend_from_slice(samples);
            }
            let _ = self.events.send(DeviceEvent::Write(samples.len()));
            if let Some(gate) = &self.cancel_on_write {
                gate.store(GATE_MUTED, Ordering::Release);
            }
            Ok(if continue_playback() {
                PcmWriteOutcome::Complete
            } else {
                PcmWriteOutcome::Cancelled
            })
        }

        fn drain(&self) -> Result<(), OssError> {
            let _ = self.events.send(DeviceEvent::Drain);
            Ok(())
        }

        fn reset(&self) -> Result<(), OssError> {
            let _ = self.events.send(DeviceEvent::Reset);
            Ok(())
        }
    }

    fn test_controls<'a>(
        errors: &'a SyncSender<PcmWorkerError>,
        queue: &'a Mutex<MonoPcmQueue>,
        queue_len: &'a AtomicUsize,
        gate: &'a AtomicU8,
        volume: &'a AtomicU8,
        counters: &'a ProducerCounters,
    ) -> WorkerControls<'a> {
        WorkerControls {
            errors,
            queue,
            queue_len,
            gate,
            volume,
            counters,
        }
    }

    fn test_queue() -> Option<MonoPcmQueue> {
        MonoPcmQueue::try_new(pcm_queue_capacity()).ok()
    }

    #[test]
    fn deck_clock_corrections_match_live_measurements() {
        let (Some(gambatte), Some(fceumm), Some(ordinary)) = (
            SampleRate::new(32_768),
            SampleRate::new(48_000),
            SampleRate::new(44_100),
        ) else {
            return;
        };
        assert_eq!(requested_device_rate(gambatte).get(), 32_000);
        assert_eq!(requested_device_rate(ordinary), ordinary);
        assert_eq!(effective_output_rate(fceumm, fceumm).get(), 47_328);
        assert_eq!(
            effective_output_rate(gambatte, requested_device_rate(gambatte)).get(),
            32_000
        );
        assert_eq!(effective_output_rate(ordinary, ordinary), ordinary);
    }

    #[test]
    fn callback_contention_drops_audio_without_waiting() {
        let (Some(rate), Some(volume)) = (SampleRate::new(44_100), Volume::new(42)) else {
            return;
        };
        let Ok(worker) = PcmStreamWorker::spawn(rate, volume) else {
            return;
        };
        {
            let Ok(_held) = worker.queue.lock() else {
                return;
            };
            assert_eq!(
                worker.try_push_mono(&[1, 2, 3]),
                PcmSubmit::DroppedContended
            );
        }
        assert_eq!(worker.stats().contended_frames, 3);
        let report = worker.shutdown();
        assert_eq!(report.opened, 0);
        assert_eq!(report.stream.contended_frames, 3);
    }

    #[test]
    fn muted_public_worker_never_opens_the_device() {
        let Some(rate) = SampleRate::new(44_100) else {
            return;
        };
        let Ok(worker) = PcmStreamWorker::spawn(rate, Volume::MUTED) else {
            return;
        };
        assert_eq!(
            worker.try_push_stereo(&[[100, -100]; 4]),
            PcmSubmit::DroppedInactive
        );
        let report = worker.shutdown();
        assert_eq!(report.opened, 0);
        assert_eq!(report.stream.inactive_frames, 4);
    }

    #[test]
    fn volume_changes_preserve_hidden_and_paused_gates() {
        let (Some(rate), Some(volume)) = (SampleRate::new(44_100), Volume::new(42)) else {
            return;
        };
        let Ok(worker) = PcmStreamWorker::spawn(rate, volume) else {
            return;
        };
        worker.set_gate(AudioGate::Hidden);
        worker.set_volume(Volume::MUTED);
        worker.set_volume(volume);
        assert_eq!(load_gate(worker.gate.as_ref()), GATE_HIDDEN);
        worker.set_gate(AudioGate::Paused);
        worker.set_volume(Volume::MUTED);
        worker.set_volume(volume);
        assert_eq!(load_gate(worker.gate.as_ref()), AudioGate::Paused.code());
        let _ = worker.shutdown();
    }

    #[test]
    fn active_pcm_opens_lazily_primes_writes_drains_and_releases() {
        let (Some(source_rate), Some(volume), Some(mut queue)) =
            (SampleRate::new(44_100), Volume::new(50), test_queue())
        else {
            return;
        };
        let report = queue.push_stereo_latest(&[[1_000, 3_000], [-2_000, 2_000]]);
        assert_eq!(report.accepted_frames, 2);
        let queue = Arc::new(Mutex::new(queue));
        let queue_len = Arc::new(AtomicUsize::new(2));
        let gate = Arc::new(AtomicU8::new(GATE_ACTIVE));
        let volume_atomic = AtomicU8::new(volume.percent());
        let counters = ProducerCounters::default();
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let (wake_sender, wake_receiver) = mpsc::sync_channel::<()>(WAKE_QUEUE_CAPACITY);
        let (event_sender, event_receiver) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let thread_queue = Arc::clone(&queue);
        let thread_queue_len = Arc::clone(&queue_len);
        let thread_gate = Arc::clone(&gate);
        let device_written = Arc::clone(&written);
        let worker = thread::spawn(move || {
            run_worker(
                &wake_receiver,
                test_controls(
                    &error_sender,
                    thread_queue.as_ref(),
                    thread_queue_len.as_ref(),
                    thread_gate.as_ref(),
                    &volume_atomic,
                    &counters,
                ),
                source_rate,
                Duration::ZERO,
                Duration::ZERO,
                |rate| {
                    let _ = event_sender.send(DeviceEvent::Open(rate));
                    Ok(FakeDevice {
                        rate,
                        events: event_sender.clone(),
                        written: Arc::clone(&device_written),
                        cancel_on_write: None,
                    })
                },
            )
        });

        let mut events = Vec::new();
        while let Ok(event) = event_receiver.recv_timeout(Duration::from_secs(1)) {
            events.push(event);
            if matches!(event, DeviceEvent::Drain) {
                gate.store(GATE_SHUTDOWN, Ordering::Release);
                let _ = wake_sender.try_send(());
                break;
            }
        }
        let Ok(report) = worker.join() else {
            return;
        };
        assert!(errors.try_recv().is_err());
        assert_eq!(
            events,
            [
                DeviceEvent::Open(source_rate),
                DeviceEvent::Prime,
                DeviceEvent::Start,
                DeviceEvent::Write(2),
                DeviceEvent::Drain,
            ]
        );
        let Ok(written) = written.lock() else {
            return;
        };
        assert_eq!(written.as_slice(), [1_000, 0]);
        assert_eq!(report.opened, 1);
        assert_eq!(report.primed_frames, 4_096);
        assert_eq!(report.written_frames, 2);
        assert_eq!(report.drained, 1);
        assert_eq!(report.released, 1);
    }

    #[test]
    fn mute_during_write_resets_immediately_and_clears_queued_pcm() {
        let (Some(source_rate), Some(volume), Some(mut queue)) =
            (SampleRate::new(44_100), Volume::new(50), test_queue())
        else {
            return;
        };
        let _ = queue.push_mono_latest(&[1_000, 2_000, 3_000]);
        let queue = Arc::new(Mutex::new(queue));
        let queue_len = Arc::new(AtomicUsize::new(3));
        let gate = Arc::new(AtomicU8::new(GATE_ACTIVE));
        let volume_atomic = AtomicU8::new(volume.percent());
        let counters = ProducerCounters::default();
        let (error_sender, _errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let (wake_sender, wake_receiver) = mpsc::sync_channel(WAKE_QUEUE_CAPACITY);
        let (event_sender, event_receiver) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let thread_queue = Arc::clone(&queue);
        let thread_queue_len = Arc::clone(&queue_len);
        let thread_gate = Arc::clone(&gate);
        let device_gate = Arc::clone(&gate);
        let worker = thread::spawn(move || {
            run_worker(
                &wake_receiver,
                test_controls(
                    &error_sender,
                    thread_queue.as_ref(),
                    thread_queue_len.as_ref(),
                    thread_gate.as_ref(),
                    &volume_atomic,
                    &counters,
                ),
                source_rate,
                Duration::from_secs(1),
                Duration::ZERO,
                |rate| {
                    Ok(FakeDevice {
                        rate,
                        events: event_sender.clone(),
                        written: Arc::clone(&written),
                        cancel_on_write: Some(Arc::clone(&device_gate)),
                    })
                },
            )
        });

        while let Ok(event) = event_receiver.recv_timeout(Duration::from_secs(1)) {
            if matches!(event, DeviceEvent::Reset) {
                gate.store(GATE_SHUTDOWN, Ordering::Release);
                let _ = wake_sender.try_send(());
                break;
            }
        }
        let Ok(report) = worker.join() else {
            return;
        };
        assert_eq!(queue_len.load(Ordering::Acquire), 0);
        assert_eq!(report.released, 1);
        assert_eq!(report.drained, 0);
    }

    #[test]
    fn open_failures_do_not_discard_waiting_source_pcm() {
        let (Some(source_rate), Some(volume), Some(mut queue)) =
            (SampleRate::new(44_100), Volume::new(50), test_queue())
        else {
            return;
        };
        let _ = queue.push_mono_latest(&[1, 2, 3]);
        let queue = Mutex::new(queue);
        let queue_len = AtomicUsize::new(3);
        let gate = AtomicU8::new(GATE_ACTIVE);
        let volume = AtomicU8::new(volume.percent());
        let counters = ProducerCounters::default();
        let (error_sender, errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let (wake_sender, wake_receiver) = mpsc::sync_channel::<()>(WAKE_QUEUE_CAPACITY);
        gate.store(GATE_SHUTDOWN, Ordering::Release);
        drop(wake_sender);
        let mut state = WorkerState::<FakeDevice>::new();
        let controls = test_controls(&error_sender, &queue, &queue_len, &gate, &volume, &counters);
        let opened = state.ensure_device(
            source_rate,
            &mut |_rate| {
                Err(OssError::Open {
                    path: PathBuf::from("/dev/dsp"),
                    source: io::Error::other("fixture open failure"),
                })
            },
            controls,
        );
        assert!(!opened);
        assert_eq!(queue_len.load(Ordering::Acquire), 3);
        assert!(matches!(errors.try_recv(), Ok(PcmWorkerError::Open(_))));
        drop(wake_receiver);
    }

    #[test]
    fn stale_release_cannot_clear_pcm_queued_after_reactivation() {
        let (Some(volume), Some(mut queue)) = (Volume::new(50), test_queue()) else {
            return;
        };
        let _ = queue.push_mono_latest(&[1, 2, 3]);
        let queue = Mutex::new(queue);
        let queue_len = AtomicUsize::new(3);
        let gate = AtomicU8::new(GATE_ACTIVE);
        let volume = AtomicU8::new(volume.percent());
        let counters = ProducerCounters::default();
        let (error_sender, _errors) = mpsc::sync_channel(ERROR_QUEUE_CAPACITY);
        let controls = test_controls(&error_sender, &queue, &queue_len, &gate, &volume, &counters);

        assert_eq!(clear_queue_for_release(ReleaseReason::Muted, controls), 0);
        assert_eq!(queue_len.load(Ordering::Acquire), 3);
        assert_eq!(
            clear_queue_for_release(ReleaseReason::Shutdown, controls),
            3
        );
        assert_eq!(queue_len.load(Ordering::Acquire), 0);
    }
}
