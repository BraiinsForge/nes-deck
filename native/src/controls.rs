use evdev::{AbsoluteAxisCode, EventSummary, InputEvent, KeyCode, SynchronizationCode};

pub const KEYBOARD_REPORT: u32 = 0;
pub const GAMEPAD_REPORT: u32 = 1;
pub const KEYBOARD_SHIFT: u32 = 1;
pub const KEYBOARD_REPEAT: u32 = 2;

const GAMEPAD_X_NEGATIVE: u32 = 1 << 8;
const GAMEPAD_X_POSITIVE: u32 = 1 << 9;
const GAMEPAD_Y_NEGATIVE: u32 = 1 << 10;
const GAMEPAD_Y_POSITIVE: u32 = 1 << 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlReport {
    pub kind: u32,
    pub value: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlAction {
    Ignore,
    Report(ControlReport),
    Resynchronize,
}

#[derive(Clone, Copy, Debug, Default)]
struct KeyboardState {
    left_shift: bool,
    right_shift: bool,
    dropping_events: bool,
}

impl KeyboardState {
    fn handle(&mut self, event: InputEvent) -> ControlAction {
        if self.dropping_events {
            return match event.destructure() {
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    ControlAction::Resynchronize
                }
                _ => ControlAction::Ignore,
            };
        }

        match event.destructure() {
            EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                self.dropping_events = true;
                ControlAction::Ignore
            }
            EventSummary::Key(_, KeyCode::KEY_LEFTSHIFT, value) => {
                self.left_shift = value != 0;
                ControlAction::Ignore
            }
            EventSummary::Key(_, KeyCode::KEY_RIGHTSHIFT, value) => {
                self.right_shift = value != 0;
                ControlAction::Ignore
            }
            EventSummary::Key(_, code, value) => {
                let repeat = value == 2 && keyboard_key_repeats(code);
                if value != 1 && !repeat {
                    return ControlAction::Ignore;
                }
                let mut flags = 0;
                if self.left_shift || self.right_shift {
                    flags |= KEYBOARD_SHIFT;
                }
                if repeat {
                    flags |= KEYBOARD_REPEAT;
                }
                ControlAction::Report(ControlReport {
                    kind: KEYBOARD_REPORT,
                    value: u32::from(code.0),
                    flags,
                })
            }
            _ => ControlAction::Ignore,
        }
    }

    fn resynchronize(&mut self, left_shift: bool, right_shift: bool) {
        self.left_shift = left_shift;
        self.right_shift = right_shift;
        self.dropping_events = false;
    }
}

fn keyboard_key_repeats(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::KEY_UP | KeyCode::KEY_DOWN | KeyCode::KEY_LEFT | KeyCode::KEY_RIGHT
    )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AxisInfo {
    minimum: i32,
    maximum: i32,
    value: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct GamepadSnapshot {
    x: AxisInfo,
    y: AxisInfo,
    raw_buttons: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct GamepadState {
    snapshot: GamepadSnapshot,
    state: u32,
    dropping_events: bool,
}

impl GamepadState {
    fn new(snapshot: GamepadSnapshot) -> Self {
        let mut state = Self::default();
        state.resynchronize(snapshot);
        state
    }

    fn handle(&mut self, event: InputEvent) -> ControlAction {
        if self.dropping_events {
            return match event.destructure() {
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    ControlAction::Resynchronize
                }
                _ => ControlAction::Ignore,
            };
        }

        match event.destructure() {
            EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                self.dropping_events = true;
                ControlAction::Ignore
            }
            EventSummary::Key(_, code, value)
                if code.0 >= KeyCode::BTN_TRIGGER.0 && code.0 <= KeyCode::BTN_BASE2.0 =>
            {
                let bit = 1 << (code.0 - KeyCode::BTN_TRIGGER.0);
                if value != 0 {
                    self.snapshot.raw_buttons |= bit;
                } else {
                    self.snapshot.raw_buttons &= !bit;
                }
                ControlAction::Ignore
            }
            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_X, value) => {
                self.snapshot.x.value = value;
                ControlAction::Ignore
            }
            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_Y, value) => {
                self.snapshot.y.value = value;
                ControlAction::Ignore
            }
            EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                let state = gamepad_state(self.snapshot);
                let pressed = state & !self.state;
                self.state = state;
                if pressed == 0 {
                    ControlAction::Ignore
                } else {
                    ControlAction::Report(ControlReport {
                        kind: GAMEPAD_REPORT,
                        value: pressed,
                        flags: 0,
                    })
                }
            }
            _ => ControlAction::Ignore,
        }
    }

    fn resynchronize(&mut self, snapshot: GamepadSnapshot) {
        self.snapshot = snapshot;
        self.state = gamepad_state(snapshot);
        self.dropping_events = false;
    }
}

fn gamepad_state(snapshot: GamepadSnapshot) -> u32 {
    snapshot.raw_buttons
        | axis_state(snapshot.x, GAMEPAD_X_NEGATIVE, GAMEPAD_X_POSITIVE)
        | axis_state(snapshot.y, GAMEPAD_Y_NEGATIVE, GAMEPAD_Y_POSITIVE)
}

