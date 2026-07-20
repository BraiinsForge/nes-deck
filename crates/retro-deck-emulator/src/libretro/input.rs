//! Pure projection from shared semantic input into libretro button values.

use retro_deck_platform::input::{Button, ButtonSet};

/// Special libretro input identifier that requests every joypad bit at once.
pub const JOYPAD_MASK_ID: u32 = 256;

/// Joypad buttons used by the supported cores.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum JoypadButton {
    /// Primary face action.
    A,
    /// Secondary face action.
    B,
    /// Select or Back.
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

impl JoypadButton {
    /// Numeric identifier defined by libretro API version 1.
    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Self::B => 0,
            Self::Select => 2,
            Self::Start => 3,
            Self::Up => 4,
            Self::Down => 5,
            Self::Left => 6,
            Self::Right => 7,
            Self::A => 8,
            Self::L => 10,
            Self::R => 11,
        }
    }

    const fn semantic(self) -> Button {
        match self {
            Self::A => Button::A,
            Self::B => Button::B,
            Self::Select => Button::Select,
            Self::Start => Button::Start,
            Self::Up => Button::Up,
            Self::Down => Button::Down,
            Self::Left => Button::Left,
            Self::Right => Button::Right,
            Self::L => Button::L,
            Self::R => Button::R,
        }
    }
}

/// Complete immutable libretro joypad snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JoypadState(u16);

impl JoypadState {
    /// Project one shared semantic controller snapshot into libretro.
    #[must_use]
    pub const fn from_buttons(buttons: ButtonSet) -> Self {
        let mut bits = 0_u16;
        bits |= projected_bit(buttons, JoypadButton::A);
        bits |= projected_bit(buttons, JoypadButton::B);
        bits |= projected_bit(buttons, JoypadButton::Select);
        bits |= projected_bit(buttons, JoypadButton::Start);
        bits |= projected_bit(buttons, JoypadButton::Up);
        bits |= projected_bit(buttons, JoypadButton::Down);
        bits |= projected_bit(buttons, JoypadButton::Left);
        bits |= projected_bit(buttons, JoypadButton::Right);
        bits |= projected_bit(buttons, JoypadButton::L);
        bits |= projected_bit(buttons, JoypadButton::R);
        Self(bits)
    }

    /// Complete libretro joypad bit mask.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Value returned for one libretro joypad input identifier.
    #[must_use]
    pub const fn value(self, id: u32) -> u16 {
        if id == JOYPAD_MASK_ID {
            self.0
        } else if id <= 15 {
            (self.0 >> id) & 1
        } else {
            0
        }
    }

    /// Whether one supported joypad button is pressed.
    #[must_use]
    pub const fn contains(self, button: JoypadButton) -> bool {
        self.value(button.id()) != 0
    }

    /// Combine simultaneous input sources into one joypad snapshot.
    #[must_use]
    pub const fn merged(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

const fn projected_bit(buttons: ButtonSet, button: JoypadButton) -> u16 {
    if buttons.contains(button.semantic()) {
        1_u16 << button.id()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_buttons_keep_their_native_libretro_positions() {
        let mappings = [
            (Button::A, JoypadButton::A, 8),
            (Button::B, JoypadButton::B, 0),
            (Button::Select, JoypadButton::Select, 2),
            (Button::Start, JoypadButton::Start, 3),
            (Button::Up, JoypadButton::Up, 4),
            (Button::Down, JoypadButton::Down, 5),
            (Button::Left, JoypadButton::Left, 6),
            (Button::Right, JoypadButton::Right, 7),
            (Button::L, JoypadButton::L, 10),
            (Button::R, JoypadButton::R, 11),
        ];

        for (semantic, libretro, id) in mappings {
            let state = JoypadState::from_buttons(ButtonSet::empty().with(semantic, true));
            assert_eq!(libretro.id(), id);
            assert_eq!(state.bits(), 1_u16 << id);
            assert!(state.contains(libretro));
            assert_eq!(state.value(id), 1);
        }
    }

    #[test]
    fn complete_masks_and_individual_queries_agree() {
        let buttons = ButtonSet::empty()
            .with(Button::A, true)
            .with(Button::B, true)
            .with(Button::Start, true)
            .with(Button::Left, true)
            .with(Button::R, true);
        let state = JoypadState::from_buttons(buttons);
        let expected = (1 << 8) | 1 | (1 << 3) | (1 << 6) | (1 << 11);
        assert_eq!(state.value(JOYPAD_MASK_ID), expected);
        assert_eq!(state.bits(), expected);
        assert_eq!(state.value(1), 0);
        assert_eq!(state.value(9), 0);
        assert_eq!(state.value(16), 0);
        assert_eq!(state.value(u32::MAX), 0);
    }

    #[test]
    fn empty_shared_state_is_an_empty_libretro_state() {
        assert_eq!(
            JoypadState::from_buttons(ButtonSet::empty()),
            JoypadState::default()
        );
    }

    #[test]
    fn merged_snapshots_preserve_both_sources() {
        let first = JoypadState::from_buttons(ButtonSet::empty().with(Button::A, true));
        let second = JoypadState::from_buttons(ButtonSet::empty().with(Button::B, true));
        let merged = first.merged(second);
        assert!(merged.contains(JoypadButton::A));
        assert!(merged.contains(JoypadButton::B));
    }
}
