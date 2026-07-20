//! Safe models and boundaries for statically linked libretro cores.

mod profile;

pub use profile::{ControllerDevice, LibretroCore, MAXIMUM_ROM_BYTES, MemoryFile, MemoryKind};
