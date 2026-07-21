//! Validated PCM values and bounded synthesis for short interface tones.
//!
//! Physical device ownership and playback belong to the BMC compositor. This
//! crate contains only small, deterministic data transformations shared by
//! Retro Deck applications.

mod format;
mod tone;

pub use format::{SampleRate, Volume};
pub use tone::{SquareTone, ToneError, ToneNote};
