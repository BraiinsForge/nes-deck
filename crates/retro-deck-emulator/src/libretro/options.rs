//! Intentional fixed options for cores that require frontend policy.

use super::LibretroCore;

const FCEUMM_OPTIONS: &[CoreOption] = &[
    CoreOption::new("fceumm_region", "Auto"),
    CoreOption::new("fceumm_overscan_h_left", "0"),
    CoreOption::new("fceumm_overscan_h_right", "0"),
    CoreOption::new("fceumm_overscan_v_top", "0"),
    CoreOption::new("fceumm_overscan_v_bottom", "0"),
];

const FUSE_OPTIONS: &[CoreOption] = &[
    CoreOption::new("fuse_machine", "Spectrum 48K"),
    CoreOption::new("fuse_emulation_speed", "100"),
    CoreOption::new("fuse_size_border", "medium"),
    CoreOption::new("fuse_palette", "Fuse Standard"),
    CoreOption::new("fuse_auto_load", "enabled"),
    CoreOption::new("fuse_fast_load", "enabled"),
    CoreOption::new("fuse_load_sound", "disabled"),
    CoreOption::new("fuse_speaker_type", "tv speaker"),
    CoreOption::new("fuse_ay_stereo_separation", "none"),
    CoreOption::new("fuse_key_ovrlay_transp", "enabled"),
    CoreOption::new("fuse_key_hold_time", "500"),
    CoreOption::new("fuse_display_joystick_type", "disabled"),
    CoreOption::new("fuse_auto_size_savestate", "enabled"),
    CoreOption::new("fuse_joypad_left", "<none>"),
    CoreOption::new("fuse_joypad_right", "<none>"),
    CoreOption::new("fuse_joypad_up", "<none>"),
    CoreOption::new("fuse_joypad_down", "<none>"),
    CoreOption::new("fuse_joypad_start", "<none>"),
    CoreOption::new("fuse_joypad_a", "<none>"),
    CoreOption::new("fuse_joypad_b", "<none>"),
    CoreOption::new("fuse_joypad_x", "<none>"),
    CoreOption::new("fuse_joypad_y", "<none>"),
    CoreOption::new("fuse_joypad_l", "<none>"),
    CoreOption::new("fuse_joypad_r", "<none>"),
    CoreOption::new("fuse_joypad_l2", "<none>"),
    CoreOption::new("fuse_joypad_r2", "<none>"),
    CoreOption::new("fuse_joypad_l3", "<none>"),
    CoreOption::new("fuse_joypad_r3", "<none>"),
];

/// One exact key and selected value exposed through the libretro environment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CoreOption {
    key: &'static str,
    value: &'static str,
}

impl CoreOption {
    const fn new(key: &'static str, value: &'static str) -> Self {
        Self { key, value }
    }

    /// Core-defined option key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        self.key
    }

    /// Fixed value selected by Retro Deck.
    #[must_use]
    pub const fn value(self) -> &'static str {
        self.value
    }
}

impl LibretroCore {
    /// Fixed options selected for this core.
    #[must_use]
    pub const fn options(self) -> &'static [CoreOption] {
        match self {
            Self::Fceumm => FCEUMM_OPTIONS,
            Self::Gambatte => &[],
            Self::Fuse => FUSE_OPTIONS,
        }
    }

    /// Look up one exact core option requested through the environment.
    #[must_use]
    pub fn option(self, key: &str) -> Option<&'static str> {
        self.options()
            .iter()
            .find(|option| option.key() == key)
            .map(|option| option.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn fceumm_uses_the_complete_uncropped_frame() {
        assert_eq!(LibretroCore::Fceumm.option("fceumm_region"), Some("Auto"));
        for edge in [
            "fceumm_overscan_h_left",
            "fceumm_overscan_h_right",
            "fceumm_overscan_v_top",
            "fceumm_overscan_v_bottom",
        ] {
            assert_eq!(LibretroCore::Fceumm.option(edge), Some("0"));
        }
        assert_eq!(LibretroCore::Fceumm.options().len(), 5);
    }

    #[test]
    fn gambatte_needs_no_frontend_overrides() {
        assert!(LibretroCore::Gambatte.options().is_empty());
        assert_eq!(
            LibretroCore::Gambatte.option("gambatte_gb_colorization"),
            None
        );
    }

    #[test]
    fn fuse_loads_tapes_without_duplicate_button_keys() {
        assert_eq!(
            LibretroCore::Fuse.option("fuse_machine"),
            Some("Spectrum 48K")
        );
        assert_eq!(LibretroCore::Fuse.option("fuse_auto_load"), Some("enabled"));
        assert_eq!(LibretroCore::Fuse.option("fuse_fast_load"), Some("enabled"));
        assert_eq!(
            LibretroCore::Fuse.option("fuse_load_sound"),
            Some("disabled")
        );
        assert_eq!(LibretroCore::Fuse.option("fuse_joypad_a"), Some("<none>"));
        assert_eq!(LibretroCore::Fuse.option("fuse_joypad_b"), Some("<none>"));
        assert_eq!(
            LibretroCore::Fuse.option("fuse_joypad_start"),
            Some("<none>")
        );
        assert_eq!(LibretroCore::Fuse.options().len(), 28);
    }

    #[test]
    fn option_tables_have_unique_nonempty_c_strings() {
        for core in [
            LibretroCore::Fceumm,
            LibretroCore::Gambatte,
            LibretroCore::Fuse,
        ] {
            let mut keys = HashSet::new();
            for option in core.options() {
                assert!(!option.key().is_empty());
                assert!(!option.value().is_empty());
                assert!(!option.key().contains('\0'));
                assert!(!option.value().contains('\0'));
                assert!(keys.insert(option.key()));
            }
        }
    }

    #[test]
    fn option_lookup_is_exact() {
        assert_eq!(LibretroCore::Fuse.option("fuse_machine\0"), None);
        assert_eq!(LibretroCore::Fuse.option("FUSE_MACHINE"), None);
        assert_eq!(LibretroCore::Fuse.option("fuse_unknown"), None);
    }
}
