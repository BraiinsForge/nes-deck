//! Product-owned gamepad mapping over compositor-routed raw reports.

use std::{collections::BTreeMap, fmt};

use bmc_widget::surface::{GamepadButtonState, GamepadEvent, GamepadPlayer};
use retro_deck_policy::Value;

use crate::BmcNavigation;

const MAXIMUM_BINDINGS: usize = 32;
const NAVIGATIONS: [BmcNavigation; 6] = [
    BmcNavigation::Up,
    BmcNavigation::Down,
    BmcNavigation::Left,
    BmcNavigation::Right,
    BmcNavigation::Confirm,
    BmcNavigation::Back,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ButtonBinding {
    code: u16,
    navigation: BmcNavigation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AxisBinding {
    code: u16,
    negative: BmcNavigation,
    positive: BmcNavigation,
}

/// Bounded raw-code mapping loaded once from trusted Common Lisp policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GamepadProfile {
    buttons: Vec<ButtonBinding>,
    axes: Vec<AxisBinding>,
}

impl Default for GamepadProfile {
    fn default() -> Self {
        Self {
            // Linux BTN_TRIGGER..BTN_BASE2 codes used by THEGamepad. Both
            // physical face-button pairs intentionally share one menu action.
            buttons: [
                (288, BmcNavigation::Back),
                (289, BmcNavigation::Back),
                (290, BmcNavigation::Confirm),
                (291, BmcNavigation::Confirm),
                (294, BmcNavigation::Back),
                (295, BmcNavigation::Confirm),
            ]
            .into_iter()
            .map(|(code, navigation)| ButtonBinding { code, navigation })
            .collect(),
            axes: [
                (0, BmcNavigation::Left, BmcNavigation::Right),
                (1, BmcNavigation::Up, BmcNavigation::Down),
            ]
            .into_iter()
            .map(|(code, negative, positive)| AxisBinding {
                code,
                negative,
                positive,
            })
            .collect(),
        }
    }
}

impl GamepadProfile {
    /// Decode `(:button CODE :ACTION)` and
    /// `(:axis CODE :NEGATIVE :POSITIVE)` rows from policy data.
    ///
    /// # Errors
    ///
    /// Returns [`GamepadProfileError`] for malformed, excessive, duplicate,
    /// out-of-range, or unknown bindings.
    pub fn from_policy(value: &Value) -> Result<Self, GamepadProfileError> {
        let Value::List(rows) = value else {
            return Err(GamepadProfileError::InvalidShape);
        };
        if rows.len() > MAXIMUM_BINDINGS {
            return Err(GamepadProfileError::TooManyBindings);
        }
        let mut profile = Self {
            buttons: Vec::new(),
            axes: Vec::new(),
        };
        for row in rows {
            let Value::List(fields) = row else {
                return Err(GamepadProfileError::InvalidShape);
            };
            match fields.as_slice() {
                [
                    Value::Keyword(kind),
                    Value::Integer(code),
                    Value::Keyword(action),
                ] if kind == "button" => {
                    let code = policy_code(*code)?;
                    if profile.buttons.iter().any(|binding| binding.code == code) {
                        return Err(GamepadProfileError::DuplicateButton(code));
                    }
                    profile.buttons.push(ButtonBinding {
                        code,
                        navigation: policy_navigation(action)?,
                    });
                }
                [
                    Value::Keyword(kind),
                    Value::Integer(code),
                    Value::Keyword(negative),
                    Value::Keyword(positive),
                ] if kind == "axis" => {
                    let code = policy_code(*code)?;
                    if profile.axes.iter().any(|binding| binding.code == code) {
                        return Err(GamepadProfileError::DuplicateAxis(code));
                    }
                    profile.axes.push(AxisBinding {
                        code,
                        negative: policy_navigation(negative)?,
                        positive: policy_navigation(positive)?,
                    });
                }
                _ => return Err(GamepadProfileError::InvalidShape),
            }
        }
        Ok(profile)
    }
}

fn policy_code(value: i64) -> Result<u16, GamepadProfileError> {
    u16::try_from(value).map_err(|_| GamepadProfileError::InvalidCode)
}

fn policy_navigation(value: &str) -> Result<BmcNavigation, GamepadProfileError> {
    match value {
        "up" => Ok(BmcNavigation::Up),
        "down" => Ok(BmcNavigation::Down),
        "left" => Ok(BmcNavigation::Left),
        "right" => Ok(BmcNavigation::Right),
        "confirm" => Ok(BmcNavigation::Confirm),
        "back" => Ok(BmcNavigation::Back),
        _ => Err(GamepadProfileError::UnknownNavigation(value.to_owned())),
    }
}

/// A Common Lisp gamepad profile violated its closed schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GamepadProfileError {
    /// The outer list or one binding row had the wrong shape or types.
    InvalidShape,
    /// The profile exceeded its fixed binding budget.
    TooManyBindings,
    /// A Linux input code was outside the unsigned 16-bit protocol range.
    InvalidCode,
    /// One raw button code occurred more than once.
    DuplicateButton(u16),
    /// One raw axis code occurred more than once.
    DuplicateAxis(u16),
    /// A binding named behavior outside the dashboard's closed action set.
    UnknownNavigation(String),
}

