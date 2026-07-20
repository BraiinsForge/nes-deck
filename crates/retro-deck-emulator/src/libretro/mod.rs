//! Safe models and boundaries for statically linked libretro cores.

mod options;
mod profile;

pub use options::CoreOption;
pub use profile::{ControllerDevice, LibretroCore, MAXIMUM_ROM_BYTES, MemoryFile, MemoryKind};
