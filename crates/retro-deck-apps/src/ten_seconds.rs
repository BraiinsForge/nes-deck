//! Deterministic 10 Seconds game state and bounded policy contract.

use retro_deck_policy::{PolicyResponse, PolicySubmit, RequestId, Value};

const TARGET_CENTISECONDS: u16 = 1_000;
const MAXIMUM_CENTISECONDS: u16 = 9_999;
const NANOSECONDS_PER_CENTISECOND: u64 = 10_000_000;

/// Timer value accepted by the game and policy boundary.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Centiseconds(u16);

impl Centiseconds {
    /// The exact game target.
    pub const TARGET: Self = Self(TARGET_CENTISECONDS);

    /// Construct a value representable by the four-digit display.
    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        if value <= MAXIMUM_CENTISECONDS {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Return the bounded integer value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Format the fixed-width `SS.CC` display text.
    #[must_use]
    pub fn display_text(self) -> String {
        format!("{:02}.{:02}", self.0 / 100, self.0 % 100)
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "the preceding bound proves the value fits in u16"
    )]
    const fn clamped(value: u64) -> Self {
        if value > MAXIMUM_CENTISECONDS as u64 {
            Self(MAXIMUM_CENTISECONDS)
        } else {
            Self(value as u16)
        }
    }
}

/// Monotonic instant supplied by the platform clock.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MonotonicNanoseconds(u64);

impl MonotonicNanoseconds {
    /// Wrap one monotonic nanosecond count.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw monotonic count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// User input that may finish a timer run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputSource {
    /// A touchscreen press outside the back control.
    Touch,
    /// Physical A on a supported controller.
    ControllerA,
}

impl InputSource {
    const fn policy_name(self) -> &'static str {
        match self {
            Self::Touch => "touch",
            Self::ControllerA => "controller-a",
        }
    }
}

/// Short sound selected without performing audio I/O on the input path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cue {
    /// A new run began.
    Start,
    /// The displayed result is exactly 10.00.
    Exact,
    /// The displayed result is early or late.
    Miss,
}

/// Coarse state rendered by the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimerPhase {
    /// Waiting for the first activation.
    Ready,
    /// Monotonic time is advancing.
    Running,
    /// A result is visible.
    Stopped,
}

/// Complete device-independent view of the timer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerView {
    phase: TimerPhase,
    displayed: Centiseconds,
}

impl TimerView {
    /// Current timer phase.
    #[must_use]
    pub const fn phase(self) -> TimerPhase {
        self.phase
    }

    /// Value shown on the display.
    #[must_use]
    pub const fn displayed(self) -> Centiseconds {
        self.displayed
    }
}

/// Validated display and sound decision for a completed run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerDecision {
    displayed: Centiseconds,
    cue: Cue,
}

impl TimerDecision {
    /// Display value selected by built-in or trusted policy.
    #[must_use]
    pub const fn displayed(self) -> Centiseconds {
        self.displayed
    }

    /// Finite sound selected by built-in or trusted policy.
    #[must_use]
    pub const fn cue(self) -> Cue {
        self.cue
    }

    const fn fallback(elapsed: Centiseconds) -> Self {
        Self {
            displayed: elapsed,
            cue: if elapsed.0 == TARGET_CENTISECONDS {
                Cue::Exact
            } else {
                Cue::Miss
            },
        }
    }
}

/// Nonblocking work emitted by a game transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimerEffect {
    /// No external work is required.
    None,
    /// Try to enqueue a small cue identifier for the audio worker.
    PlayCue(Cue),
    /// Try to submit these bounded arguments to `:ten-seconds/result`.
    SubmitPolicy(Value),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Ready,
    Running { started_at: MonotonicNanoseconds },
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingDecision {
    request_id: Option<RequestId>,
    fallback: TimerDecision,
}

/// Deterministic game state with asynchronous policy fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimerGame {
    state: State,
    displayed: Centiseconds,
    pending: Option<PendingDecision>,
}

