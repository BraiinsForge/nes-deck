//! Native Rust Deck runtime for the vendored c-octo CHIP-8 core.

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use retro_deck_audio::{SampleRate, Volume};
use retro_deck_emulator::chip8::{
    Controller, ControllerButton, ControllerState, Core, CoreError, FrameError,
    NORMALIZED_FRAME_HEIGHT, NORMALIZED_FRAME_WIDTH, NormalizedFrame, Program, ProgramError,
};
use retro_deck_platform::audio::{SquareStream, SquareStreamWorker, StreamWorkerReport};
use retro_deck_platform::display::{Dimensions, DisplayError, Frame};
use retro_deck_platform::input::{Button, ButtonSet, Player};
use retro_deck_platform::shutdown::ShutdownFlag;
use retro_deck_platform::time::{FrameClock, FrameRate};
use retro_deck_platform::wayland::{
    GameplayBackground, PresentOutcome, WaylandPresentation, WaylandPresentationError,
};

const APPLICATION: &str = "chip8-deck";
const DEFAULT_VOLUME_PERCENT: u8 = 42;
const AUDIO_SAMPLE_RATE: u32 = 44_100;
const AUDIO_FREQUENCY_HZ: u32 = 440;
const EMULATED_FRAMES_PER_SECOND: u32 = 60;
const DIAGNOSTIC_FRAME_INTERVAL: u64 = 60;

