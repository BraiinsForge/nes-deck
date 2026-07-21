//! Native Deck runtime for the 10 Seconds game.

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::Duration;

use retro_deck_apps::ten_seconds::{
    CANVAS_HEIGHT, CANVAS_WIDTH, Cue, InputSource, MonotonicNanoseconds, RenderError, TimerEffect,
    TimerFrame, TimerGame,
};
use retro_deck_audio::{SampleRate, ToneNote, Volume};
use retro_deck_platform::audio::{AudioGate, ToneCuePlayer};
use retro_deck_platform::display::{Dimensions, DisplayError, Frame};
use retro_deck_platform::input::{Button, ButtonEdge, InputEvent};
use retro_deck_platform::shutdown::ShutdownFlag;
use retro_deck_platform::time::MonotonicClock;
use retro_deck_platform::wayland::{
    GameplayBackground, PresentOutcome, WaylandPresentation, WaylandPresentationError,
};
use retro_deck_policy::{
    PolicyClient, PolicyEvent, PolicyEventPoll, PolicySubmit, WorkerCommand, WorkerConfig,
};

const APPLICATION: &str = "ten-seconds-deck";
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(8);
const POLICY_HOOK: &str = "ten-seconds/result";
const DEFAULT_VOLUME_PERCENT: u8 = 42;
const CUE_SAMPLE_RATE: u32 = 44_100;

const ECL_PROGRAM: &str = "/mnt/data/nes-deck/ecl/bin/ecl.bin";
const ECL_DIRECTORY: &str = "/mnt/data/nes-deck/ecl/lib/ecl/";
const LISP_DIRECTORY: &str = "/mnt/data/nes-deck/lisp";
const LISP_WORKER: &str = "/mnt/data/nes-deck/lisp/run-worker.lisp";
const LISP_SITE_DIRECTORY: &str = "/mnt/data/nes-deck/lisp/site.d";

const START_NOTE_SPEC: [(u32, u32); 2] = [(523, 28), (784, 38)];
const EXACT_NOTE_SPEC: [(u32, u32); 3] = [(784, 35), (1_047, 40), (1_319, 55)];
const MISS_NOTE_SPEC: [(u32, u32); 2] = [(659, 35), (440, 55)];

