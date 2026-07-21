//! Thin clients for audio playback owned by the BMC compositor.

mod application_pcm;
mod square_pcm;
mod tone_cues;

pub use application_pcm::{
    ApplicationPcm, ApplicationPcmError, ApplicationPcmStartError, ApplicationPcmStats,
};
pub use square_pcm::{SquarePcm, SquareStream};
pub use tone_cues::{ToneCuePlayer, ToneCueStartError};

/// Reason application audio is allowed or suppressed.
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