impl TimerGame {
    /// Construct a ready game at 00.00.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Ready,
            displayed: Centiseconds(0),
            pending: None,
        }
    }

    /// Return the complete render state.
    #[must_use]
    pub const fn view(&self) -> TimerView {
        TimerView {
            phase: match self.state {
                State::Ready => TimerPhase::Ready,
                State::Running { .. } => TimerPhase::Running,
                State::Stopped => TimerPhase::Stopped,
            },
            displayed: self.displayed,
        }
    }

    /// Start, stop, or reset the timer at a supplied monotonic instant.
    ///
    /// Starting emits only a cheap cue identifier. Stopping updates the honest
    /// built-in display immediately and emits a bounded policy request. The
    /// caller submits that request without waiting and reports the outcome
    /// through [`Self::policy_submitted`] or [`Self::policy_not_submitted`].
    pub fn activate(&mut self, now: MonotonicNanoseconds, input: InputSource) -> TimerEffect {
        if let State::Running { started_at } = self.state {
            let elapsed = elapsed_centiseconds(started_at, now);
            let fallback = TimerDecision::fallback(elapsed);
            self.state = State::Stopped;
            self.displayed = fallback.displayed;
            self.pending = Some(PendingDecision {
                request_id: None,
                fallback,
            });
            TimerEffect::SubmitPolicy(policy_arguments(elapsed, input))
        } else {
            self.state = State::Running { started_at: now };
            self.displayed = Centiseconds(0);
            self.pending = None;
            TimerEffect::PlayCue(Cue::Start)
        }
    }

    /// Advance the visible running value without changing game phase.
    ///
    /// Returns true only when a redraw-visible value changed.
    pub fn tick(&mut self, now: MonotonicNanoseconds) -> bool {
        let State::Running { started_at } = self.state else {
            return false;
        };
        let displayed = elapsed_centiseconds(started_at, now);
        if displayed == self.displayed {
            return false;
        }
        self.displayed = displayed;
        true
    }

    /// Record the result of a nonblocking policy submission.
    ///
    /// A full or unavailable queue immediately selects the safe built-in cue.
    /// A queued request leaves sound pending while the UI remains responsive.
    pub const fn policy_submitted(&mut self, submission: PolicySubmit) -> TimerEffect {
        match submission {
            PolicySubmit::Queued(request_id) => {
                if let Some(pending) = &mut self.pending {
                    if pending.request_id.is_none() {
                        pending.request_id = Some(request_id);
                    }
                }
                TimerEffect::None
            }
            PolicySubmit::DroppedFull | PolicySubmit::Unavailable => self.resolve_with_fallback(),
        }
    }

    /// Resolve a result safely when request construction or submission fails.
    pub const fn policy_not_submitted(&mut self) -> TimerEffect {
        self.resolve_with_fallback()
    }

    /// Apply one validated worker response without waiting.
    ///
    /// Stale responses from an earlier run are ignored. A hook error or an
    /// invalid result selects the built-in display and cue.
    pub fn apply_policy_response(&mut self, response: &PolicyResponse) -> TimerEffect {
        let Some(pending) = self.pending else {
            return TimerEffect::None;
        };
        if pending.request_id != Some(response.id()) {
            return TimerEffect::None;
        }
        let decision = match response {
            PolicyResponse::Ok { value, .. } => {
                parse_policy_decision(value).unwrap_or(pending.fallback)
            }
            PolicyResponse::Error { .. } => pending.fallback,
        };
        self.pending = None;
        self.displayed = decision.displayed;
        TimerEffect::PlayCue(decision.cue)
    }

    /// Resolve a queued result after the policy worker becomes unavailable.
    pub const fn policy_unavailable(&mut self) -> TimerEffect {
        self.resolve_with_fallback()
    }

    /// Return whether a stopped result still awaits a policy outcome.
    #[must_use]
    pub const fn has_pending_policy(&self) -> bool {
        self.pending.is_some()
    }

    const fn resolve_with_fallback(&mut self) -> TimerEffect {
        let Some(pending) = self.pending.take() else {
            return TimerEffect::None;
        };
        self.displayed = pending.fallback.displayed;
        TimerEffect::PlayCue(pending.fallback.cue)
    }
}

impl Default for TimerGame {
    fn default() -> Self {
        Self::new()
    }
}

/// Failure to interpret a trusted timer policy result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecisionError {
    /// The result is not a property list containing exactly two pairs.
    InvalidShape,
    /// A property name is not a keyword.
    InvalidPropertyName,
    /// A property is not part of the timer contract.
    UnknownProperty,
    /// A property occurs more than once.
    DuplicateProperty,
    /// A required property is absent.
    MissingProperty,
    /// The display value is not an integer from 0 through 9999.
    InvalidDisplay,
    /// The cue is neither `:exact` nor `:miss`.
    InvalidCue,
}