impl fmt::Display for GamepadProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShape => formatter.write_str("invalid gamepad binding row"),
            Self::TooManyBindings => formatter.write_str("too many gamepad bindings"),
            Self::InvalidCode => formatter.write_str("gamepad code is outside 0 through 65535"),
            Self::DuplicateButton(code) => write!(formatter, "button code {code} is repeated"),
            Self::DuplicateAxis(code) => write!(formatter, "axis code {code} is repeated"),
            Self::UnknownNavigation(action) => {
                write!(formatter, "unknown gamepad navigation :{action}")
            }
        }
    }
}

impl std::error::Error for GamepadProfileError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawAxis {
    range: Option<(i32, i32)>,
    value: i32,
}

#[derive(Debug, Default)]
struct PlayerState {
    buttons: BTreeMap<u16, bool>,
    axes: BTreeMap<u16, RawAxis>,
    reported: u8,
}

impl PlayerState {
    fn set_button(&mut self, code: u16, pressed: bool) {
        self.buttons.insert(code, pressed);
    }

    fn set_axis_info(&mut self, code: u16, minimum: i32, maximum: i32) {
        let range = (maximum > minimum).then_some((minimum, maximum));
        if let Some(axis) = self.axes.get_mut(&code) {
            axis.range = range;
        } else {
            let midpoint = midpoint(minimum, maximum);
            self.axes.insert(
                code,
                RawAxis {
                    range,
                    value: midpoint,
                },
            );
        }
    }

    fn set_axis(&mut self, code: u16, value: i32) {
        if let Some(axis) = self.axes.get_mut(&code) {
            axis.value = value;
        } else {
            self.axes.insert(code, RawAxis { range: None, value });
        }
    }

    fn synchronize_profile(&mut self, profile: &GamepadProfile) {
        self.reported = self.current(profile);
    }

    fn finish_frame(&mut self, profile: &GamepadProfile) -> Vec<BmcNavigation> {
        let current = self.current(profile);
        let pressed = current & !self.reported;
        self.reported = current;
        NAVIGATIONS
            .into_iter()
            .filter(|navigation| pressed & navigation_mask(*navigation) != 0)
            .collect()
    }

    fn current(&self, profile: &GamepadProfile) -> u8 {
        let mut current = 0;
        for binding in &profile.buttons {
            if self.buttons.get(&binding.code).copied().unwrap_or(false) {
                current |= navigation_mask(binding.navigation);
            }
        }
        for binding in &profile.axes {
            let Some(axis) = self.axes.get(&binding.code) else {
                continue;
            };
            current |= axis_navigation(*axis, *binding);
        }
        current
    }
}

/// Per-player report state for compositor-routed gamepad events.
#[derive(Debug, Default)]
pub struct GamepadInput {
    profile: GamepadProfile,
    one: PlayerState,
    two: PlayerState,
}

impl GamepadInput {
    /// Forget raw state after this surface loses compositor focus.
    pub fn reset(&mut self) {
        self.one = PlayerState::default();
        self.two = PlayerState::default();
    }

    /// Replace product policy without synthesizing presses for held controls.
    pub fn set_profile(&mut self, profile: GamepadProfile) {
        self.one.synchronize_profile(&profile);
        self.two.synchronize_profile(&profile);
        self.profile = profile;
    }

