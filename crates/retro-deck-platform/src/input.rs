//! Device-independent input state and Linux evdev integration.

mod linux;

pub use linux::{ControllerDevices, DrainStats, InputDevices, InputError};

/// Logical width reported by the Deck touchscreen.
pub const LOGICAL_WIDTH: u16 = crate::DECK_LOGICAL_WIDTH;
/// Logical height reported by the Deck touchscreen.
pub const LOGICAL_HEIGHT: u16 = crate::DECK_LOGICAL_HEIGHT;

/// Stable controller slot exposed to applications and emulators.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Player {
    /// The controller on the first physical USB path.
    One,
    /// The controller on the second physical USB path.
    Two,
}

/// Semantic controller button used throughout Retro Deck.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Button {
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

impl Button {
    const ALL: [Self; 10] = [
        Self::A,
        Self::B,
        Self::Select,
        Self::Start,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::L,
        Self::R,
    ];

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

/// Compact immutable set of semantic buttons.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ButtonSet(u16);

impl ButtonSet {
    /// Construct an empty set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Return whether a button is present.
    #[must_use]
    pub const fn contains(self, button: Button) -> bool {
        self.0 & button.mask() != 0
    }

    const fn insert(&mut self, button: Button) {
        self.0 |= button.mask();
    }
}

/// Raw button position in Retro Games' published `THEGamepad` mapping.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PhysicalButton {
    /// Physical Y, Linux button zero.
    Y,
    /// Physical B, Linux button one.
    B,
    /// Physical A, Linux button two.
    A,
    /// Physical X, Linux button three.
    X,
    /// Left shoulder, Linux button four.
    L,
    /// Right shoulder, Linux button five.
    R,
    /// Back, Linux button six.
    Back,
    /// Start, Linux button seven.
    Start,
}

impl PhysicalButton {
    const fn mask(self) -> u8 {
        match self {
            Self::Y => 1 << 0,
            Self::B => 1 << 1,
            Self::A => 1 << 2,
            Self::X => 1 << 3,
            Self::L => 1 << 4,
            Self::R => 1 << 5,
            Self::Back => 1 << 6,
            Self::Start => 1 << 7,
        }
    }

    const fn semantic(self) -> Button {
        match self {
            Self::A | Self::X => Button::A,
            Self::B | Self::Y => Button::B,
            Self::Back => Button::Select,
            Self::Start => Button::Start,
            Self::L => Button::L,
            Self::R => Button::R,
        }
    }
}

/// Inclusive raw axis range advertised by an evdev device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AxisRange {
    minimum: i32,
    maximum: i32,
}

impl AxisRange {
    /// Construct a nonempty axis range.
    #[must_use]
    pub const fn new(minimum: i32, maximum: i32) -> Option<Self> {
        if maximum > minimum {
            Some(Self { minimum, maximum })
        } else {
            None
        }
    }

    fn direction(self, value: i32, negative: Button, positive: Button) -> ButtonSet {
        let span = i64::from(self.maximum) - i64::from(self.minimum);
        let low = i64::from(self.minimum) + span / 3;
        let high = i64::from(self.maximum) - span / 3;
        let mut buttons = ButtonSet::empty();
        if i64::from(value) <= low {
            buttons.insert(negative);
        } else if i64::from(value) >= high {
            buttons.insert(positive);
        }
        buttons
    }
}

/// Press or release edge committed by one complete evdev report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonEdge {
    /// The semantic button became active.
    Pressed,
    /// The semantic button became inactive.
    Released,
}

/// Normalized event consumed by an application without device I/O.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEvent {
    /// One complete touchscreen report began a new contact.
    TouchPressed(TouchPoint),
    /// One semantic controller button changed state.
    Controller {
        /// Stable controller slot.
        player: Player,
        /// Normalized button.
        button: Button,
        /// New edge state.
        edge: ButtonEdge,
    },
}

/// Clamped logical touchscreen coordinate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchPoint {
    x: u16,
    y: u16,
}

impl TouchPoint {
    /// Logical horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> u16 {
        self.x
    }

    /// Logical vertical coordinate.
    #[must_use]
    pub const fn y(self) -> u16 {
        self.y
    }
}

