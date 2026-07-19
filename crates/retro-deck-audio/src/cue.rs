//! Bounded, nonblocking delivery of short user-interface sound cues.

use std::{
    num::NonZeroUsize,
    sync::mpsc::{
        self, Receiver, RecvError, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
    },
    time::Duration,
};

/// Result of trying to enqueue one sound cue from an input path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CueEnqueue {
    /// The audio worker can consume the cue.
    Queued,
    /// The bounded queue was full, so the new cue was discarded.
    DroppedFull,
    /// The audio worker was gone, so the cue was discarded.
    DroppedDisconnected,
}

/// Result of draining currently queued cues without waiting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CueReceive<T> {
    /// No cue is currently waiting.
    Empty,
    /// The newest cue was returned and older queued cues were discarded.
    Latest(T),
    /// Every sender has gone and no queued cue remains.
    Disconnected,
}

/// Input-side handle for a bounded audio cue queue.
///
/// This type intentionally exposes no blocking send operation. Input handling
/// calls [`Self::try_enqueue`] and continues immediately even when audio is
/// busy, its queue is full, or its worker has failed.
#[derive(Clone, Debug)]
pub struct CueSender<T> {
    sender: SyncSender<T>,
}

impl<T> CueSender<T> {
    /// Try to enqueue `cue` without waiting for audio work.
    ///
    /// Full and disconnected queues discard the cue by design. Sound feedback
    /// is optional; preserving current touch and controller input is not.
    pub fn try_enqueue(&self, cue: T) -> CueEnqueue {
        match self.sender.try_send(cue) {
            Ok(()) => CueEnqueue::Queued,
            Err(TrySendError::Full(_)) => CueEnqueue::DroppedFull,
            Err(TrySendError::Disconnected(_)) => CueEnqueue::DroppedDisconnected,
        }
    }
}

/// Audio-worker handle for a bounded cue queue.
#[derive(Debug)]
pub struct CueReceiver<T> {
    receiver: Receiver<T>,
}

impl<T> CueReceiver<T> {
    /// Wait for one cue, then coalesce any others already queued.
    ///
    /// Only the dedicated audio worker should call this method. Input-side
    /// handles expose no blocking operation.
    #[must_use]
    pub fn wait_latest(&self) -> CueReceive<T> {
        match self.receiver.recv() {
            Ok(cue) => self.coalesce(cue),
            Err(RecvError) => CueReceive::Disconnected,
        }
    }

    /// Wait up to `timeout` for one cue, then coalesce queued successors.
    ///
    /// [`CueReceive::Empty`] means the timeout expired. This is intended for
    /// audio-worker idle deadlines and never runs on an input path.
    #[must_use]
    pub fn wait_latest_timeout(&self, timeout: Duration) -> CueReceive<T> {
        match self.receiver.recv_timeout(timeout) {
            Ok(cue) => self.coalesce(cue),
            Err(RecvTimeoutError::Timeout) => CueReceive::Empty,
            Err(RecvTimeoutError::Disconnected) => CueReceive::Disconnected,
        }
    }

    /// Drain all cues already waiting and return only the newest one.
    ///
    /// Coalescing prevents delayed navigation sounds from playing after the
    /// corresponding interaction has already finished. This method never
    /// waits for a producer.
    #[must_use]
    pub fn try_latest(&self) -> CueReceive<T> {
        let latest = match self.receiver.try_recv() {
            Ok(cue) => cue,
            Err(TryRecvError::Empty) => return CueReceive::Empty,
            Err(TryRecvError::Disconnected) => return CueReceive::Disconnected,
        };

        self.coalesce(latest)
    }

    fn coalesce(&self, mut latest: T) -> CueReceive<T> {
        loop {
            match self.receiver.try_recv() {
                Ok(cue) => latest = cue,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                    return CueReceive::Latest(latest);
                }
            }
        }
    }
}

/// Construct a bounded channel for cheap cue identifiers.
///
/// The input event path owns the sender. A dedicated audio worker owns the
/// receiver, waveform lookup, device lifecycle, and sample writes.
#[must_use]
pub fn cue_channel<T>(capacity: NonZeroUsize) -> (CueSender<T>, CueReceiver<T>) {
    let (sender, receiver) = mpsc::sync_channel(capacity.get());
    (CueSender { sender }, CueReceiver { receiver })
}

#[cfg(test)]
mod tests {
    use super::{CueEnqueue, CueReceive, cue_channel};
    use std::num::NonZeroUsize;
    use std::time::Duration;

    fn capacity(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).unwrap_or(NonZeroUsize::MIN)
    }

    #[test]
    fn full_queue_drops_sound_without_displacing_input() {
        let (sender, receiver) = cue_channel(capacity(1));

        assert_eq!(sender.try_enqueue(10), CueEnqueue::Queued);
        assert_eq!(sender.try_enqueue(11), CueEnqueue::DroppedFull);
        assert_eq!(receiver.try_latest(), CueReceive::Latest(10));
    }

    #[test]
    fn worker_coalesces_stale_cues_to_the_newest() {
        let (sender, receiver) = cue_channel(capacity(3));
        assert_eq!(sender.try_enqueue("left"), CueEnqueue::Queued);
        assert_eq!(sender.try_enqueue("right"), CueEnqueue::Queued);
        assert_eq!(sender.try_enqueue("confirm"), CueEnqueue::Queued);

        assert_eq!(receiver.try_latest(), CueReceive::Latest("confirm"));
        assert_eq!(receiver.try_latest(), CueReceive::Empty);
    }

    #[test]
    fn missing_worker_drops_new_cues() {
        let (sender, receiver) = cue_channel::<u8>(capacity(1));
        drop(receiver);

        assert_eq!(sender.try_enqueue(1), CueEnqueue::DroppedDisconnected);
    }

    #[test]
    fn queued_cue_survives_the_last_sender() {
        let (sender, receiver) = cue_channel(capacity(1));
        assert_eq!(sender.try_enqueue(7), CueEnqueue::Queued);
        drop(sender);

        assert_eq!(receiver.try_latest(), CueReceive::Latest(7));
        assert_eq!(receiver.try_latest(), CueReceive::Disconnected);
    }

    #[test]
    fn worker_wait_coalesces_without_polling() {
        let (sender, receiver) = cue_channel(capacity(3));
        assert_eq!(sender.try_enqueue("old"), CueEnqueue::Queued);
        assert_eq!(sender.try_enqueue("new"), CueEnqueue::Queued);

        assert_eq!(receiver.wait_latest(), CueReceive::Latest("new"));
    }

    #[test]
    fn zero_timeout_and_disconnect_are_distinct() {
        let (sender, receiver) = cue_channel::<u8>(capacity(1));
        assert_eq!(
            receiver.wait_latest_timeout(Duration::ZERO),
            CueReceive::Empty
        );
        drop(sender);
        assert_eq!(
            receiver.wait_latest_timeout(Duration::ZERO),
            CueReceive::Disconnected
        );
    }
}