    /// Consume one raw protocol event and return newly pressed menu actions.
    #[must_use]
    pub fn handle(&mut self, event: &GamepadEvent) -> Vec<BmcNavigation> {
        let player = match event {
            GamepadEvent::Connected { player, .. }
            | GamepadEvent::AxisInfo { player, .. }
            | GamepadEvent::Disconnected { player }
            | GamepadEvent::Button { player, .. }
            | GamepadEvent::Axis { player, .. }
            | GamepadEvent::Frame { player } => *player,
        };
        let state = match player {
            GamepadPlayer::One => &mut self.one,
            GamepadPlayer::Two => &mut self.two,
        };
        match event {
            GamepadEvent::Connected { .. } | GamepadEvent::Disconnected { .. } => {
                *state = PlayerState::default();
                Vec::new()
            }
            GamepadEvent::AxisInfo {
                code,
                minimum,
                maximum,
                ..
            } => {
                state.set_axis_info(*code, *minimum, *maximum);
                Vec::new()
            }
            GamepadEvent::Button {
                code, state: edge, ..
            } => {
                state.set_button(*code, *edge == GamepadButtonState::Pressed);
                Vec::new()
            }
            GamepadEvent::Axis { code, value, .. } => {
                state.set_axis(*code, *value);
                Vec::new()
            }
            GamepadEvent::Frame { .. } => state.finish_frame(&self.profile),
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the midpoint of two i32 endpoints is necessarily representable as i32"
)]
const fn midpoint(minimum: i32, maximum: i32) -> i32 {
    let value = (minimum as i64 + maximum as i64) / 2;
    value as i32
}

fn axis_navigation(axis: RawAxis, binding: AxisBinding) -> u8 {
    let Some((minimum, maximum)) = axis.range else {
        return 0;
    };
    let span = i64::from(maximum) - i64::from(minimum);
    let low = i64::from(minimum) + span / 3;
    let high = i64::from(maximum) - span / 3;
    if i64::from(axis.value) <= low {
        navigation_mask(binding.negative)
    } else if i64::from(axis.value) >= high {
        navigation_mask(binding.positive)
    } else {
        0
    }
}

const fn navigation_mask(navigation: BmcNavigation) -> u8 {
    1 << match navigation {
        BmcNavigation::Up => 0,
        BmcNavigation::Down => 1,
        BmcNavigation::Left => 2,
        BmcNavigation::Right => 3,
        BmcNavigation::Confirm => 4,
        BmcNavigation::Back => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyword(value: &str) -> Value {
        Value::Keyword(value.to_owned())
    }

    fn row(values: Vec<Value>) -> Value {
        Value::List(values)
    }

    #[test]
    fn policy_profile_is_bounded_and_rejects_duplicates() {
        let profile = GamepadProfile::from_policy(&Value::List(vec![
            row(vec![
                keyword("button"),
                Value::Integer(42),
                keyword("confirm"),
            ]),
            row(vec![
                keyword("axis"),
                Value::Integer(7),
                keyword("left"),
                keyword("right"),
            ]),
        ]));
        assert!(profile.is_ok());
        assert!(matches!(
            GamepadProfile::from_policy(&Value::List(vec![
                row(vec![
                    keyword("button"),
                    Value::Integer(42),
                    keyword("confirm")
                ]),
                row(vec![keyword("button"), Value::Integer(42), keyword("back")]),
            ])),
            Err(GamepadProfileError::DuplicateButton(42))
        ));
    }

    #[test]
    fn raw_reports_emit_only_new_navigation_presses() {
        let mut input = GamepadInput::default();
        let player = GamepadPlayer::One;
        assert!(
            input
                .handle(&GamepadEvent::AxisInfo {
                    player,
                    code: 0,
                    minimum: -32_768,
                    maximum: 32_767,
                })
                .is_empty()
        );
        assert!(
            input
                .handle(&GamepadEvent::Axis {
                    player,
                    code: 0,
                    value: 32_767,
                })
                .is_empty()
        );
        assert_eq!(
            input.handle(&GamepadEvent::Frame { player }),
            [BmcNavigation::Right]
        );
        assert!(input.handle(&GamepadEvent::Frame { player }).is_empty());

        let _ = input.handle(&GamepadEvent::Button {
            player,
            code: 290,
            state: GamepadButtonState::Pressed,
        });
        assert_eq!(
            input.handle(&GamepadEvent::Frame { player }),
            [BmcNavigation::Confirm]
        );
    }
}
