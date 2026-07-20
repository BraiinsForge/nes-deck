//! Validated values shared by PCM producers and audio backends.

const MAXIMUM_SAMPLE_RATE: u32 = 192_000;

/// Positive PCM sample rate bounded for an appliance audio stream.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SampleRate(u32);

impl SampleRate {
    /// Validate a rate from 1 through 192 kHz.
    #[must_use]
    pub const fn new(hertz: u32) -> Option<Self> {
        if hertz == 0 || hertz > MAXIMUM_SAMPLE_RATE {
            None
        } else {
            Some(Self(hertz))
        }
    }

    /// Return samples per second.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// User volume percentage from muted through full scale.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Volume(u8);

impl Volume {
    /// Muted output.
    pub const MUTED: Self = Self(0);

    /// Validate a percentage from 0 through 100.
    #[must_use]
    pub const fn new(percent: u8) -> Option<Self> {
        if percent <= 100 {
            Some(Self(percent))
        } else {
            None
        }
    }

    /// Return the validated percentage.
    #[must_use]
    pub const fn percent(self) -> u8 {
        self.0
    }

    /// Whether playback should avoid opening an audio device.
    #[must_use]
    pub const fn muted(self) -> bool {
        self.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{SampleRate, Volume};

    #[test]
    fn sample_rates_are_positive_and_bounded() {
        assert_eq!(SampleRate::new(0), None);
        assert_eq!(SampleRate::new(44_100).map(SampleRate::get), Some(44_100));
        assert_eq!(SampleRate::new(192_001), None);
    }

    #[test]
    fn volume_includes_mute_and_full_scale() {
        assert_eq!(Volume::new(0), Some(Volume::MUTED));
        assert_eq!(Volume::new(100).map(Volume::percent), Some(100));
        assert_eq!(Volume::new(101), None);
    }
}