fn axis_state(axis: AxisInfo, negative: u32, positive: u32) -> u32 {
    if axis.maximum <= axis.minimum {
        return 0;
    }
    let span = i64::from(axis.maximum) - i64::from(axis.minimum);
    let low = axis.minimum + (span / 3) as i32;
    let high = axis.maximum - (span / 3) as i32;
    if axis.value <= low {
        negative
    } else if axis.value >= high {
        positive
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::EventType;

    fn event(event_type: EventType, code: u16, value: i32) -> InputEvent {
        InputEvent::new(event_type.0, code, value)
    }

    fn key(code: KeyCode, value: i32) -> InputEvent {
        event(EventType::KEY, code.0, value)
    }

    fn syn(code: SynchronizationCode) -> InputEvent {
        event(EventType::SYNCHRONIZATION, code.0, 0)
    }

    fn axis(code: AbsoluteAxisCode, value: i32) -> InputEvent {
        event(EventType::ABSOLUTE, code.0, value)
    }

    fn gamepad_snapshot(x: i32, y: i32, raw_buttons: u32) -> GamepadSnapshot {
        GamepadSnapshot {
            x: AxisInfo {
                minimum: 0,
                maximum: 255,
                value: x,
            },
            y: AxisInfo {
                minimum: 0,
                maximum: 255,
                value: y,
            },
            raw_buttons,
        }
    }

    #[test]
    fn keyboard_tracks_shift_and_accepts_only_arrow_repeats() {
        let mut state = KeyboardState::default();
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_TAB.0),
                flags: 0,
            })
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_LEFTSHIFT, 1)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_TAB.0),
                flags: KEYBOARD_SHIFT,
            })
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 2)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_RIGHT, 2)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_RIGHT.0),
                flags: KEYBOARD_SHIFT | KEYBOARD_REPEAT,
            })
        );
        state.handle(key(KeyCode::KEY_RIGHTSHIFT, 1));
        state.handle(key(KeyCode::KEY_LEFTSHIFT, 0));
        assert_eq!(
            state.handle(key(KeyCode::KEY_ENTER, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_ENTER.0),
                flags: KEYBOARD_SHIFT,
            })
        );
        state.handle(key(KeyCode::KEY_RIGHTSHIFT, 0));
        assert_eq!(
            state.handle(key(KeyCode::KEY_ENTER, 0)),
            ControlAction::Ignore
        );
    }

    #[test]
    fn keyboard_drop_waits_for_report_and_resynchronizes_without_an_edge() {
        let mut state = KeyboardState::default();
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_DROPPED)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_LEFTSHIFT, 1)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Resynchronize
        );
        state.resynchronize(false, true);
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_TAB.0),
                flags: KEYBOARD_SHIFT,
            })
        );
    }

    #[test]
    fn gamepad_uses_exact_thirds_and_reports_rising_edges_on_syn_report() {
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 84
                },
                1,
                2
            ),
            1
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 85
                },
                1,
                2
            ),
            1
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 86
                },
                1,
                2
            ),
            0
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 169
                },
                1,
                2
            ),
            0
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 170
                },
                1,
                2
            ),
            2
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 4,
                    maximum: 4,
                    value: 4
                },
                1,
                2
            ),
            0
        );

        let mut state = GamepadState::new(gamepad_snapshot(127, 127, 0));
        state.handle(axis(AbsoluteAxisCode::ABS_X, 255));
        state.handle(key(KeyCode::BTN_THUMB2, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Report(ControlReport {
                kind: GAMEPAD_REPORT,
                value: GAMEPAD_X_POSITIVE | (1 << (KeyCode::BTN_THUMB2.0 - KeyCode::BTN_TRIGGER.0)),
                flags: 0,
            })
        );
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Ignore
        );
        state.handle(key(KeyCode::BTN_THUMB2, 0));
        state.handle(syn(SynchronizationCode::SYN_REPORT));
        state.handle(key(KeyCode::BTN_THUMB2, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Report(ControlReport {
                kind: GAMEPAD_REPORT,
                value: 1 << (KeyCode::BTN_THUMB2.0 - KeyCode::BTN_TRIGGER.0),
                flags: 0,
            })
        );
    }

    #[test]
    fn gamepad_drop_uses_snapshot_without_synthesizing_an_edge() {
        let mut state = GamepadState::new(gamepad_snapshot(127, 127, 0));
        state.handle(syn(SynchronizationCode::SYN_DROPPED));
        state.handle(axis(AbsoluteAxisCode::ABS_Y, 255));
        state.handle(key(KeyCode::BTN_BASE, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Resynchronize
        );
        state.resynchronize(gamepad_snapshot(127, 255, 1 << 6));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Ignore
        );
        state.handle(axis(AbsoluteAxisCode::ABS_Y, 127));
        state.handle(syn(SynchronizationCode::SYN_REPORT));
        state.handle(axis(AbsoluteAxisCode::ABS_Y, 255));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Report(ControlReport {
                kind: GAMEPAD_REPORT,
                value: GAMEPAD_Y_POSITIVE,
                flags: 0,
            })
        );
    }
}