fn main() -> ExitCode {
    if env::args_os().count() != 1 {
        eprintln!("Usage: {APPLICATION}");
        return ExitCode::from(2);
    }

    match TimerRuntime::start().and_then(TimerRuntime::run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
struct TimerRuntime {
    shutdown: ShutdownFlag,
    clock: MonotonicClock,
    input_events: Vec<InputEvent>,
    presentation: WaylandPresentation,
    source_dimensions: Dimensions,
    game: TimerGame,
    frame: TimerFrame,
    audio: Option<ToneCuePlayer<Cue>>,
    policy: Option<PolicyClient>,
    dirty: bool,
}

impl TimerRuntime {
    fn start() -> Result<Self, RuntimeError> {
        let shutdown = ShutdownFlag::install().map_err(RuntimeError::Signals)?;
        let source_dimensions = Dimensions::new(CANVAS_WIDTH, CANVAS_HEIGHT)
            .ok_or(RuntimeError::InvalidCanvasDimensions)?;
        let presentation =
            WaylandPresentation::connect_application(source_dimensions, GameplayBackground::Plain)
                .map_err(RuntimeError::Presentation)?;

        let game = TimerGame::new();
        let frame = TimerFrame::render(game.view()).map_err(RuntimeError::Render)?;
        let volume = configured_volume();

        Ok(Self {
            shutdown,
            clock: MonotonicClock::start(),
            input_events: Vec::with_capacity(64),
            presentation,
            source_dimensions,
            game,
            frame,
            audio: start_audio(volume),
            policy: start_policy(),
            dirty: true,
        })
    }

    fn run(mut self) -> Result<(), RuntimeError> {
        let result = self.event_loop();
        self.finish_audio();
        result
    }

    fn event_loop(&mut self) -> Result<(), RuntimeError> {
        while !self.shutdown.requested() && !self.presentation.shutdown_requested() {
            self.handle_policy_events();
            let now = self.now();
            self.dirty |= self.game.tick(now);

            if self.dirty && !self.present_frame()? {
                break;
            }
            self.report_audio_errors();

            self.presentation
                .dispatch_with_timeout(EVENT_POLL_INTERVAL)
                .map_err(RuntimeError::Presentation)?;
            self.sync_audio_gate();
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                break;
            }
            if self.handle_input() == LoopControl::Exit {
                break;
            }
        }
        Ok(())
    }

    fn now(&self) -> MonotonicNanoseconds {
        MonotonicNanoseconds::new(self.clock.nanoseconds())
    }

    fn present_frame(&mut self) -> Result<bool, RuntimeError> {
        self.frame.redraw(self.game.view());
        let frame = Frame::rgb565(self.frame.pixels(), self.source_dimensions, CANVAS_WIDTH)
            .map_err(RuntimeError::Frame)?;
        match self.presentation.present(frame) {
            Ok(PresentOutcome::Submitted) => {
                self.dirty = false;
                Ok(true)
            }
            Ok(PresentOutcome::Busy) => Ok(true),
            Err(WaylandPresentationError::SurfaceClosed) => Ok(false),
            Err(error) => Err(RuntimeError::Presentation(error)),
        }
    }

    fn handle_input(&mut self) -> LoopControl {
        self.input_events.clear();
        let dropped = self.presentation.drain_input_into(&mut self.input_events);
        if dropped != 0 {
            eprintln!("{APPLICATION}: discarded {dropped} input event(s) after the bounded drain");
        }

        let mut back = false;
        let mut controller = false;
        let mut touch = false;
        for event in self.input_events.iter().copied() {
            match event {
                InputEvent::TouchPressed(point) => {
                    if is_back_press(point.x(), point.y()) {
                        back = true;
                    } else {
                        touch = true;
                    }
                }
                InputEvent::Controller {
                    button: Button::A,
                    edge: ButtonEdge::Pressed,
                    ..
                } => controller = true,
                InputEvent::Controller { .. } => {}
            }
        }

        if back {
            return LoopControl::Exit;
        }
        let activation = if controller {
            Some(InputSource::ControllerA)
        } else if touch {
            Some(InputSource::Touch)
        } else {
            None
        };
        if let Some(source) = activation {
            let effect = self.game.activate(self.now(), source);
            self.dirty = true;
            self.apply_effect(effect);
        }
        LoopControl::Continue
    }

    fn handle_policy_events(&mut self) {
        let event = match &mut self.policy {
            Some(policy) => policy.try_event(),
            None => return,
        };
        match event {
            PolicyEventPoll::Event(PolicyEvent::Response(response)) => {
                let before = self.game.view();
                let effect = self.game.apply_policy_response(&response);
                self.dirty |= self.game.view() != before;
                self.apply_effect(effect);
            }
            PolicyEventPoll::Event(PolicyEvent::Unavailable(failure)) => {
                self.policy.take();
                eprintln!(
                    "{APPLICATION}: Common Lisp policy unavailable: {failure}; using built-in behavior"
                );
                self.resolve_unavailable_policy();
            }
            PolicyEventPoll::Disconnected => {
                self.policy.take();
                eprintln!(
                    "{APPLICATION}: Common Lisp policy supervisor ended; using built-in behavior"
                );
                self.resolve_unavailable_policy();
            }
            PolicyEventPoll::Empty => {}
        }
    }

    fn resolve_unavailable_policy(&mut self) {
        let before = self.game.view();
        let effect = self.game.policy_unavailable();
        self.dirty |= self.game.view() != before;
        self.apply_effect(effect);
    }

    fn apply_effect(&mut self, mut effect: TimerEffect) {
        loop {
            effect = match effect {
                TimerEffect::None => return,
                TimerEffect::PlayCue(cue) => {
                    if cue == Cue::Start && self.policy.is_none() {
                        self.policy = start_policy();
                    }
                    if let Some(audio) = &self.audio {
                        audio.play(cue);
                    }
                    return;
                }
                TimerEffect::SubmitPolicy(arguments) => {
                    let submission = match &mut self.policy {
                        Some(policy) => policy.try_submit(POLICY_HOOK, arguments),
                        None => Ok(PolicySubmit::Unavailable),
                    };
                    match submission {
                        Ok(outcome) => self.game.policy_submitted(outcome),
                        Err(error) => {
                            eprintln!(
                                "{APPLICATION}: cannot encode Common Lisp policy request: {error}; using built-in behavior"
                            );
                            self.game.policy_not_submitted()
                        }
                    }
                }
            };
        }
    }

    fn report_audio_errors(&self) {
        if let Some(audio) = &self.audio
            && let Some(error) = audio.take_error()
        {
            eprintln!("{APPLICATION}: {error}");
        }
    }

    fn sync_audio_gate(&self) {
        if let Some(audio) = &self.audio {
            audio.set_gate(if self.presentation.visible() {
                AudioGate::Active
            } else {
                AudioGate::Hidden
            });
        }
    }

    fn finish_audio(&mut self) {
        self.report_audio_errors();
        if let Some(audio) = self.audio.take() {
            audio.release();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopControl {
    Continue,
    Exit,
}

const fn is_back_press(x: u16, y: u16) -> bool {
    x >= 16 && x < 168 && y >= 16 && y < 80
}

fn configured_volume() -> Volume {
    match parse_volume(env::var_os("RETRO_DECK_VOLUME_PERCENT").as_deref()) {
        Ok(volume) => volume,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}; game cues disabled");
            Volume::MUTED
        }
    }
}

fn parse_volume(value: Option<&OsStr>) -> Result<Volume, VolumeConfigError> {
    let Some(value) = value else {
        return Volume::new(DEFAULT_VOLUME_PERCENT).ok_or(VolumeConfigError);
    };
    let Some(text) = value.to_str() else {
        return Err(VolumeConfigError);
    };
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VolumeConfigError);
    }
    let percent = text.parse::<u8>().map_err(|_| VolumeConfigError)?;
    Volume::new(percent).ok_or(VolumeConfigError)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VolumeConfigError;

impl fmt::Display for VolumeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("volume must be an integer from 0 through 100")
    }
}

