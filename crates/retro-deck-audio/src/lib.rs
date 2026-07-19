//! Audio device ownership policy shared by Retro Deck runtimes.
//!
//! This crate deliberately knows nothing about OSS, ALSA, file descriptors,
//! sample formats, or threads. A platform backend performs the returned
//! [`AudioAction`] and reports the result. Keeping this state machine pure
//! makes the rule "release audio when it is not needed" testable without
//! hardware.

mod cue;

pub use cue::{CueEnqueue, CueReceive, CueReceiver, CueSender, cue_channel};

use std::time::Duration;

/// Monotonic milliseconds supplied by the platform runtime.
pub type MonotonicMilliseconds = u64;

/// Observable ownership state of the audio device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioState {
    /// No device handle is owned.
    Closed,
    /// An open was requested and the backend is configuring and priming it.
    Priming,
    /// The source is actively producing a stream.
    Active,
    /// The source stopped and already queued samples are finishing.
    Draining,
    /// The queue is empty, but a short grace period avoids immediate reopen.
    Idle,
}

/// Side effect the platform backend must perform after a state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioAction {
    /// No device operation is required.
    None,
    /// Open, configure, and prime the device, then call
    /// [`AudioLifecycle::opened`] with the result.
    OpenDevice,
    /// Close the device and discard backend buffers.
    CloseDevice,
}

/// Reason an active source no longer needs audio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseReason {
    /// A finite sound ended normally and queued samples should drain.
    Finished,
    /// The user muted audio. Release immediately.
    Muted,
    /// Playback was paused. Release immediately.
    Paused,
    /// The application is no longer visible or active. Release immediately.
    Hidden,
    /// The process is shutting down. Release immediately.
    Shutdown,
}

impl ReleaseReason {
    const fn drains(self) -> bool {
        matches!(self, Self::Finished)
    }
}

/// Pure state machine controlling one process's audio device lease.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioLifecycle {
    state: AudioState,
    idle_timeout_ms: MonotonicMilliseconds,
    close_at_ms: Option<MonotonicMilliseconds>,
}

impl AudioLifecycle {
    /// Create a closed lifecycle with the supplied post-drain grace period.
    ///
    /// The grace period should be long enough to absorb closely spaced short
    /// cues, but short enough that another process can acquire the device
    /// promptly. A zero duration closes as soon as [`Self::tick`] observes the
    /// idle state.
    #[must_use]
    pub fn new(idle_timeout: Duration) -> Self {
        let idle_timeout_ms = u64::try_from(idle_timeout.as_millis()).unwrap_or(u64::MAX);
        Self {
            state: AudioState::Closed,
            idle_timeout_ms,
            close_at_ms: None,
        }
    }

    /// Return the current ownership state.
    #[must_use]
    pub const fn state(&self) -> AudioState {
        self.state
    }

    /// Return whether the backend should currently own an audio device.
    #[must_use]
    pub const fn owns_device(&self) -> bool {
        !matches!(self.state, AudioState::Closed)
    }

    /// Request active playback.
    ///
    /// Closed playback requires a device open. An idle or draining device can
    /// be reused immediately. Repeated requests while active or priming are
    /// idempotent.
    pub const fn request_playback(&mut self) -> AudioAction {
        self.close_at_ms = None;
        match self.state {
            AudioState::Closed => {
                self.state = AudioState::Priming;
                AudioAction::OpenDevice
            }
            AudioState::Priming | AudioState::Active => AudioAction::None,
            AudioState::Draining | AudioState::Idle => {
                self.state = AudioState::Active;
                AudioAction::None
            }
        }
    }

    /// Report completion of a requested device open.
    ///
    /// A failed open returns to the closed state. Late backend reports after a
    /// forced release are ignored.
    pub const fn opened(&mut self, succeeded: bool) {
        if !matches!(self.state, AudioState::Priming) {
            return;
        }
        self.state = if succeeded {
            AudioState::Active
        } else {
            AudioState::Closed
        };
    }

    /// Release active playback for the supplied reason.
    ///
    /// Normal completion drains already queued samples. Muting, pausing,
    /// hiding, and shutdown close immediately because the application no
    /// longer has a valid reason to retain the device.
    pub const fn release(&mut self, reason: ReleaseReason) -> AudioAction {
        self.close_at_ms = None;
        if matches!(self.state, AudioState::Closed) {
            return AudioAction::None;
        }
        if reason.drains() && matches!(self.state, AudioState::Active) {
            self.state = AudioState::Draining;
            AudioAction::None
        } else {
            self.state = AudioState::Closed;
            AudioAction::CloseDevice
        }
    }

    /// Report that all queued samples have drained.
    ///
    /// The device remains open for the configured grace period. A backend
    /// should call [`Self::tick`] from its ordinary event loop to perform the
    /// eventual close.
    pub const fn drained(&mut self, now_ms: MonotonicMilliseconds) {
        if !matches!(self.state, AudioState::Draining) {
            return;
        }
        self.state = AudioState::Idle;
        self.close_at_ms = Some(now_ms.saturating_add(self.idle_timeout_ms));
    }

    /// Advance idle timeout handling at a monotonic instant.
    pub const fn tick(&mut self, now_ms: MonotonicMilliseconds) -> AudioAction {
        let Some(close_at_ms) = self.close_at_ms else {
            return AudioAction::None;
        };
        if !matches!(self.state, AudioState::Idle) || now_ms < close_at_ms {
            return AudioAction::None;
        }
        self.state = AudioState::Closed;
        self.close_at_ms = None;
        AudioAction::CloseDevice
    }

