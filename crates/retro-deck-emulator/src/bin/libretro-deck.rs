//! Rust runtime shared by the statically linked NES, Game Boy, and ZX cores.

use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use retro_deck_audio::Volume;
use retro_deck_emulator::libretro::{
    AudioBatchError, Content, ContentError, CoreSession, CoreSessionError, JoypadState,
    LibretroCore, PersistenceIssue, VideoCallbackError,
};
use retro_deck_platform::audio::{AudioGate, PcmStreamWorker, PcmWorkerReport};
use retro_deck_platform::input::{KeyboardState, MediumRawKeyboard, Player};
use retro_deck_platform::shutdown::ShutdownFlag;
use retro_deck_platform::time::FrameClock;
use retro_deck_platform::wayland::{
    GameplayBackground, WaylandPresentation, WaylandPresentationError,
};

const DEFAULT_VOLUME_PERCENT: u8 = 42;
const SAVE_INTERVAL: Duration = Duration::from_secs(10);
const AUDIO_REPORT_INTERVAL: Duration = Duration::from_secs(1);

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments
        .next()
        .unwrap_or_else(|| OsString::from("libretro-deck"));
    let Some(core) = core_for_executable(&executable) else {
        eprintln!("libretro-deck: install this executable as nes-deck, gb-deck, or zx-deck");
        return ExitCode::from(2);
    };
    let application = core.frontend_name();
    let Some(content_path) = arguments.next() else {
        print_usage(application, core);
        return ExitCode::from(2);
    };
    if arguments.next().is_some() {
        print_usage(application, core);
        return ExitCode::from(2);
    }

    let content_path = PathBuf::from(content_path);
    match Runtime::start(core, &content_path).and_then(Runtime::run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{application}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage(application: &str, core: LibretroCore) {
    let extensions = core.extensions().join("|");
    eprintln!("Usage: {application} ROM.({extensions})");
}

fn core_for_executable(executable: &OsStr) -> Option<LibretroCore> {
    match Path::new(executable).file_name()?.as_encoded_bytes() {
        b"nes-deck" => Some(LibretroCore::Fceumm),
        b"gb-deck" => Some(LibretroCore::Gambatte),
        b"zx-deck" => Some(LibretroCore::Fuse),
        _ => None,
    }
}

#[derive(Debug)]
struct Runtime {
    application: &'static str,
    shutdown: ShutdownFlag,
    keyboard: Option<MediumRawKeyboard>,
    session: CoreSession,
    clock: FrameClock,
    next_save: Instant,
    next_audio_report: Instant,
    muted: bool,
    audio_gate: AudioGate,
}

impl Runtime {
    fn start(core: LibretroCore, path: &Path) -> Result<Self, RuntimeError> {
        let application = core.frontend_name();
        let content = Content::load(core, path).map_err(RuntimeError::Content)?;
        let mut session = CoreSession::open(core, content).map_err(RuntimeError::Session)?;
        let av_info = session.av_info();
        let background = configured_background(env::var_os("RETRO_DECK_EXIT_HINT").as_deref());
        let presentation =
            WaylandPresentation::connect_gameplay(av_info.source_dimensions(), background)
                .map_err(RuntimeError::Presentation)?;
        if session.attach_presentation(presentation).is_some() {
            return Err(RuntimeError::ResourceAlreadyAttached("presentation"));
        }

        let volume = configured_volume(application);
        match PcmStreamWorker::spawn(av_info.sample_rate(), volume) {
            Ok(worker) => {
                if let Some(worker) = session.attach_audio_worker(worker) {
                    let _report = worker.shutdown();
                    return Err(RuntimeError::ResourceAlreadyAttached("audio worker"));
                }
            }
            Err(error) => {
                eprintln!("{application}: cannot start audio worker: {error}; sound disabled");
            }
        }

        let shutdown = ShutdownFlag::install().map_err(RuntimeError::Signals)?;
        let keyboard = discover_keyboard(application);
        let clock = FrameClock::start_period(av_info.frame_period())
            .ok_or(RuntimeError::InvalidFramePeriod)?;
        for issue in session.persistence_issues() {
            eprintln!("{application}: native save not loaded: {issue}");
        }
        eprintln!(
            "{application}: {} {}, {:.4} fps, {} Hz, compositor input, volume {}%",
            session.metadata().name(),
            session.metadata().version(),
            av_info.frames_per_second(),
            av_info.sample_rate().get(),
            volume.percent()
        );

        let now = Instant::now();
        Ok(Self {
            application,
            shutdown,
            keyboard,
            session,
            clock,
            next_save: now + SAVE_INTERVAL,
            next_audio_report: now + AUDIO_REPORT_INTERVAL,
            muted: volume.muted(),
            audio_gate: desired_audio_gate(true, volume.muted()),
        })
    }

    fn run(mut self) -> Result<(), RuntimeError> {
        let result = self.event_loop();
        self.finish_audio();
        self.finish_keyboard();
        self.save("final");
        result
    }

    fn event_loop(&mut self) -> Result<(), RuntimeError> {
        while !self.shutdown.requested() && !self.presentation()?.shutdown_requested() {
            let timeout = self.clock.wait_duration();
            if self.shutdown.requested() {
                break;
            }
            match self.presentation_mut()?.dispatch_with_timeout(timeout) {
                Ok(_) => {}
                Err(WaylandPresentationError::SurfaceClosed) => break,
                Err(error) => return Err(RuntimeError::Presentation(error)),
            }
            if self.presentation()?.shutdown_requested() {
                break;
            }

            self.drain_controller_input()?;
            self.drain_keyboard();
            self.update_audio_gate();
            if !self.clock.wait_duration().is_zero() {
                self.report_audio_errors_if_due();
                continue;
            }

            let player_one = self.presentation()?.controller_buttons(Player::One);
            let player_two = self.presentation()?.controller_buttons(Player::Two);
            self.session.set_input(
                JoypadState::from_buttons(player_one),
                JoypadState::from_buttons(player_two),
                self.keyboard
                    .as_ref()
                    .map_or_else(KeyboardState::empty, MediumRawKeyboard::state),
            );
            let (audio_error, video_error) = self.session.run_frame().into_errors();
            if let Some(error) = audio_error {
                return Err(RuntimeError::AudioCallback(error));
            }
            match video_error {
                Some(VideoCallbackError::Presentation(WaylandPresentationError::SurfaceClosed)) => {
                    break;
                }
                Some(error) => return Err(RuntimeError::VideoCallback(error)),
                None => {}
            }
            self.clock.complete_frame();
            self.save_if_due();
            self.report_audio_errors_if_due();
        }
        Ok(())
    }

    fn presentation(&self) -> Result<&WaylandPresentation, RuntimeError> {
        self.session
            .presentation()
            .ok_or(RuntimeError::MissingResource("presentation"))
    }

    fn presentation_mut(&mut self) -> Result<&mut WaylandPresentation, RuntimeError> {
        self.session
            .presentation_mut()
            .ok_or(RuntimeError::MissingResource("presentation"))
    }

    fn drain_controller_input(&mut self) -> Result<(), RuntimeError> {
        let dropped = self.presentation_mut()?.discard_input_events();
        if dropped != 0 {
            eprintln!(
                "{}: discarded {} controller edge(s); complete state remains synchronized",
                self.application, dropped
            );
        }
        Ok(())
    }

    fn drain_keyboard(&mut self) {
        let failure = self
            .keyboard
            .as_mut()
            .and_then(|keyboard| keyboard.drain().err());
        if let Some(error) = failure {
            eprintln!(
                "{}: keyboard input disabled after read failure: {error}",
                self.application
            );
            let _keyboard = self.keyboard.take();
        }
    }

    fn update_audio_gate(&mut self) {
        let visible = self.presentation().is_ok_and(WaylandPresentation::visible);
        let gate = desired_audio_gate(visible, self.muted);
        if gate == self.audio_gate {
            return;
        }
        self.audio_gate = gate;
        if let Some(worker) = self.session.audio_worker() {
            worker.set_gate(gate);
        }
    }

    fn save_if_due(&mut self) {
        let now = Instant::now();
        if now < self.next_save {
            return;
        }
        self.next_save = now + SAVE_INTERVAL;
        self.save("periodic");
    }

    fn save(&self, phase: &str) {
        for issue in self.session.save_persistent_memory() {
            if matches!(issue, PersistenceIssue::WriteBlocked { .. }) {
                continue;
            }
            eprintln!("{}: {phase} save problem: {issue}", self.application);
        }
    }

    fn report_audio_errors_if_due(&mut self) {
        let now = Instant::now();
        if now < self.next_audio_report {
            return;
        }
        self.next_audio_report = now + AUDIO_REPORT_INTERVAL;
        self.report_audio_errors();
    }

    fn report_audio_errors(&self) {
        let Some(worker) = self.session.audio_worker() else {
            return;
        };
        for error in worker.take_errors() {
            eprintln!("{}: sound unavailable for now: {error}", self.application);
        }
    }

    fn finish_audio(&mut self) {
        self.report_audio_errors();
        let Some(worker) = self.session.take_audio_worker() else {
            return;
        };
        worker.set_gate(AudioGate::Hidden);
        report_audio_shutdown(self.application, worker.shutdown());
    }

    fn finish_keyboard(&mut self) {
        let Some(keyboard) = self.keyboard.take() else {
            return;
        };
        if let Err(error) = keyboard.restore() {
            eprintln!(
                "{}: keyboard mode restoration failed: {error}",
                self.application
            );
        }
    }
}

fn discover_keyboard(application: &str) -> Option<MediumRawKeyboard> {
    match MediumRawKeyboard::discover() {
        Ok(keyboard) => {
            eprintln!(
                "{application}: physical keyboard enabled through {}",
                keyboard.path().display()
            );
            Some(keyboard)
        }
        Err(error) => {
            eprintln!("{application}: {error}; continuing without physical keyboard input");
            None
        }
    }
}

fn configured_volume(application: &str) -> Volume {
    match parse_volume(env::var_os("RETRO_DECK_VOLUME_PERCENT").as_deref()) {
        Ok(volume) => volume,
        Err(error) => {
            eprintln!("{application}: {error}; sound disabled");
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

fn configured_background(value: Option<&OsStr>) -> GameplayBackground {
    match value {
        Some(value) if value.as_encoded_bytes() == b"1" => GameplayBackground::ExitHint,
        Some(_) | None => GameplayBackground::Plain,
    }
}

const fn desired_audio_gate(visible: bool, muted: bool) -> AudioGate {
    if !visible {
        AudioGate::Hidden
    } else if muted {
        AudioGate::Muted
    } else {
        AudioGate::Active
    }
}

fn report_audio_shutdown(application: &str, report: PcmWorkerReport) {
    if report.panicked {
        eprintln!("{application}: audio worker panicked during shutdown");
    }
    if report.errors != 0 || report.dropped_errors != 0 {
        eprintln!(
            "{application}: audio worker stopped with {} error(s), including {} unreported",
            report.errors, report.dropped_errors
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VolumeConfigError;

impl fmt::Display for VolumeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("volume must be an integer from 0 through 100")
    }
}

#[derive(Debug)]
enum RuntimeError {
    Content(ContentError),
    Session(CoreSessionError),
    Signals(io::Error),
    Presentation(WaylandPresentationError),
    AudioCallback(AudioBatchError),
    VideoCallback(VideoCallbackError),
    InvalidFramePeriod,
    ResourceAlreadyAttached(&'static str),
    MissingResource(&'static str),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Content(source) => source.fmt(formatter),
            Self::Session(source) => source.fmt(formatter),
            Self::Signals(source) => write!(formatter, "cannot install signal handlers: {source}"),
            Self::Presentation(source) => source.fmt(formatter),
            Self::AudioCallback(source) => source.fmt(formatter),
            Self::VideoCallback(source) => source.fmt(formatter),
            Self::InvalidFramePeriod => formatter.write_str("core frame period is zero"),
            Self::ResourceAlreadyAttached(resource) => {
                write!(formatter, "core session already owns its {resource}")
            }
            Self::MissingResource(resource) => {
                write!(formatter, "core session lost its required {resource}")
            }
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Content(source) => Some(source),
            Self::Session(source) => Some(source),
            Self::Signals(source) => Some(source),
            Self::Presentation(source) => Some(source),
            Self::AudioCallback(source) => Some(source),
            Self::VideoCallback(source) => Some(source),
            Self::InvalidFramePeriod
            | Self::ResourceAlreadyAttached(_)
            | Self::MissingResource(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt as _;

    #[test]
    fn installed_name_selects_exactly_one_core() {
        assert_eq!(
            core_for_executable(OsStr::new("/mnt/data/nes-deck/nes-deck")),
            Some(LibretroCore::Fceumm)
        );
        assert_eq!(
            core_for_executable(OsStr::new("gb-deck")),
            Some(LibretroCore::Gambatte)
        );
        assert_eq!(
            core_for_executable(OsStr::new("zx-deck")),
            Some(LibretroCore::Fuse)
        );
        assert_eq!(core_for_executable(OsStr::new("libretro-deck")), None);
        assert_eq!(core_for_executable(OsStr::from_bytes(&[0xff])), None);
    }

    #[test]
    fn volume_is_strict_and_defaults_to_the_dashboard_level() {
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
        assert_eq!(
            parse_volume(Some(OsStr::from_bytes(&[0xff]))),
            Err(VolumeConfigError)
        );
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

    #[test]
    fn visibility_never_forgets_that_volume_is_muted() {
        assert_eq!(desired_audio_gate(true, false), AudioGate::Active);
        assert_eq!(desired_audio_gate(false, false), AudioGate::Hidden);
        assert_eq!(desired_audio_gate(false, true), AudioGate::Hidden);
        assert_eq!(desired_audio_gate(true, true), AudioGate::Muted);
    }
}
