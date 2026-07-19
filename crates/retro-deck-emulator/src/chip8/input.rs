use super::InputProfile;

/// Stable controller slot used by a CHIP-8 input profile.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Controller {
    /// First controller in physical USB-path order.
    One,
    /// Second controller in physical USB-path order.
    Two,
}

/// Device-independent controller control understood by emulator profiles.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ControllerButton {
    /// Primary face action.
    A,
    /// Secondary face action.
    B,
    /// Back or Select.
    Select,
    /// Start.
    Start,
    /// Direction up.
    Up,
    /// Direction down.
    Down,
    /// Direction left.
    Left,
    /// Direction right.
    Right,
    /// Left shoulder.
    L,
    /// Right shoulder.
    R,
}

impl ControllerButton {
    const fn mask(self) -> u16 {
        match self {
            Self::A => 1 << 0,
            Self::B => 1 << 1,
            Self::Select => 1 << 2,
            Self::Start => 1 << 3,
            Self::Up => 1 << 4,
            Self::Down => 1 << 5,
            Self::Left => 1 << 6,
            Self::Right => 1 << 7,
            Self::L => 1 << 8,
            Self::R => 1 << 9,
        }
    }
}

/// Current pressed controls for both ordered controllers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ControllerState {
    first: u16,
    second: u16,
}

impl ControllerState {
    /// Apply one normalized controller edge.
    pub const fn set(&mut self, controller: Controller, button: ControllerButton, pressed: bool) {
        let state = match controller {
            Controller::One => &mut self.first,
            Controller::Two => &mut self.second,
        };
        if pressed {
            *state |= button.mask();
        } else {
            *state &= !button.mask();
        }
    }

    /// Project current controls onto the 16-key CHIP-8 keypad.
    #[must_use]
    pub const fn keypad(self, profile: InputProfile) -> KeypadState {
        let mut keypad = KeypadState::empty();
        match profile {
            InputProfile::Octo => {
                keypad.bind(self.first, ControllerButton::Up, 0x5);
                keypad.bind(self.first, ControllerButton::Down, 0x8);
                keypad.bind(self.first, ControllerButton::Left, 0x7);
                keypad.bind(self.first, ControllerButton::Right, 0x9);
                keypad.bind(self.first, ControllerButton::A, 0x6);
                keypad.bind(self.first, ControllerButton::B, 0x4);
                keypad.bind(self.first, ControllerButton::Select, 0xa);
                keypad.bind(self.first, ControllerButton::Start, 0xf);
            }
            InputProfile::SpaceRacer => {
                keypad.bind(self.first, ControllerButton::Up, 0x4);
                keypad.bind(self.first, ControllerButton::Down, 0x7);
                keypad.bind(self.second, ControllerButton::Up, 0xd);
                keypad.bind(self.second, ControllerButton::Down, 0xe);
                keypad.bind(self.first, ControllerButton::A, 0xf);
                keypad.bind(self.first, ControllerButton::Start, 0xf);
            }
        }
        keypad
    }
}

/// Compact immutable CHIP-8 keypad snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeypadState(u16);

impl KeypadState {
    /// Empty keypad snapshot.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Return whether a hexadecimal CHIP-8 key is pressed.
    #[must_use]
    pub const fn pressed(self, key: u8) -> bool {
        key < 16 && self.0 & (1_u16 << key) != 0
    }

    const fn bind(&mut self, controls: u16, button: ControllerButton, key: u8) {
        if controls & button.mask() != 0 {
            self.0 |= 1_u16 << key;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octo_profile_matches_the_legacy_wasd_qezv_layout() {
        let mut controllers = ControllerState::default();
        for button in [
            ControllerButton::Up,
            ControllerButton::Right,
            ControllerButton::A,
            ControllerButton::Select,
        ] {
            controllers.set(Controller::One, button, true);
        }
        controllers.set(Controller::Two, ControllerButton::Down, true);
        let keypad = controllers.keypad(InputProfile::Octo);
        for key in [0x5, 0x9, 0x6, 0xa] {
            assert!(keypad.pressed(key));
        }
        for key in [0x4, 0x7, 0x8, 0xd, 0xe, 0xf] {
            assert!(!keypad.pressed(key));
        }
    }

    #[test]
    fn space_racer_maps_both_players_and_either_start_control() {
        let mut controllers = ControllerState::default();
        controllers.set(Controller::One, ControllerButton::Down, true);
        controllers.set(Controller::One, ControllerButton::Start, true);
        controllers.set(Controller::Two, ControllerButton::Up, true);
        let keypad = controllers.keypad(InputProfile::SpaceRacer);
        assert!(keypad.pressed(0x7));
        assert!(keypad.pressed(0xd));
        assert!(keypad.pressed(0xf));

        controllers.set(Controller::One, ControllerButton::Start, false);
        controllers.set(Controller::One, ControllerButton::A, true);
        assert!(controllers.keypad(InputProfile::SpaceRacer).pressed(0xf));
    }

    #[test]
    fn released_controls_and_out_of_range_keys_are_clear() {
        let mut controllers = ControllerState::default();
        controllers.set(Controller::One, ControllerButton::B, true);
        assert!(controllers.keypad(InputProfile::Octo).pressed(0x4));
        controllers.set(Controller::One, ControllerButton::B, false);
        let keypad = controllers.keypad(InputProfile::Octo);
        assert!(!keypad.pressed(0x4));
        assert!(!keypad.pressed(16));
        assert!(!keypad.pressed(u8::MAX));
    }
}
