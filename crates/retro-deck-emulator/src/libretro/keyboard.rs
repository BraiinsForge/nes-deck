//! Keyboard projections for console-style and ZX Spectrum input.

use retro_deck_platform::input::{Button, ButtonSet, KeyboardState, MediumRawKey};

use super::JoypadState;

const KEY_1: u8 = 2;
const KEY_2: u8 = 3;
const KEY_3: u8 = 4;
const KEY_4: u8 = 5;
const KEY_5: u8 = 6;
const KEY_6: u8 = 7;
const KEY_7: u8 = 8;
const KEY_8: u8 = 9;
const KEY_9: u8 = 10;
const KEY_0: u8 = 11;
const KEY_BACKSPACE: u8 = 14;
const KEY_Q: u8 = 16;
const KEY_W: u8 = 17;
const KEY_E: u8 = 18;
const KEY_R: u8 = 19;
const KEY_T: u8 = 20;
const KEY_Y: u8 = 21;
const KEY_U: u8 = 22;
const KEY_I: u8 = 23;
const KEY_O: u8 = 24;
const KEY_P: u8 = 25;
const KEY_ENTER: u8 = 28;
const KEY_LEFT_CONTROL: u8 = 29;
const KEY_A: u8 = 30;
const KEY_S: u8 = 31;
const KEY_D: u8 = 32;
const KEY_F: u8 = 33;
const KEY_G: u8 = 34;
const KEY_H: u8 = 35;
const KEY_J: u8 = 36;
const KEY_K: u8 = 37;
const KEY_L: u8 = 38;
const KEY_LEFT_SHIFT: u8 = 42;
const KEY_Z: u8 = 44;
const KEY_X: u8 = 45;
const KEY_C: u8 = 46;
const KEY_V: u8 = 47;
const KEY_B: u8 = 48;
const KEY_N: u8 = 49;
const KEY_M: u8 = 50;
const KEY_RIGHT_SHIFT: u8 = 54;
const KEY_LEFT_ALT: u8 = 56;
const KEY_SPACE: u8 = 57;
const KEY_RIGHT_CONTROL: u8 = 97;
const KEY_RIGHT_ALT: u8 = 100;
const KEY_UP: u8 = 103;
const KEY_LEFT: u8 = 105;
const KEY_RIGHT: u8 = 106;
const KEY_DOWN: u8 = 108;
const KEY_LEFT_META: u8 = 125;
const KEY_RIGHT_META: u8 = 126;

/// Project a keyboard into Player 1 controls for NES and Game Boy games.
#[must_use]
pub const fn joypad_from_keyboard(keyboard: KeyboardState) -> JoypadState {
    let up = pressed(keyboard, KEY_UP) || pressed(keyboard, KEY_W);
    let down = pressed(keyboard, KEY_DOWN) || pressed(keyboard, KEY_S);
    let left = pressed(keyboard, KEY_LEFT) || pressed(keyboard, KEY_A);
    let right = pressed(keyboard, KEY_RIGHT) || pressed(keyboard, KEY_D);
    let buttons = ButtonSet::empty()
        .with(Button::A, pressed(keyboard, KEY_SPACE))
        .with(
            Button::B,
            pressed(keyboard, KEY_LEFT_SHIFT) || pressed(keyboard, KEY_RIGHT_SHIFT),
        )
        .with(
            Button::Select,
            pressed(keyboard, KEY_LEFT_CONTROL) || pressed(keyboard, KEY_RIGHT_CONTROL),
        )
        .with(Button::Start, pressed(keyboard, KEY_ENTER))
        .with(Button::Up, up)
        .with(Button::Down, down)
        .with(Button::Left, left)
        .with(Button::Right, right);
    JoypadState::from_buttons(buttons)
}

