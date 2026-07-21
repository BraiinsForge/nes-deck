//! Normalize compositor-routed raw gamepad reports without owning devices.

use super::{AxisRange, ButtonSet, ControllerTracker, InputEvent, PhysicalButton, Player};

const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const PHYSICAL_BUTTONS: [PhysicalButton; 8] = [
    PhysicalButton::Y,
    PhysicalButton::B,
    PhysicalButton::A,
    PhysicalButton::X,
    PhysicalButton::L,
    PhysicalButton::R,
    PhysicalButton::Back,
    PhysicalButton::Start,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ControllerState {
    x_range: Option<AxisRange>,
    y_range: Option<AxisRange>,
    x: i32,
    y: i32,
    physical: u8,
    tracker: Option<ControllerTracker>,
}

impl ControllerState {
    fn axis_info(&mut self, code: u16, minimum: i32, maximum: i32) {
        let Some(range) = AxisRange::new(minimum, maximum) else {
            return;
        };
        match code {
            ABS_X => {
                self.x_range = Some(range);
                self.x = midpoint(minimum, maximum);
            }
            ABS_Y => {
                self.y_range = Some(range);
                self.y = midpoint(minimum, maximum);
            }
            _ => return,
        }
        self.rebuild_tracker();
    }

    const fn button(&mut self, code: u16, pressed: bool) {
        let Some(button) = physical_button(code) else {
            return;
        };
        if pressed {
            self.physical |= button.mask();
        } else {
            self.physical &= !button.mask();
        }
        if let Some(tracker) = &mut self.tracker {
            tracker.set_button(button, pressed);
        }
    }

    const fn axis(&mut self, code: u16, value: i32) {
        match code {
            ABS_X => {
                self.x = value;
                if let Some(tracker) = &mut self.tracker {
                    tracker.set_x(value);
                }
            }
            ABS_Y => {
                self.y = value;
                if let Some(tracker) = &mut self.tracker {
                    tracker.set_y(value);
                }
            }
            _ => {}
        }
    }

    fn finish_report(&mut self, player: Player, output: &mut Vec<InputEvent>) {
        if let Some(tracker) = &mut self.tracker {
            tracker.finish_report(player, &mut |event| output.push(event));
        }
    }

    fn buttons(self) -> ButtonSet {
        match self.tracker {
            Some(tracker) => tracker.state(),
            None => ButtonSet::empty(),
        }
    }

    fn rebuild_tracker(&mut self) {
        let (Some(x_range), Some(y_range)) = (self.x_range, self.y_range) else {
            return;
        };
        let pressed = PHYSICAL_BUTTONS
            .into_iter()
            .filter(|button| self.physical & button.mask() != 0);
        self.tracker = Some(ControllerTracker::new(
            x_range, y_range, self.x, self.y, pressed,
        ));
    }
}

/// Two stable player slots fed by the BMC gamepad protocol.
#[derive(Debug, Default)]
pub(crate) struct WaylandControllers {
    one: ControllerState,
    two: ControllerState,
}

impl WaylandControllers {
    pub(crate) fn connected(&mut self, player: Player) {
        *self.player_mut(player) = ControllerState::default();
    }

    pub(crate) fn disconnected(&mut self, player: Player) {
        *self.player_mut(player) = ControllerState::default();
    }

    pub(crate) fn axis_info(&mut self, player: Player, code: u16, minimum: i32, maximum: i32) {
        self.player_mut(player).axis_info(code, minimum, maximum);
    }

    pub(crate) const fn button(&mut self, player: Player, code: u16, pressed: bool) {
        self.player_mut(player).button(code, pressed);
    }

    pub(crate) const fn axis(&mut self, player: Player, code: u16, value: i32) {
        self.player_mut(player).axis(code, value);
    }

    pub(crate) fn finish_report(&mut self, player: Player, output: &mut Vec<InputEvent>) {
        self.player_mut(player).finish_report(player, output);
    }

    pub(crate) fn buttons(&self, player: Player) -> ButtonSet {
        self.player(player).buttons()
    }

    const fn player_mut(&mut self, player: Player) -> &mut ControllerState {
        match player {
            Player::One => &mut self.one,
            Player::Two => &mut self.two,
        }
    }

    const fn player(&self, player: Player) -> &ControllerState {
        match player {
            Player::One => &self.one,
            Player::Two => &self.two,
        }
    }
}

const fn physical_button(code: u16) -> Option<PhysicalButton> {
    match code {
        288 => Some(PhysicalButton::Y),
        289 => Some(PhysicalButton::B),
        290 => Some(PhysicalButton::A),
        291 => Some(PhysicalButton::X),
        292 => Some(PhysicalButton::L),
        293 => Some(PhysicalButton::R),
        294 => Some(PhysicalButton::Back),
        295 => Some(PhysicalButton::Start),
        _ => None,
    }
}

const fn midpoint(minimum: i32, maximum: i32) -> i32 {
    i32::midpoint(minimum, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Button, ButtonEdge};

    #[test]
    fn protocol_reports_keep_player_and_commit_boundaries() {
        let mut controllers = WaylandControllers::default();
        let mut output = Vec::new();
        controllers.connected(Player::Two);
        controllers.axis_info(Player::Two, ABS_X, -32_768, 32_767);
        controllers.axis_info(Player::Two, ABS_Y, -32_768, 32_767);
        controllers.button(Player::Two, 290, true);
        assert!(output.is_empty());
        controllers.finish_report(Player::Two, &mut output);
        assert_eq!(
            output,
            [InputEvent::Controller {
                player: Player::Two,
                button: Button::A,
                edge: ButtonEdge::Pressed,
            }]
        );
        assert!(controllers.buttons(Player::Two).contains(Button::A));
        controllers.disconnected(Player::Two);
        assert_eq!(controllers.buttons(Player::Two), ButtonSet::empty());
    }

    #[test]
    fn protocol_axes_use_the_existing_semantic_tracker() {
        let mut controllers = WaylandControllers::default();
        let mut output = Vec::new();
        controllers.axis_info(Player::One, ABS_X, -100, 100);
        controllers.axis_info(Player::One, ABS_Y, -100, 100);
        controllers.axis(Player::One, ABS_Y, -100);
        controllers.finish_report(Player::One, &mut output);
        assert_eq!(
            output,
            [InputEvent::Controller {
                player: Player::One,
                button: Button::Up,
                edge: ButtonEdge::Pressed,
            }]
        );
    }
}