/// Parse the complete result returned by `:ten-seconds/result`.
///
/// # Errors
///
/// Returns [`PolicyDecisionError`] for unknown, repeated, missing, malformed,
/// or out-of-range properties.
pub fn parse_policy_decision(value: &Value) -> Result<TimerDecision, PolicyDecisionError> {
    let Value::List(items) = value else {
        return Err(PolicyDecisionError::InvalidShape);
    };
    if items.len() != 4 {
        return Err(PolicyDecisionError::InvalidShape);
    }

    let mut displayed = None;
    let mut cue = None;
    for pair in items.chunks_exact(2) {
        let [Value::Keyword(name), property] = pair else {
            return Err(PolicyDecisionError::InvalidPropertyName);
        };
        match name.as_str() {
            "display-centiseconds" if displayed.is_none() => {
                let Value::Integer(value) = property else {
                    return Err(PolicyDecisionError::InvalidDisplay);
                };
                displayed = u16::try_from(*value).ok().and_then(Centiseconds::new);
                if displayed.is_none() {
                    return Err(PolicyDecisionError::InvalidDisplay);
                }
            }
            "cue" if cue.is_none() => {
                let Value::Keyword(value) = property else {
                    return Err(PolicyDecisionError::InvalidCue);
                };
                cue = Some(match value.as_str() {
                    "exact" => Cue::Exact,
                    "miss" => Cue::Miss,
                    _ => return Err(PolicyDecisionError::InvalidCue),
                });
            }
            "display-centiseconds" | "cue" => {
                return Err(PolicyDecisionError::DuplicateProperty);
            }
            _ => return Err(PolicyDecisionError::UnknownProperty),
        }
    }

    Ok(TimerDecision {
        displayed: displayed.ok_or(PolicyDecisionError::MissingProperty)?,
        cue: cue.ok_or(PolicyDecisionError::MissingProperty)?,
    })
}

const fn elapsed_centiseconds(
    started_at: MonotonicNanoseconds,
    now: MonotonicNanoseconds,
) -> Centiseconds {
    let elapsed = now.0.saturating_sub(started_at.0) / NANOSECONDS_PER_CENTISECOND;
    Centiseconds::clamped(elapsed)
}

fn policy_arguments(elapsed: Centiseconds, input: InputSource) -> Value {
    Value::List(vec![
        keyword("elapsed-centiseconds"),
        Value::Integer(i64::from(elapsed.0)),
        keyword("input"),
        keyword(input.policy_name()),
    ])
}

