//! Deterministic dashboard input routing without device or audio work.

use retro_deck_platform::input::{Button, ButtonEdge};

use crate::{Action, Screen};

const BURST_LIMIT: usize = 12;
const BURST_WINDOW_MS: u64 = 1_000;
const QUIET_RESET_MS: u64 = 1_000;
const EXIT_HOLD_WIDTH: u16 = 96;
const EXIT_HOLD_HEIGHT: u16 = 96;
const EXIT_HOLD_MILLISECONDS: u64 = 2_000;

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

/// Fixed-capacity guard against a malfunctioning controller event flood.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerGuard {
    accepted_at: [u64; BURST_LIMIT],
    accepted: usize,
    last_edge_at: Option<u64>,
    suspended: bool,
}

/// Observable edge in the supervised top-left hold-to-exit gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitHoldEvent {
    /// A clean contact began inside the exit target.
    Started,
    /// The contact ended or left the target before its deadline.
    Cancelled,
    /// The uninterrupted hold reached two seconds.
    Completed,
}

/// Pure hold-to-exit recognizer that never reads a device or clock itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExitHold {
    state: ExitHoldState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitHoldState {
    BlockedUntilRelease,
    Ready,
    Holding { started_at_ms: u64 },
}

impl ExitHold {
    /// Start ready only when no contact was already present at discovery.
    #[must_use]
    pub const fn new(initially_down: bool) -> Self {
        Self {
            state: if initially_down {
                ExitHoldState::BlockedUntilRelease
            } else {
                ExitHoldState::Ready
            },
        }
    }

    /// Advance the gesture from one committed logical touch state.
    ///
    /// The target is the top-left 96 by 96 logical pixels around the visible
    /// cross. Leaving it while held cancels and requires a clean release.
    #[must_use]
    pub const fn update(
        &mut self,
        down: bool,
        x: u16,
        y: u16,
        monotonic_ms: u64,
    ) -> Option<ExitHoldEvent> {
        let inside = down && x < EXIT_HOLD_WIDTH && y < EXIT_HOLD_HEIGHT;
        match self.state {
            ExitHoldState::BlockedUntilRelease => {
                if !down {
                    self.state = ExitHoldState::Ready;
                }
                None
            }
            ExitHoldState::Ready => {
                if inside {
                    self.state = ExitHoldState::Holding {
                        started_at_ms: monotonic_ms,
                    };
                    Some(ExitHoldEvent::Started)
                } else {
                    None
                }
            }
            ExitHoldState::Holding { started_at_ms } => {
                if !inside {
                    self.state = if down {
                        ExitHoldState::BlockedUntilRelease
                    } else {
                        ExitHoldState::Ready
                    };
                    return Some(ExitHoldEvent::Cancelled);
                }
                if monotonic_ms.saturating_sub(started_at_ms) < EXIT_HOLD_MILLISECONDS {
                    return None;
                }
                self.state = ExitHoldState::BlockedUntilRelease;
                Some(ExitHoldEvent::Completed)
            }
        }
    }
}

impl Default for ExitHold {
    fn default() -> Self {
        Self::new(false)
    }
}

impl ControllerGuard {
    /// Empty, immediately accepting guard.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            accepted_at: [0; BURST_LIMIT],
            accepted: 0,
            last_edge_at: None,
            suspended: false,
        }
    }

    /// Accept at most twelve mapped press edges in any one-second window.
    ///
    /// Every edge observed while suspended restarts the quiet interval. Caller
    /// timestamps are monotonic milliseconds; a backwards value fails safely
    /// by retaining prior events in the active window.
    #[must_use]
    pub fn accept(&mut self, now_ms: u64) -> bool {
        self.last_edge_at = Some(now_ms);
        if self.suspended {
            return false;
        }
        let retained_from = self
            .accepted_at
            .get(..self.accepted)
            .unwrap_or_default()
            .iter()
            .position(|accepted| now_ms.saturating_sub(*accepted) < BURST_WINDOW_MS)
            .unwrap_or(self.accepted);
        if retained_from > 0 {
            self.accepted_at
                .copy_within(retained_from..self.accepted, 0);
            self.accepted = self.accepted.saturating_sub(retained_from);
        }
        let Some(slot) = self.accepted_at.get_mut(self.accepted) else {
            self.suspended = true;
            return false;
        };
        *slot = now_ms;
        self.accepted = self.accepted.saturating_add(1);
        true
    }

    /// Resume after one full second without another mapped press edge.
    #[must_use]
    pub fn recover_if_quiet(&mut self, now_ms: u64) -> bool {
        if !self.suspended
            || self
                .last_edge_at
                .is_none_or(|last| now_ms.saturating_sub(last) < QUIET_RESET_MS)
        {
            return false;
        }
        *self = Self::new();
        true
    }

    /// Whether mapped controller actions are currently discarded.
    #[must_use]
    pub const fn suspended(self) -> bool {
        self.suspended
    }
}

impl Default for ControllerGuard {
    fn default() -> Self {
        Self::new()
    }
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

    use super::{ControllerGuard, ExitHold, ExitHoldEvent, TouchCommitter, controller_action};
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

    #[test]
    fn controller_burst_suspends_and_requires_a_quiet_second() {
        let mut guard = ControllerGuard::new();
        for now in 0..12 {
            assert!(guard.accept(now));
        }
        assert!(!guard.accept(12));
        assert!(guard.suspended());
        assert!(!guard.recover_if_quiet(1_011));
        assert!(guard.recover_if_quiet(1_012));
        assert!(!guard.suspended());
        assert!(guard.accept(1_012));
    }

    #[test]
    fn old_edges_expire_without_allocating_or_reordering() {
        let mut guard = ControllerGuard::new();
        for now in 0..12 {
            assert!(guard.accept(now));
        }
        assert!(guard.accept(1_000));
        assert!(guard.accept(1_001));
        assert!(!guard.suspended());
    }

    #[test]
    fn exit_hold_requires_two_seconds_inside_the_visible_corner_target() {
        let mut hold = ExitHold::new(false);
        assert_eq!(
            hold.update(true, 40, 40, 1_000),
            Some(ExitHoldEvent::Started)
        );
        assert_eq!(hold.update(true, 40, 40, 2_999), None);
        assert_eq!(
            hold.update(true, 40, 40, 3_000),
            Some(ExitHoldEvent::Completed)
        );
        assert_eq!(hold.update(true, 40, 40, 4_000), None);
        assert_eq!(hold.update(false, 40, 40, 4_001), None);
        assert_eq!(
            hold.update(true, 95, 95, 4_002),
            Some(ExitHoldEvent::Started)
        );
    }

    #[test]
    fn exit_hold_rejects_launch_contacts_and_dragging_outside() {
        let mut held_at_launch = ExitHold::new(true);
        assert_eq!(held_at_launch.update(true, 20, 20, 10_000), None);
        assert_eq!(held_at_launch.update(true, 20, 20, 20_000), None);
        assert_eq!(held_at_launch.update(false, 20, 20, 20_001), None);
        assert_eq!(
            held_at_launch.update(true, 20, 20, 20_002),
            Some(ExitHoldEvent::Started)
        );
        assert_eq!(
            held_at_launch.update(true, 100, 20, 21_000),
            Some(ExitHoldEvent::Cancelled)
        );
        assert_eq!(held_at_launch.update(true, 20, 20, 30_000), None);
        assert_eq!(held_at_launch.update(false, 20, 20, 30_001), None);
        assert_eq!(held_at_launch.update(true, 200, 200, 30_002), None);
    }
}
