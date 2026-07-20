//! Safe models and boundaries for statically linked libretro cores.

mod input;
mod keyboard;
mod options;
mod profile;

pub use input::{JOYPAD_MASK_ID, JoypadButton, JoypadState};
pub use keyboard::{joypad_from_keyboard, medium_raw_key_for_retro};
pub use options::CoreOption;
pub use profile::{ControllerDevice, LibretroCore, MAXIMUM_ROM_BYTES, MemoryFile, MemoryKind};