fn keyword(name: &str) -> Value {
    Value::Keyword(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(seconds: u64, centiseconds: u64) -> MonotonicNanoseconds {
        MonotonicNanoseconds::new(
            seconds
                .saturating_mul(1_000_000_000)
                .saturating_add(centiseconds.saturating_mul(NANOSECONDS_PER_CENTISECOND)),
        )
    }

    fn request_id(value: i64) -> RequestId {
        RequestId::new(value).unwrap_or(RequestId::ZERO)
    }

    fn stop(game: &mut TimerGame, centiseconds: u64, input: InputSource) -> TimerEffect {
        assert_eq!(
            game.activate(instant(5, 0), InputSource::Touch),
            TimerEffect::PlayCue(Cue::Start)
        );
        game.activate(instant(5, centiseconds), input)
    }

    #[test]
    fn starts_ticks_and_clamps_on_monotonic_time() {
        let mut game = TimerGame::new();
        assert_eq!(game.view().phase(), TimerPhase::Ready);
        assert_eq!(game.view().displayed(), Centiseconds::default());

        assert_eq!(
            game.activate(instant(8, 0), InputSource::ControllerA),
            TimerEffect::PlayCue(Cue::Start)
        );
        assert_eq!(game.view().phase(), TimerPhase::Running);
        assert!(!game.tick(instant(7, 99)));
        assert!(game.tick(instant(8, 42)));
        assert_eq!(game.view().displayed().get(), 42);
        assert!(game.tick(MonotonicNanoseconds::new(u64::MAX)));
        assert_eq!(game.view().displayed().get(), MAXIMUM_CENTISECONDS);
    }

    #[test]
    fn stopping_updates_honest_display_and_requests_policy() {
        let mut game = TimerGame::new();
        let effect = stop(&mut game, 1_000, InputSource::ControllerA);
        assert_eq!(game.view().phase(), TimerPhase::Stopped);
        assert_eq!(game.view().displayed(), Centiseconds::TARGET);
        assert!(game.has_pending_policy());
        assert_eq!(
            effect,
            TimerEffect::SubmitPolicy(Value::List(vec![
                keyword("elapsed-centiseconds"),
                Value::Integer(1_000),
                keyword("input"),
                keyword("controller-a"),
            ]))
        );
    }

    #[test]
    fn unavailable_policy_uses_the_built_in_cue_without_waiting() {
        for submission in [PolicySubmit::DroppedFull, PolicySubmit::Unavailable] {
            let mut exact = TimerGame::new();
            let _ = stop(&mut exact, 1_000, InputSource::Touch);
            assert_eq!(
                exact.policy_submitted(submission),
                TimerEffect::PlayCue(Cue::Exact)
            );

            let mut miss = TimerGame::new();
            let _ = stop(&mut miss, 999, InputSource::Touch);
            assert_eq!(
                miss.policy_submitted(submission),
                TimerEffect::PlayCue(Cue::Miss)
            );
        }
    }

    #[test]
    fn matching_policy_can_apply_a_trusted_local_skew() {
        let mut game = TimerGame::new();
        let _ = stop(&mut game, 913, InputSource::ControllerA);
        let id = request_id(7);
        assert_eq!(
            game.policy_submitted(PolicySubmit::Queued(id)),
            TimerEffect::None
        );
        let response = PolicyResponse::Ok {
            id,
            value: Value::List(vec![
                keyword("cue"),
                keyword("exact"),
                keyword("display-centiseconds"),
                Value::Integer(1_000),
            ]),
        };
        assert_eq!(
            game.apply_policy_response(&response),
            TimerEffect::PlayCue(Cue::Exact)
        );
        assert_eq!(game.view().displayed(), Centiseconds::TARGET);
        assert!(!game.has_pending_policy());
    }

    #[test]
    fn malformed_or_error_policy_falls_back_safely() {
        for response in [
            PolicyResponse::Ok {
                id: request_id(8),
                value: Value::List(vec![
                    keyword("display-centiseconds"),
                    Value::Integer(10_000),
                    keyword("cue"),
                    keyword("exact"),
                ]),
            },
            PolicyResponse::Error {
                id: request_id(8),
                message: "site hook failed".to_owned(),
            },
        ] {
            let mut game = TimerGame::new();
            let _ = stop(&mut game, 987, InputSource::Touch);
            let _ = game.policy_submitted(PolicySubmit::Queued(request_id(8)));
            assert_eq!(
                game.apply_policy_response(&response),
                TimerEffect::PlayCue(Cue::Miss)
            );
            assert_eq!(game.view().displayed().get(), 987);
        }
    }

    #[test]
    fn stale_policy_response_cannot_change_a_new_run() {
        let mut game = TimerGame::new();
        let _ = stop(&mut game, 987, InputSource::Touch);
        let id = request_id(9);
        let _ = game.policy_submitted(PolicySubmit::Queued(id));
        assert_eq!(
            game.activate(instant(20, 0), InputSource::Touch),
            TimerEffect::PlayCue(Cue::Start)
        );
        let response = PolicyResponse::Ok {
            id,
            value: Value::List(vec![
                keyword("display-centiseconds"),
                Value::Integer(1_000),
                keyword("cue"),
                keyword("exact"),
            ]),
        };
        assert_eq!(game.apply_policy_response(&response), TimerEffect::None);
        assert_eq!(game.view().phase(), TimerPhase::Running);
        assert_eq!(game.view().displayed().get(), 0);
    }

    #[test]
    fn strict_policy_result_rejects_ambiguous_values() {
        for (value, expected) in [
            (Value::Nil, PolicyDecisionError::InvalidShape),
            (
                Value::List(vec![keyword("cue"), keyword("exact")]),
                PolicyDecisionError::InvalidShape,
            ),
            (
                Value::List(vec![
                    keyword("display-centiseconds"),
                    Value::Integer(1_000),
                    keyword("unknown"),
                    keyword("exact"),
                ]),
                PolicyDecisionError::UnknownProperty,
            ),
            (
                Value::List(vec![
                    keyword("cue"),
                    keyword("exact"),
                    keyword("cue"),
                    keyword("miss"),
                ]),
                PolicyDecisionError::DuplicateProperty,
            ),
        ] {
            assert_eq!(parse_policy_decision(&value), Err(expected));
        }
    }

    #[test]
    fn display_format_is_fixed_and_bounded() {
        assert_eq!(Centiseconds::default().display_text(), "00.00");
        assert_eq!(Centiseconds::TARGET.display_text(), "10.00");
        assert_eq!(Centiseconds(MAXIMUM_CENTISECONDS).display_text(), "99.99");
        assert_eq!(Centiseconds::new(10_000), None);
    }
}