/// Nonzero logical touchscreen bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchBounds {
    maximum_x: u16,
    maximum_y: u16,
}

impl TouchBounds {
    /// Construct bounds from width and height.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Option<Self> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                maximum_x: width - 1,
                maximum_y: height - 1,
            })
        }
    }

    const fn clamp(self, x: i32, y: i32) -> TouchPoint {
        TouchPoint {
            x: clamp_coordinate(x, self.maximum_x),
            y: clamp_coordinate(y, self.maximum_y),
        }
    }
}

const DECK_TOUCH_BOUNDS: TouchBounds = TouchBounds {
    maximum_x: LOGICAL_WIDTH - 1,
    maximum_y: LOGICAL_HEIGHT - 1,
};

const fn clamp_coordinate(value: i32, maximum: u16) -> u16 {
    if value <= 0 {
        0
    } else if value >= maximum as i32 {
        maximum
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the preceding comparisons prove the value is within u16 bounds"
        )]
        let bounded = value as u16;
        bounded
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ControllerTracker {
    x_range: AxisRange,
    y_range: AxisRange,
    x: i32,
    y: i32,
    physical: u8,
    reported: ButtonSet,
}

impl ControllerTracker {
    pub(crate) fn new(
        x_range: AxisRange,
        y_range: AxisRange,
        x: i32,
        y: i32,
        pressed: impl IntoIterator<Item = PhysicalButton>,
    ) -> Self {
        let mut tracker = Self {
            x_range,
            y_range,
            x,
            y,
            physical: 0,
            reported: ButtonSet::empty(),
        };
        for button in pressed {
            tracker.set_button(button, true);
        }
        tracker.reported = tracker.state();
        tracker
    }

    pub(crate) const fn set_button(&mut self, button: PhysicalButton, pressed: bool) {
        if pressed {
            self.physical |= button.mask();
        } else {
            self.physical &= !button.mask();
        }
    }

    pub(crate) const fn set_x(&mut self, value: i32) {
        self.x = value;
    }

    pub(crate) const fn set_y(&mut self, value: i32) {
        self.y = value;
    }

    pub(crate) fn finish_report(&mut self, player: Player, emit: &mut impl FnMut(InputEvent)) {
        let current = self.state();
        for button in Button::ALL {
            let before = self.reported.contains(button);
            let after = current.contains(button);
            if before != after {
                emit(InputEvent::Controller {
                    player,
                    button,
                    edge: if after {
                        ButtonEdge::Pressed
                    } else {
                        ButtonEdge::Released
                    },
                });
            }
        }
        self.reported = current;
    }