    /// Report a backend failure and require disposal of any partial device.
    pub const fn failed(&mut self) -> AudioAction {
        self.close_at_ms = None;
        if matches!(self.state, AudioState::Closed) {
            AudioAction::None
        } else {
            self.state = AudioState::Closed;
            AudioAction::CloseDevice
        }
    }
}

impl Default for AudioLifecycle {
    fn default() -> Self {
        Self::new(Duration::from_millis(250))
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioAction, AudioLifecycle, AudioState, ReleaseReason};
    use std::time::Duration;

    #[test]
    fn finite_playback_opens_drains_and_closes_after_grace() {
        let mut lifecycle = AudioLifecycle::new(Duration::from_millis(250));

        assert_eq!(lifecycle.request_playback(), AudioAction::OpenDevice);
        assert_eq!(lifecycle.state(), AudioState::Priming);
        lifecycle.opened(true);
        assert_eq!(lifecycle.state(), AudioState::Active);

        assert_eq!(
            lifecycle.release(ReleaseReason::Finished),
            AudioAction::None
        );
        assert_eq!(lifecycle.state(), AudioState::Draining);
        lifecycle.drained(1_000);
        assert_eq!(lifecycle.state(), AudioState::Idle);
        assert_eq!(lifecycle.tick(1_249), AudioAction::None);
        assert_eq!(lifecycle.tick(1_250), AudioAction::CloseDevice);
        assert_eq!(lifecycle.state(), AudioState::Closed);
    }

    #[test]
    fn new_playback_reuses_draining_or_idle_device() {
        for resume_state in [AudioState::Draining, AudioState::Idle] {
            let mut lifecycle = AudioLifecycle::default();
            assert_eq!(lifecycle.request_playback(), AudioAction::OpenDevice);
            lifecycle.opened(true);
            assert_eq!(
                lifecycle.release(ReleaseReason::Finished),
                AudioAction::None
            );
            if matches!(resume_state, AudioState::Idle) {
                lifecycle.drained(500);
            }
            assert_eq!(lifecycle.state(), resume_state);

            assert_eq!(lifecycle.request_playback(), AudioAction::None);
            assert_eq!(lifecycle.state(), AudioState::Active);
            assert_eq!(lifecycle.tick(u64::MAX), AudioAction::None);
        }
    }

    #[test]
    fn inactive_application_releases_immediately() {
        for reason in [
            ReleaseReason::Muted,
            ReleaseReason::Paused,
            ReleaseReason::Hidden,
            ReleaseReason::Shutdown,
        ] {
            let mut lifecycle = AudioLifecycle::default();
            assert_eq!(lifecycle.request_playback(), AudioAction::OpenDevice);
            lifecycle.opened(true);

            assert_eq!(lifecycle.release(reason), AudioAction::CloseDevice);
            assert_eq!(lifecycle.state(), AudioState::Closed);
            assert!(!lifecycle.owns_device());
        }
    }

    #[test]
    fn failed_or_cancelled_open_never_leaks_ownership() {
        let mut failed = AudioLifecycle::default();
        assert_eq!(failed.request_playback(), AudioAction::OpenDevice);
        failed.opened(false);
        assert_eq!(failed.state(), AudioState::Closed);
        assert!(!failed.owns_device());

        let mut cancelled = AudioLifecycle::default();
        assert_eq!(cancelled.request_playback(), AudioAction::OpenDevice);
        assert_eq!(
            cancelled.release(ReleaseReason::Shutdown),
            AudioAction::CloseDevice
        );
        cancelled.opened(true);
        assert_eq!(cancelled.state(), AudioState::Closed);
        assert!(!cancelled.owns_device());
    }

    #[test]
    fn backend_failure_closes_every_owned_state() {
        for state in [
            AudioState::Priming,
            AudioState::Active,
            AudioState::Draining,
            AudioState::Idle,
        ] {
            let mut lifecycle = AudioLifecycle::default();
            assert_eq!(lifecycle.request_playback(), AudioAction::OpenDevice);
            if !matches!(state, AudioState::Priming) {
                lifecycle.opened(true);
            }
            if matches!(state, AudioState::Draining | AudioState::Idle) {
                assert_eq!(
                    lifecycle.release(ReleaseReason::Finished),
                    AudioAction::None
                );
            }
            if matches!(state, AudioState::Idle) {
                lifecycle.drained(10);
            }
            assert_eq!(lifecycle.state(), state);

            assert_eq!(lifecycle.failed(), AudioAction::CloseDevice);
            assert_eq!(lifecycle.state(), AudioState::Closed);
        }
    }

    #[test]
    fn monotonic_deadline_saturates_without_early_close() {
        let mut lifecycle = AudioLifecycle::new(Duration::MAX);
        assert_eq!(lifecycle.request_playback(), AudioAction::OpenDevice);
        lifecycle.opened(true);
        assert_eq!(
            lifecycle.release(ReleaseReason::Finished),
            AudioAction::None
        );
        lifecycle.drained(u64::MAX - 1);

        assert_eq!(lifecycle.tick(u64::MAX - 1), AudioAction::None);
        assert_eq!(lifecycle.tick(u64::MAX), AudioAction::CloseDevice);
    }
}
