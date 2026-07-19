//! CHIP-8 compatibility configuration, input mapping, and c-octo ownership.

mod config;
mod core;
mod input;
mod presentation;
mod program;

pub use config::{
    ConfigError, Configuration, CoreOptions, InputProfile, MAXIMUM_CONFIG_BYTES, Quirk, Quirks,
};
pub use core::{Core, CoreError, CoreFrame, FrameError, FrameOutcome, MAXIMUM_ROM_BYTES};
pub use input::{Controller, ControllerButton, ControllerState, KeypadState};
pub use presentation::{
    NORMALIZED_FRAME_HEIGHT, NORMALIZED_FRAME_PIXELS, NORMALIZED_FRAME_WIDTH, NormalizedFrame,
};
pub use program::{Program, ProgramError};
