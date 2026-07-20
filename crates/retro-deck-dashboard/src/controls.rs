//! Deterministic dashboard input routing without device or audio work.

use retro_deck_platform::input::{Button, ButtonEdge};

use crate::{Action, Screen};

/// Convert one committed controller edge to a pure dashboard action.
///
/// Both connected controllers use the same menu controls. Shoulder buttons
/// change volume as promised by the UI; they never share category navigation.
#[must_use]
pub const fn controller_action(screen: Screen, button: Button, edge: ButtonEdge) -> Option<Action> {
    if matches!(edge, ButtonEdge::Released) {
        return None;
    }
    match button {
        Button::A => Some(Action::Confirm),
        Button::B => Some(Action::Back),
        Button::Select => Some(Action::ToggleSettings),
        Button::L => Some(Action::VolumeDown),
        Button::R => Some(Action::VolumeUp),
        Button::Left => directional_action(screen, Action::Previous),
        Button::Right => directional_action(screen, Action::Next),
        Button::Up => match screen {
            Screen::Dashboard => Some(Action::CategoryPrevious),
            Screen::Settings => Some(Action::Previous),
            Screen::Credits => None,
        },
        Button::Down => match screen {
            Screen::Dashboard => Some(Action::CategoryNext),
            Screen::Settings => Some(Action::Next),
            Screen::Credits => None,
        },
        Button::Start => None,
    }
}

const fn directional_action(screen: Screen, action: Action) -> Option<Action> {
    match screen {
        Screen::Dashboard | Screen::Settings => Some(action),
        Screen::Credits => None,
    }
}

/// Touch press/release pairing that commits only one unchanged target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TouchCommitter {
    pressed: Option<Action>,
}

impl TouchCommitter {
    /// Start or finish a touch report over the currently hit action.
    ///
    /// Moving off the pressed control cancels activation at release. A report
    /// containing both edges can commit immediately, and a new press replaces
    /// stale gesture state.
    #[must_use]
    pub fn update(
        &mut self,
        pressed: bool,
        released: bool,
        target: Option<Action>,
    ) -> Option<Action> {
        if pressed {
            self.pressed = target;
        }
        if !released {
            return None;
        }
        let started = self.pressed.take();
        (started.is_some() && started == target)
            .then_some(started)
            .flatten()
    }

    /// Cancel a partial gesture after a screen change or non-touch action.
    pub const fn cancel(&mut self) {
        self.pressed = None;
    }
}

#[cfg(test)]
mod tests {
    use retro_deck_platform::input::{Button, ButtonEdge};

    use super::{TouchCommitter, controller_action};
    use crate::{Action, Screen};

    #[test]
    fn controller_mapping_separates_categories_carousel_and_volume() {
        assert_eq!(
            controller_action(Screen::Dashboard, Button::Left, ButtonEdge::Pressed),
            Some(Action::Previous)
        );
        assert_eq!(
            controller_action(Screen::Dashboard, Button::Up, ButtonEdge::Pressed),
            Some(Action::CategoryPrevious)
        );
        assert_eq!(
            controller_action(Screen::Dashboard, Button::L, ButtonEdge::Pressed),
            Some(Action::VolumeDown)
        );
        assert_eq!(
            controller_action(Screen::Dashboard, Button::R, ButtonEdge::Pressed),
            Some(Action::VolumeUp)
        );
        assert_eq!(
            controller_action(Screen::Dashboard, Button::A, ButtonEdge::Released),
            None
        );
    }

    #[test]
    fn modal_controls_are_explicit_and_directional_input_is_ignored_in_credits() {
        assert_eq!(
            controller_action(Screen::Settings, Button::Down, ButtonEdge::Pressed),
            Some(Action::Next)
        );
        assert_eq!(
            controller_action(Screen::Settings, Button::Select, ButtonEdge::Pressed),
            Some(Action::ToggleSettings)
        );
        assert_eq!(
            controller_action(Screen::Credits, Button::Left, ButtonEdge::Pressed),
            None
        );
        assert_eq!(
            controller_action(Screen::Credits, Button::B, ButtonEdge::Pressed),
            Some(Action::Back)
        );
    }

    #[test]
    fn touch_commits_only_the_target_pressed_and_released() {
        let mut touch = TouchCommitter::default();
        assert_eq!(touch.update(true, false, Some(Action::Next)), None);
        assert_eq!(
            touch.update(false, true, Some(Action::Next)),
            Some(Action::Next)
        );

        assert_eq!(touch.update(true, false, Some(Action::Previous)), None);
        assert_eq!(touch.update(false, true, Some(Action::Next)), None);

        assert_eq!(
            touch.update(true, true, Some(Action::ToggleSettings)),
            Some(Action::ToggleSettings)
        );
        assert_eq!(touch.update(true, false, Some(Action::ShowCredits)), None);
        touch.cancel();
        assert_eq!(touch.update(false, true, Some(Action::ShowCredits)), None);
    }
}
