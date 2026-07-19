//! Saturation-safe monotonic time for native application runtimes.

use std::time::{Duration, Instant};

use rustix::event::Timespec;

/// Process-local monotonic nanosecond clock with a stable zero point.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// Start a new process-local time domain at zero.
    #[must_use]
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Return elapsed monotonic nanoseconds, saturating at `u64::MAX`.
    #[must_use]
    pub fn nanoseconds(self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::start()
    }
}

pub(crate) fn duration_timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(duration.subsec_nanos()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_conversion_is_exact_and_saturating() {
        assert_eq!(
            duration_timespec(Duration::from_millis(125)),
            Timespec {
                tv_sec: 0,
                tv_nsec: 125_000_000,
            }
        );
        assert_eq!(
            duration_timespec(Duration::MAX).tv_sec,
            i64::try_from(Duration::MAX.as_secs()).unwrap_or(i64::MAX)
        );
    }

    #[test]
    fn process_clock_never_moves_backwards() {
        let clock = MonotonicClock::start();
        let before = clock.nanoseconds();
        let after = clock.nanoseconds();
        assert!(after >= before);
    }
}