const BUTTON_MAP: [(Button, ControllerButton); 10] = [
    (Button::A, ControllerButton::A),
    (Button::B, ControllerButton::B),
    (Button::Select, ControllerButton::Select),
    (Button::Start, ControllerButton::Start),
    (Button::Up, ControllerButton::Up),
    (Button::Down, ControllerButton::Down),
    (Button::Left, ControllerButton::Left),
    (Button::Right, ControllerButton::Right),
    (Button::L, ControllerButton::L),
    (Button::R, ControllerButton::R),
];

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let _ = arguments.next();
    let Some(rom_path) = arguments.next() else {
        eprintln!("Usage: {APPLICATION} ROM.ch8");
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("Usage: {APPLICATION} ROM.ch8");
        return ExitCode::from(2);
    }

    let rom_path = PathBuf::from(rom_path);
    match Chip8Runtime::start(&rom_path).and_then(Chip8Runtime::run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
struct Chip8Runtime {
    shutdown: ShutdownFlag,
    controller_state: ControllerState,
    presentation: WaylandPresentation,
    source_dimensions: Dimensions,
    core: Core,
    input_profile: retro_deck_emulator::chip8::InputProfile,
    normalized_frame: NormalizedFrame,
    audio: Option<SquareStreamWorker>,
    clock: FrameClock,
    diagnostics: Option<RuntimeDiagnostics>,
}

impl Chip8Runtime {
    fn start(path: &Path) -> Result<Self, RuntimeError> {
        let program = Program::load(path).map_err(RuntimeError::Program)?;
        let configuration = program.configuration();
        let core = Core::new(program.rom(), configuration.core()).map_err(RuntimeError::Core)?;
        let source_dimensions = Dimensions::new(NORMALIZED_FRAME_WIDTH, NORMALIZED_FRAME_HEIGHT)
            .ok_or(RuntimeError::InvalidDimensions)?;
        let background = configured_background(env::var_os("RETRO_DECK_EXIT_HINT").as_deref());
        let presentation = WaylandPresentation::connect_gameplay(source_dimensions, background)
            .map_err(RuntimeError::Presentation)?;
        let shutdown = ShutdownFlag::install().map_err(RuntimeError::Signals)?;
        let volume = configured_volume();
        let frame_rate =
            FrameRate::new(EMULATED_FRAMES_PER_SECOND).ok_or(RuntimeError::InvalidFrameRate)?;

        eprintln!(
            "{APPLICATION}: {}-byte ROM, {} instructions/frame, compositor input, volume {}%",
            program.rom().len(),
            configuration.core().instructions_per_frame(),
            volume.percent()
        );

        Ok(Self {
            shutdown,
            controller_state: ControllerState::default(),
            presentation,
            source_dimensions,
            core,
            input_profile: configuration.input(),
            normalized_frame: NormalizedFrame::new(),
            audio: start_audio(volume),
            clock: FrameClock::start(frame_rate),
            diagnostics: env::var_os("RETRO_DECK_RUNTIME_DIAGNOSTICS")
                .is_some()
                .then(RuntimeDiagnostics::start),
        })
    }

    fn run(mut self) -> Result<(), RuntimeError> {
        let result = self.event_loop();
        self.finish_audio();
        if result.is_ok() && self.core.halted() {
            if let Some(message) = self.core.halt_message() {
                eprintln!("{APPLICATION}: halted: {message}");
            }
        }
        result
    }

    fn event_loop(&mut self) -> Result<(), RuntimeError> {
        while !self.shutdown.requested()
            && !self.presentation.shutdown_requested()
            && !self.core.halted()
        {
            self.presentation
                .dispatch_with_timeout(self.clock.wait_duration())
                .map_err(RuntimeError::Presentation)?;
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                break;
            }
            self.sync_controller_input();
            if !self.clock.wait_duration().is_zero() {
                continue;
            }
            if self.run_frame()? == LoopControl::Exit {
                break;
            }
            self.clock.complete_frame();
            self.report_audio_errors();
        }
        Ok(())
    }

    fn sync_controller_input(&mut self) {
        let dropped = self.presentation.discard_input_events();
        if dropped != 0 {
            eprintln!(
                "{APPLICATION}: discarded {dropped} controller edge(s); complete state remains synchronized"
            );
        }
        self.controller_state = controller_state(&self.presentation);
    }

    fn run_frame(&mut self) -> Result<LoopControl, RuntimeError> {
        self.core
            .set_keypad(self.controller_state.keypad(self.input_profile));
        let outcome = self.core.run_frame();
        if let Some(audio) = &self.audio {
            audio.set_source_active(outcome.sound_active());
        }

        let core_frame = self.core.frame().map_err(RuntimeError::CoreFrame)?;
        self.normalized_frame.update(core_frame);
        let frame = Frame::indexed8(
            self.normalized_frame.pixels(),
            self.source_dimensions,
            NORMALIZED_FRAME_WIDTH,
            self.normalized_frame.palette(),
        )
        .map_err(RuntimeError::Frame)?;
        let control = match self.presentation.present(frame) {
            Ok(PresentOutcome::Submitted | PresentOutcome::Busy) => LoopControl::Continue,
            Err(WaylandPresentationError::SurfaceClosed) => LoopControl::Exit,
            Err(error) => return Err(RuntimeError::Presentation(error)),
        };
        if let Some(diagnostics) = &mut self.diagnostics {
            diagnostics.frame_completed();
        }
        Ok(control)
    }

    fn report_audio_errors(&self) {
        if let Some(audio) = &self.audio {
            for error in audio.take_errors() {
                eprintln!("{APPLICATION}: sound disabled for now: {error}");
            }
        }
    }

    fn finish_audio(&mut self) {
        self.report_audio_errors();
        let Some(audio) = self.audio.take() else {
            return;
        };
        audio.set_source_active(false);
        report_audio_shutdown(audio.shutdown());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopControl {
    Continue,
    Exit,
}

fn controller_state(presentation: &WaylandPresentation) -> ControllerState {
    let mut output = ControllerState::default();
    for (player, controller) in [
        (Player::One, Controller::One),
        (Player::Two, Controller::Two),
    ] {
        apply_buttons(
            &mut output,
            controller,
            presentation.controller_buttons(player),
        );
    }
    output
}

fn apply_buttons(output: &mut ControllerState, controller: Controller, buttons: ButtonSet) {
    for (source, target) in BUTTON_MAP {
        output.set(controller, target, buttons.contains(source));
    }
}

fn configured_volume() -> Volume {
    match parse_volume(env::var_os("RETRO_DECK_VOLUME_PERCENT").as_deref()) {
        Ok(volume) => volume,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}; sound disabled");
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

fn configured_background(value: Option<&OsStr>) -> GameplayBackground {
    match value {
        Some(value) if value.as_encoded_bytes() == b"1" => GameplayBackground::ExitHint,
        Some(_) | None => GameplayBackground::Plain,
    }
}

fn start_audio(volume: Volume) -> Option<SquareStreamWorker> {
    let rate = SampleRate::new(AUDIO_SAMPLE_RATE)?;
    let stream = SquareStream::new(rate, AUDIO_FREQUENCY_HZ)?;
    match SquareStreamWorker::spawn(stream, volume) {
        Ok(worker) => Some(worker),
        Err(error) => {
            eprintln!("{APPLICATION}: cannot start audio worker: {error}; sound disabled");
            None
        }
    }
}

fn report_audio_shutdown(report: StreamWorkerReport) {
    if report.panicked {
        eprintln!("{APPLICATION}: audio worker panicked during shutdown");
    }
    if report.errors != 0 {
        eprintln!(
            "{APPLICATION}: audio worker reported {} error(s), {} diagnostic(s) dropped",
            report.errors, report.dropped_errors
        );
    }
}

#[derive(Debug)]
struct RuntimeDiagnostics {
    started: Instant,
    frames: u64,
}

impl RuntimeDiagnostics {
    fn start() -> Self {
        Self {
            started: Instant::now(),
            frames: 0,
        }
    }

    fn frame_completed(&mut self) {
        self.frames = self.frames.saturating_add(1);
        if self.frames == DIAGNOSTIC_FRAME_INTERVAL {
            eprintln!(
                "{APPLICATION}: diagnostics video={} wall={:.3}",
                self.frames,
                self.started.elapsed().as_secs_f64()
            );
            self.started = Instant::now();
            self.frames = 0;
        }
    }
}

#[derive(Debug)]
enum RuntimeError {
    Program(ProgramError),
    Core(CoreError),
    CoreFrame(FrameError),
    Frame(DisplayError),
    Presentation(WaylandPresentationError),
    Signals(io::Error),
    InvalidDimensions,
    InvalidFrameRate,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Program(source) => source.fmt(formatter),
            Self::Core(source) => source.fmt(formatter),
            Self::CoreFrame(source) => source.fmt(formatter),
            Self::Frame(source) => source.fmt(formatter),
            Self::Presentation(source) => source.fmt(formatter),
            Self::Signals(source) => write!(formatter, "cannot install signal handlers: {source}"),
            Self::InvalidDimensions => formatter.write_str("CHIP-8 frame dimensions are invalid"),
            Self::InvalidFrameRate => formatter.write_str("CHIP-8 frame rate is invalid"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Program(source) => Some(source),
            Self::Core(source) => Some(source),
            Self::CoreFrame(source) => Some(source),
            Self::Frame(source) => Some(source),
            Self::Presentation(source) => Some(source),
            Self::Signals(source) => Some(source),
            Self::InvalidDimensions | Self::InvalidFrameRate => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_volume_is_strict_and_defaults_to_the_dashboard_level() {
        assert_eq!(
            parse_volume(None).map(Volume::percent),
            Ok(DEFAULT_VOLUME_PERCENT)
        );
        assert_eq!(parse_volume(Some(OsStr::new("0"))), Ok(Volume::MUTED));
        assert_eq!(
            parse_volume(Some(OsStr::new("100"))).map(Volume::percent),
            Ok(100)
        );
        for invalid in ["", " 42", "42 ", "+42", "101", "loud"] {
            assert_eq!(
                parse_volume(Some(OsStr::new(invalid))),
                Err(VolumeConfigError)
            );
        }
    }

    #[test]
    fn exit_hint_requires_the_exact_dashboard_contract() {
        assert_eq!(configured_background(None), GameplayBackground::Plain);
        assert_eq!(
            configured_background(Some(OsStr::new("1"))),
            GameplayBackground::ExitHint
        );
        assert_eq!(
            configured_background(Some(OsStr::new("true"))),
            GameplayBackground::Plain
        );
    }
}