fn start_audio(volume: Volume) -> Option<ToneCuePlayer<Cue>> {
    let Some(rate) = SampleRate::new(CUE_SAMPLE_RATE) else {
        eprintln!("{APPLICATION}: internal cue sample rate is invalid; game cues disabled");
        return None;
    };
    let cues = [
        (Cue::Start, timer_notes(Cue::Start)),
        (Cue::Exact, timer_notes(Cue::Exact)),
        (Cue::Miss, timer_notes(Cue::Miss)),
    ];
    match ToneCuePlayer::from_inherited(rate, volume, cues) {
        Ok(player) => Some(player),
        Err(error) => {
            eprintln!("{APPLICATION}: cannot prepare game cues: {error}; game cues disabled");
            None
        }
    }
}

fn timer_notes(cue: Cue) -> &'static [ToneNote] {
    static START: OnceLock<Vec<ToneNote>> = OnceLock::new();
    static EXACT: OnceLock<Vec<ToneNote>> = OnceLock::new();
    static MISS: OnceLock<Vec<ToneNote>> = OnceLock::new();

    match cue {
        Cue::Start => START.get_or_init(|| validated_notes(&START_NOTE_SPEC)),
        Cue::Exact => EXACT.get_or_init(|| validated_notes(&EXACT_NOTE_SPEC)),
        Cue::Miss => MISS.get_or_init(|| validated_notes(&MISS_NOTE_SPEC)),
    }
    .as_slice()
}

