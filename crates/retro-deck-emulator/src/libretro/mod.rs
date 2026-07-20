//! Safe models and boundaries for statically linked libretro cores.

mod abi;
mod audio;
mod callbacks;
mod content;
mod environment;
mod input;
mod keyboard;
mod options;
mod profile;
mod save;
mod session;
mod video;

pub use audio::AudioBatchError;
pub use content::{Content, ContentError};
pub use environment::PixelFormat;
pub use input::{JOYPAD_MASK_ID, JoypadButton, JoypadState};
pub use keyboard::{joypad_from_keyboard, medium_raw_key_for_retro};
pub use options::CoreOption;
pub use profile::{ControllerDevice, LibretroCore, MAXIMUM_ROM_BYTES, MemoryFile, MemoryKind};
pub use save::{LoadOutcome, MAXIMUM_SAVE_BYTES, SaveError, SaveOutcome, SaveStore};
pub use video::{VideoCallbackError, VideoFrameError};
