//! Saturation-safe monotonic time for native application runtimes.

use std::time::{Duration, Instant};

use rustix::event::Timespec;

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const MAXIMUM_FRAME_RATE: u32 = 1_000;
const LATE_FRAME_RESET_PERIODS: u32 = 5;

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

/// Validated fixed presentation rate.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FrameRate(u32);

impl FrameRate {
    /// Construct a rate from 1 through 1000 frames per second.
    #[must_use]
    pub const fn new(frames_per_second: u32) -> Option<Self> {
        if frames_per_second == 0 || frames_per_second > MAXIMUM_FRAME_RATE {
            None
        } else {
            Some(Self(frames_per_second))
        }
    }

    /// Frames scheduled per second.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    const fn period(self) -> Duration {
        Duration::from_nanos(NANOSECONDS_PER_SECOND / self.0 as u64)
    }
}

/// Drift-free frame deadline clock for integration with descriptor polling.
///
/// The first frame is due immediately. After each completed frame, deadlines
/// advance from the prior absolute schedule rather than from the completion
/// time. A runtime delayed by more than five periods drops the stale schedule
/// and resumes one period after the current time.
#[derive(Clone, Copy, Debug)]
pub struct FrameClock {
    origin: Instant,
    schedule: FrameSchedule,
}

impl FrameClock {
    /// Start a new fixed-rate schedule with its first frame due now.
    #[must_use]
    pub fn start(rate: FrameRate) -> Self {
        Self::with_period(rate.period())
    }

    /// Start a schedule from an exact nonzero frame period.
    ///
    /// This preserves fractional emulator rates whose reciprocal cannot be
    /// represented by an integer [`FrameRate`].
    #[must_use]
    pub fn start_period(period: Duration) -> Option<Self> {
        if period.is_zero() {
            None
        } else {
            Some(Self::with_period(period))
        }
    }

    fn with_period(period: Duration) -> Self {
        Self {
            origin: Instant::now(),
            schedule: FrameSchedule::new(period),
        }
    }

    /// Remaining duration suitable for a poll timeout.
    #[must_use]
    pub fn wait_duration(&self) -> Duration {
        self.schedule.wait_duration(self.origin.elapsed())
    }

    /// Advance the absolute deadline after executing one emulated frame.
    pub fn complete_frame(&mut self) {
        self.schedule.complete_frame(self.origin.elapsed());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FrameSchedule {
    period: Duration,
    deadline: Duration,
}

impl FrameSchedule {
    const fn new(period: Duration) -> Self {
        Self {
            period,
            deadline: Duration::ZERO,
        }
    }

    const fn wait_duration(self, now: Duration) -> Duration {
        self.deadline.saturating_sub(now)
    }

    fn complete_frame(&mut self, now: Duration) {
        let next = self.deadline.saturating_add(self.period);
        let maximum_lag = self.period.saturating_mul(LATE_FRAME_RESET_PERIODS);
        self.deadline = if now.saturating_sub(next) > maximum_lag {
            now.saturating_add(self.period)
        } else {
            next
        };
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

    #[test]
    fn frame_rates_are_bounded_and_have_an_exact_integer_period() {
        assert_eq!(FrameRate::new(0), None);
        assert_eq!(FrameRate::new(MAXIMUM_FRAME_RATE + 1), None);
        let Some(rate) = FrameRate::new(60) else {
            return;
        };
        assert_eq!(rate.get(), 60);
        assert_eq!(rate.period(), Duration::from_nanos(16_666_666));
    }

    #[test]
    fn frame_schedule_is_immediate_then_advances_without_drift() {
        let period = Duration::from_nanos(16_666_666);
        let processing = Duration::from_millis(2);
        let mut schedule = FrameSchedule::new(period);
        assert_eq!(schedule.wait_duration(Duration::ZERO), Duration::ZERO);

        schedule.complete_frame(processing);
        assert_eq!(schedule.wait_duration(processing), period - processing);

        schedule.complete_frame(period + processing);
        assert_eq!(
            schedule.wait_duration(period + processing),
            period - processing
        );
        assert_eq!(schedule.deadline, period.saturating_mul(2));
    }

    #[test]
    fn exact_period_clock_rejects_zero_without_rounding() {
        assert!(FrameClock::start_period(Duration::ZERO).is_none());
        let period = Duration::from_nanos(16_639_274);
        let clock = FrameClock::start_period(period).expect("nonzero exact period");
        assert_eq!(clock.schedule.period, period);
        assert_eq!(clock.schedule.deadline, Duration::ZERO);
    }

    #[test]
    fn badly_late_frame_discards_only_the_stale_schedule() {
        let period = Duration::from_millis(10);
        let mut schedule = FrameSchedule::new(period);
        schedule.complete_frame(Duration::ZERO);

        let delayed = period.saturating_mul(8);
        schedule.complete_frame(delayed);
        assert_eq!(schedule.wait_duration(delayed), period);
    }
}