fn validated_notes(specification: &[(u32, u32)]) -> Vec<ToneNote> {
    specification
        .iter()
        .filter_map(|(frequency, duration)| ToneNote::new(*frequency, *duration))
        .collect()
}

fn start_policy() -> Option<PolicyClient> {
    let command = WorkerCommand::new(ECL_PROGRAM)
        .arg("--norc")
        .arg("--shell")
        .arg(LISP_WORKER)
        .env("ECLDIR", ECL_DIRECTORY)
        .env("RETRO_DECK_LISP_SITE_DIR", LISP_SITE_DIRECTORY)
        .current_dir(LISP_DIRECTORY);
    match PolicyClient::spawn(WorkerConfig::new(command)) {
        Ok(policy) => Some(policy),
        Err(error) => {
            eprintln!(
                "{APPLICATION}: cannot start Common Lisp policy supervisor: {error}; using built-in behavior"
            );
            None
        }
    }
}

#[derive(Debug)]
enum RuntimeError {
    Signals(io::Error),
    InvalidCanvasDimensions,
    Presentation(WaylandPresentationError),
    Render(RenderError),
    Frame(DisplayError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signals(source) => write!(formatter, "cannot install signal handlers: {source}"),
            Self::InvalidCanvasDimensions => {
                formatter.write_str("timer canvas dimensions are invalid")
            }
            Self::Presentation(source) => write!(formatter, "{source}"),
            Self::Render(source) => write!(formatter, "{source}"),
            Self::Frame(source) => write!(formatter, "cannot construct timer frame: {source}"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Signals(source) => Some(source),
            Self::Presentation(source) => Some(source),
            Self::Render(source) => Some(source),
            Self::Frame(source) => Some(source),
            Self::InvalidCanvasDimensions => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt as _;

    #[test]
    fn volume_configuration_is_strict_and_defaults_to_42() {
        assert_eq!(parse_volume(None).map(Volume::percent), Ok(42));
        assert_eq!(
            parse_volume(Some(OsStr::new("0"))).map(Volume::percent),
            Ok(0)
        );
        assert_eq!(
            parse_volume(Some(OsStr::new("100"))).map(Volume::percent),
            Ok(100)
        );
        for invalid in ["", " 42", "+42", "101", "256", "quiet"] {
            assert_eq!(
                parse_volume(Some(OsStr::new(invalid))),
                Err(VolumeConfigError)
            );
        }
        assert_eq!(
            parse_volume(Some(OsStr::from_bytes(&[0xff]))),
            Err(VolumeConfigError)
        );
    }

    #[test]
    fn back_control_matches_the_rendered_top_left_button() {
        assert!(is_back_press(16, 16));
        assert!(is_back_press(167, 79));
        assert!(!is_back_press(15, 16));
        assert!(!is_back_press(168, 16));
        assert!(!is_back_press(16, 15));
        assert!(!is_back_press(16, 80));
    }

    #[test]
    fn every_timer_cue_has_its_complete_validated_note_sequence() {
        assert_eq!(timer_notes(Cue::Start).len(), START_NOTE_SPEC.len());
        assert_eq!(timer_notes(Cue::Exact).len(), EXACT_NOTE_SPEC.len());
        assert_eq!(timer_notes(Cue::Miss).len(), MISS_NOTE_SPEC.len());
        assert_eq!(
            timer_notes(Cue::Start)
                .first()
                .map(|note| note.frequency_hz()),
            Some(523)
        );
        assert_eq!(
            timer_notes(Cue::Exact)
                .get(2)
                .map(|note| note.duration_ms()),
            Some(55)
        );
        assert_eq!(
            timer_notes(Cue::Miss)
                .get(1)
                .map(|note| note.frequency_hz()),
            Some(440)
        );
    }
}
