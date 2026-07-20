//! Static system contracts kept independent of the libretro C ABI.

/// Largest ROM accepted by any libretro host.
pub const MAXIMUM_ROM_BYTES: usize = 8 * 1_024 * 1_024;

const NES_EXTENSIONS: [&str; 1] = ["nes"];
const GAME_BOY_EXTENSIONS: [&str; 2] = ["gb", "gbc"];
const ZX_EXTENSIONS: [&str; 1] = ["tap"];

const NES_MEMORY: [MemoryFile; 1] = [MemoryFile::new(MemoryKind::SaveRam, ".srm")];
const GAME_BOY_MEMORY: [MemoryFile; 2] = [
    MemoryFile::new(MemoryKind::SaveRam, ".sav"),
    MemoryFile::new(MemoryKind::Rtc, ".rtc"),
];
const ZX_MEMORY: [MemoryFile; 0] = [];

const NES_CONTROLLERS: [ControllerDevice; 2] = [ControllerDevice::Joypad, ControllerDevice::Joypad];
const GAME_BOY_CONTROLLERS: [ControllerDevice; 1] = [ControllerDevice::Joypad];
const ZX_CONTROLLERS: [ControllerDevice; 2] = [
    ControllerDevice::JoypadSubclass(1),
    ControllerDevice::JoypadSubclass(3),
];

/// One pinned libretro core and the system contract around it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LibretroCore {
    /// `FCEUmm` for Nintendo Entertainment System ROMs.
    Fceumm,
    /// Gambatte for original and Color Game Boy ROMs.
    Gambatte,
    /// Fuse for ZX Spectrum tape images.
    Fuse,
}

impl LibretroCore {
    /// Installed frontend executable name.
    #[must_use]
    pub const fn frontend_name(self) -> &'static str {
        match self {
            Self::Fceumm => "nes-deck",
            Self::Gambatte => "gb-deck",
            Self::Fuse => "zx-deck",
        }
    }

    /// Human-readable system name used in diagnostics.
    #[must_use]
    pub const fn system_name(self) -> &'static str {
        match self {
            Self::Fceumm => "NES",
            Self::Gambatte => "Game Boy",
            Self::Fuse => "ZX Spectrum",
        }
    }

    /// Expected upstream core name used when core metadata is absent.
    #[must_use]
    pub const fn core_name(self) -> &'static str {
        match self {
            Self::Fceumm => "FCEUmm",
            Self::Gambatte => "Gambatte",
            Self::Fuse => "Fuse",
        }
    }

    /// Accepted lowercase filename extensions without a leading dot.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Fceumm => &NES_EXTENSIONS,
            Self::Gambatte => &GAME_BOY_EXTENSIONS,
            Self::Fuse => &ZX_EXTENSIONS,
        }
    }

    /// Smallest structurally possible content image for this core.
    #[must_use]
    pub const fn minimum_rom_bytes(self) -> usize {
        match self {
            Self::Fceumm => 16,
            Self::Gambatte => 0x150,
            Self::Fuse => 4,
        }
    }

    /// Persistent core memories written beside the ROM.
    #[must_use]
    pub const fn memory_files(self) -> &'static [MemoryFile] {
        match self {
            Self::Fceumm => &NES_MEMORY,
            Self::Gambatte => &GAME_BOY_MEMORY,
            Self::Fuse => &ZX_MEMORY,
        }
    }

    /// Controller device assigned to each libretro port in player order.
    #[must_use]
    pub const fn controller_ports(self) -> &'static [ControllerDevice] {
        match self {
            Self::Fceumm => &NES_CONTROLLERS,
            Self::Gambatte => &GAME_BOY_CONTROLLERS,
            Self::Fuse => &ZX_CONTROLLERS,
        }
    }
}

/// One libretro persistent-memory region.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemoryKind {
    /// Battery-backed cartridge or machine RAM.
    SaveRam,
    /// Real-time clock state used by supported Game Boy cartridges.
    Rtc,
}

/// Filesystem representation of one persistent-memory region.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MemoryFile {
    kind: MemoryKind,
    extension: &'static str,
}

impl MemoryFile {
    const fn new(kind: MemoryKind, extension: &'static str) -> Self {
        Self { kind, extension }
    }

    /// Libretro memory region represented by this file.
    #[must_use]
    pub const fn kind(self) -> MemoryKind {
        self.kind
    }

    /// Filename extension including its leading dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        self.extension
    }
}

/// Libretro input device assigned to one player port.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ControllerDevice {
    /// Standard libretro joypad.
    Joypad,
    /// Core-defined joypad subclass with the given identifier.
    JoypadSubclass(u8),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_profiles_are_complete_and_distinct() {
        let profiles = [
            (
                LibretroCore::Fceumm,
                "nes-deck",
                "NES",
                "FCEUmm",
                &NES_EXTENSIONS[..],
                16,
            ),
            (
                LibretroCore::Gambatte,
                "gb-deck",
                "Game Boy",
                "Gambatte",
                &GAME_BOY_EXTENSIONS[..],
                0x150,
            ),
            (
                LibretroCore::Fuse,
                "zx-deck",
                "ZX Spectrum",
                "Fuse",
                &ZX_EXTENSIONS[..],
                4,
            ),
        ];

        for (core, frontend, system, name, extensions, minimum) in profiles {
            assert_eq!(core.frontend_name(), frontend);
            assert_eq!(core.system_name(), system);
            assert_eq!(core.core_name(), name);
            assert_eq!(core.extensions(), extensions);
            assert_eq!(core.minimum_rom_bytes(), minimum);
            assert!(minimum < MAXIMUM_ROM_BYTES);
        }
        assert_ne!(
            LibretroCore::Fceumm.frontend_name(),
            LibretroCore::Gambatte.frontend_name()
        );
        assert_ne!(
            LibretroCore::Gambatte.frontend_name(),
            LibretroCore::Fuse.frontend_name()
        );
    }

    #[test]
    fn persistent_memory_is_simple_and_core_native() {
        assert_eq!(
            LibretroCore::Fceumm.memory_files(),
            &[MemoryFile::new(MemoryKind::SaveRam, ".srm")]
        );
        assert_eq!(
            LibretroCore::Gambatte.memory_files(),
            &[
                MemoryFile::new(MemoryKind::SaveRam, ".sav"),
                MemoryFile::new(MemoryKind::Rtc, ".rtc")
            ]
        );
        assert!(LibretroCore::Fuse.memory_files().is_empty());
    }

    #[test]
    fn controller_topology_matches_each_machine() {
        assert_eq!(
            LibretroCore::Fceumm.controller_ports(),
            &[ControllerDevice::Joypad, ControllerDevice::Joypad]
        );
        assert_eq!(
            LibretroCore::Gambatte.controller_ports(),
            &[ControllerDevice::Joypad]
        );
        assert_eq!(
            LibretroCore::Fuse.controller_ports(),
            &[
                ControllerDevice::JoypadSubclass(1),
                ControllerDevice::JoypadSubclass(3)
            ]
        );
    }
}