/// Translate one libretro keyboard identifier into a medium-raw Linux key.
///
/// Fuse requests these identifiers from its keyboard input callback. Unknown
/// keys are intentionally absent instead of aliasing to the reserved code.
#[must_use]
pub const fn medium_raw_key_for_retro(retro_key: u32) -> Option<MediumRawKey> {
    let code = match retro_key {
        8 => KEY_BACKSPACE,
        13 => KEY_ENTER,
        32 => KEY_SPACE,
        48 => KEY_0,
        49 => KEY_1,
        50 => KEY_2,
        51 => KEY_3,
        52 => KEY_4,
        53 => KEY_5,
        54 => KEY_6,
        55 => KEY_7,
        56 => KEY_8,
        57 => KEY_9,
        97 => KEY_A,
        98 => KEY_B,
        99 => KEY_C,
        100 => KEY_D,
        101 => KEY_E,
        102 => KEY_F,
        103 => KEY_G,
        104 => KEY_H,
        105 => KEY_I,
        106 => KEY_J,
        107 => KEY_K,
        108 => KEY_L,
        109 => KEY_M,
        110 => KEY_N,
        111 => KEY_O,
        112 => KEY_P,
        113 => KEY_Q,
        114 => KEY_R,
        115 => KEY_S,
        116 => KEY_T,
        117 => KEY_U,
        118 => KEY_V,
        119 => KEY_W,
        120 => KEY_X,
        121 => KEY_Y,
        122 => KEY_Z,
        273 => KEY_UP,
        274 => KEY_DOWN,
        275 => KEY_RIGHT,
        276 => KEY_LEFT,
        303 => KEY_RIGHT_SHIFT,
        304 => KEY_LEFT_SHIFT,
        305 => KEY_RIGHT_CONTROL,
        306 => KEY_LEFT_CONTROL,
        307 => KEY_RIGHT_ALT,
        308 => KEY_LEFT_ALT,
        309 | 312 => KEY_RIGHT_META,
        310 | 311 => KEY_LEFT_META,
        _ => return None,
    };
    MediumRawKey::new(code)
}

const fn pressed(keyboard: KeyboardState, code: u8) -> bool {
    let Some(key) = MediumRawKey::new(code) else {
        return false;
    };
    keyboard.contains(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libretro::JoypadButton;

    fn key(code: u8) -> MediumRawKey {
        MediumRawKey::new(code).expect("test key code is valid in medium-raw mode")
    }

    #[test]
    fn console_keyboard_contract_matches_the_dashboard_help() {
        let keyboard = KeyboardState::empty()
            .with(key(KEY_SPACE), true)
            .with(key(KEY_LEFT_SHIFT), true)
            .with(key(KEY_ENTER), true)
            .with(key(KEY_RIGHT_CONTROL), true)
            .with(key(KEY_W), true)
            .with(key(KEY_RIGHT), true);
        let joypad = joypad_from_keyboard(keyboard);
        assert!(joypad.contains(JoypadButton::A));
        assert!(joypad.contains(JoypadButton::B));
        assert!(joypad.contains(JoypadButton::Start));
        assert!(joypad.contains(JoypadButton::Select));
        assert!(joypad.contains(JoypadButton::Up));
        assert!(joypad.contains(JoypadButton::Right));
        assert!(!joypad.contains(JoypadButton::Down));
        assert!(!joypad.contains(JoypadButton::Left));
    }

    #[test]
    fn zx_digits_and_letters_follow_linux_qwerty_codes() {
        let expected = [
            (48, KEY_0),
            (49, KEY_1),
            (57, KEY_9),
            (97, KEY_A),
            (109, KEY_M),
            (113, KEY_Q),
            (122, KEY_Z),
        ];
        for (retro, linux) in expected {
            assert_eq!(medium_raw_key_for_retro(retro), Some(key(linux)));
        }
    }

    #[test]
    fn zx_controls_and_modifiers_are_complete() {
        let expected = [
            (8, KEY_BACKSPACE),
            (13, KEY_ENTER),
            (32, KEY_SPACE),
            (273, KEY_UP),
            (274, KEY_DOWN),
            (275, KEY_RIGHT),
            (276, KEY_LEFT),
            (303, KEY_RIGHT_SHIFT),
            (304, KEY_LEFT_SHIFT),
            (305, KEY_RIGHT_CONTROL),
            (306, KEY_LEFT_CONTROL),
            (307, KEY_RIGHT_ALT),
            (308, KEY_LEFT_ALT),
            (309, KEY_RIGHT_META),
            (310, KEY_LEFT_META),
            (311, KEY_LEFT_META),
            (312, KEY_RIGHT_META),
        ];
        for (retro, linux) in expected {
            assert_eq!(medium_raw_key_for_retro(retro), Some(key(linux)));
        }
    }

    #[test]
    fn zx_unknown_keys_are_not_reserved_aliases() {
        assert_eq!(medium_raw_key_for_retro(0), None);
        assert_eq!(medium_raw_key_for_retro(9), None);
        assert_eq!(medium_raw_key_for_retro(282), None);
        assert_eq!(medium_raw_key_for_retro(u32::MAX), None);
    }
}
