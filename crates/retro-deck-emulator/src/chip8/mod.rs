//! CHIP-8 compatibility configuration, input mapping, and c-octo ownership.

mod config;
mod input;

pub use config::{ConfigError, Configuration, CoreOptions, InputProfile, Quirk, Quirks};
pub use input::{Controller, ControllerButton, ControllerState, KeypadState};