    fn state(self) -> ButtonSet {
        let mut state = ButtonSet::empty();
        for physical in [
            PhysicalButton::Y,
            PhysicalButton::B,
            PhysicalButton::A,
            PhysicalButton::X,
            PhysicalButton::L,
            PhysicalButton::R,
            PhysicalButton::Back,
            PhysicalButton::Start,
        ] {
            if self.physical & physical.mask() != 0 {
                state.insert(physical.semantic());
            }
        }
        let horizontal = self.x_range.direction(self.x, Button::Left, Button::Right);
        let vertical = self.y_range.direction(self.y, Button::Up, Button::Down);
        state.0 |= horizontal.0 | vertical.0;
        state
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TouchTracker {
    bounds: TouchBounds,
    point: TouchPoint,
    down: bool,
    reported_down: bool,
}

impl TouchTracker {
    pub(crate) const fn deck(x: i32, y: i32, down: bool) -> Self {
        Self {
            bounds: DECK_TOUCH_BOUNDS,
            point: DECK_TOUCH_BOUNDS.clamp(x, y),
            down,
            reported_down: down,
        }
    }

    pub(crate) const fn set_x(&mut self, value: i32) {
        self.point.x = clamp_coordinate(value, self.bounds.maximum_x);
    }

    pub(crate) const fn set_y(&mut self, value: i32) {
        self.point.y = clamp_coordinate(value, self.bounds.maximum_y);
    }

    pub(crate) const fn set_down(&mut self, down: bool) {
        self.down = down;
    }

    pub(crate) fn finish_report(&mut self, emit: &mut impl FnMut(InputEvent)) {
        if self.down && !self.reported_down {
            emit(InputEvent::TouchPressed(self.point));
        }
        self.reported_down = self.down;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AXIS: AxisRange = AxisRange {
        minimum: -32_768,
        maximum: 32_767,
    };

    #[test]
    fn invalid_axis_ranges_are_rejected() {
        assert_eq!(AxisRange::new(0, 0), None);
        assert_eq!(AxisRange::new(1, 0), None);
        assert_eq!(
            AxisRange::new(-1, 1),
            Some(AxisRange {
                minimum: -1,
                maximum: 1
            })
        );
    }

    #[test]
    fn axis_thirds_match_the_legacy_mapping() {
        let low = AXIS.direction(-10_923, Button::Left, Button::Right);
        let center = AXIS.direction(0, Button::Left, Button::Right);
        let high = AXIS.direction(10_922, Button::Left, Button::Right);
        assert!(low.contains(Button::Left));
        assert_eq!(center, ButtonSet::empty());
        assert!(high.contains(Button::Right));
    }

    #[test]
    fn face_button_aliases_do_not_release_each_other() {
        let mut tracker = ControllerTracker::new(AXIS, AXIS, 0, 0, []);
        let mut events = Vec::new();
        tracker.set_button(PhysicalButton::A, true);
        tracker.finish_report(Player::One, &mut |event| events.push(event));
        tracker.set_button(PhysicalButton::X, true);
        tracker.set_button(PhysicalButton::A, false);
        tracker.finish_report(Player::One, &mut |event| events.push(event));
        tracker.set_button(PhysicalButton::X, false);
        tracker.finish_report(Player::One, &mut |event| events.push(event));
        assert_eq!(
            events,
            [
                InputEvent::Controller {
                    player: Player::One,
                    button: Button::A,
                    edge: ButtonEdge::Pressed,
                },
                InputEvent::Controller {
                    player: Player::One,
                    button: Button::A,
                    edge: ButtonEdge::Released,
                },
            ]
        );
    }

    #[test]
    fn complete_report_commits_all_controller_edges() {
        let mut tracker = ControllerTracker::new(AXIS, AXIS, 0, 0, []);
        let mut events = Vec::new();
        tracker.set_x(i32::MIN);
        tracker.set_y(i32::MAX);
        tracker.set_button(PhysicalButton::Start, true);
        tracker.finish_report(Player::Two, &mut |event| events.push(event));
        assert_eq!(
            events,
            [
                InputEvent::Controller {
                    player: Player::Two,
                    button: Button::Start,
                    edge: ButtonEdge::Pressed,
                },
                InputEvent::Controller {
                    player: Player::Two,
                    button: Button::Down,
                    edge: ButtonEdge::Pressed,
                },
                InputEvent::Controller {
                    player: Player::Two,
                    button: Button::Left,
                    edge: ButtonEdge::Pressed,
                },
            ]
        );
    }

    #[test]
    fn existing_controller_state_does_not_create_startup_edges() {
        let mut tracker = ControllerTracker::new(AXIS, AXIS, i32::MIN, 0, [PhysicalButton::A]);
        let mut events = Vec::new();
        tracker.finish_report(Player::One, &mut |event| events.push(event));
        assert!(events.is_empty());
    }

    #[test]
    fn touch_is_clamped_and_emitted_only_on_a_rising_report() {
        let mut tracker = TouchTracker::deck(-20, 900, false);
        let mut events = Vec::new();
        tracker.set_down(true);
        tracker.finish_report(&mut |event| events.push(event));
        tracker.finish_report(&mut |event| events.push(event));
        tracker.set_down(false);
        tracker.finish_report(&mut |event| events.push(event));
        assert_eq!(
            events,
            [InputEvent::TouchPressed(TouchPoint {
                x: 0,
                y: LOGICAL_HEIGHT - 1,
            })]
        );
    }

    #[test]
    fn touch_coordinates_commit_at_report_boundary() {
        let mut tracker = TouchTracker::deck(0, 0, false);
        let mut events = Vec::new();
        tracker.set_down(true);
        tracker.set_x(319);
        tracker.set_y(127);
        tracker.finish_report(&mut |event| events.push(event));
        assert_eq!(
            events,
            [InputEvent::TouchPressed(TouchPoint { x: 319, y: 127 })]
        );
    }
}
