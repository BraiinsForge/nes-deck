//! CHIP-8 compatibility configuration, input mapping, and c-octo ownership.

mod config;
mod core;
mod input;

pub use config::{ConfigError, Configuration, CoreOptions, InputProfile, Quirk, Quirks};
pub use core::{Core, CoreError, CoreFrame, FrameError, FrameOutcome};
pub use input::{Controller, ControllerButton, ControllerState, KeypadState};
