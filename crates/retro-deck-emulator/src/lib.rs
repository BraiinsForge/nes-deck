//! Safe emulator boundaries separated from device-specific runtime I/O.

#[cfg(feature = "chip8")]
pub mod chip8;
pub mod libretro;
