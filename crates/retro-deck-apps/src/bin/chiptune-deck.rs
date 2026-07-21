//! Native 60 Hz Deck runtime and bounded diagnostics for the chiptune player.

use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use retro_deck_apps::chiptune::{
    CANVAS_HEIGHT, CANVAS_WIDTH, ChiptuneCatalog, ChiptuneFrame, ChiptunePlayer,
    ChiptunePlayerError, ChiptuneView, PlaybackMode, PlayerContent, PlayerControl, PlayerEffect,
    PlayerModel, RenderError, TrackView, controller_control, touch_control,
};
use retro_deck_audio::{SampleRate, Volume};
use retro_deck_platform::audio::{ApplicationPcm, AudioGate};
use retro_deck_platform::display::{Dimensions, DisplayError, Frame, rgb565_to_xrgb8888};
use retro_deck_platform::file::write_private_atomic;
use retro_deck_platform::input::InputEvent;
use retro_deck_platform::shutdown::ShutdownFlag;
use retro_deck_platform::time::{FrameClock, FrameRate};
use retro_deck_platform::wayland::{
    GameplayBackground, PresentOutcome, WaylandPresentation, WaylandPresentationError,
};

const APPLICATION: &str = "chiptune-deck";
const DEFAULT_DIRECTORY: &str = "/mnt/data/chiptunes";
const DEFAULT_VOLUME_PERCENT: u8 = 42;
const SAMPLE_RATE_HERTZ: u32 = 44_100;
const FRAMES_PER_SECOND: u32 = 60;
const INPUT_EVENT_CAPACITY: usize = 64;
const VOLUME_STATE_MAXIMUM_BYTES: usize = 4;
const RANDOM_SEED: u32 = 0x5244_4348;
const PREVIEW_BLOCKS: usize = 4;
const PROBE_BLOCKS: usize = 60;

fn main() -> ExitCode {
    let Ok(command) = parse_arguments(env::args_os().skip(1)) else {
        print_usage();
        return ExitCode::from(2);
    };

    let result: Result<(), Box<dyn Error>> = match command {
        Command::Run(directory) => PlayerRuntime::start(&directory)
            .and_then(PlayerRuntime::run)
            .map_err(Into::into),
        Command::Probe(path) => probe(&path).map_err(Into::into),
        Command::RenderPreview { input, output } => {
            render_preview(&input, &output).map_err(Into::into)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Run(PathBuf),
    Probe(PathBuf),
    RenderPreview { input: PathBuf, output: PathBuf },
}

fn parse_arguments(mut arguments: impl Iterator<Item = OsString>) -> Result<Command, UsageError> {
    let first = arguments.next();
    let second = arguments.next();
    let third = arguments.next();
    let extra = arguments.next();
    if extra.is_some() {
        return Err(UsageError);
    }
    match (first, second, third) {
        (None, None, None) => Ok(Command::Run(PathBuf::from(DEFAULT_DIRECTORY))),
        (Some(option), Some(path), None) if option == "--probe" => {
            Ok(Command::Probe(PathBuf::from(path)))
        }
        (Some(option), Some(input), Some(output)) if option == "--render-preview" => {
            Ok(Command::RenderPreview {
                input: PathBuf::from(input),
                output: PathBuf::from(output),
            })
        }
        (Some(directory), None, None) if !is_option(&directory) => {
            Ok(Command::Run(PathBuf::from(directory)))
        }
        _ => Err(UsageError),
    }
}

fn is_option(value: &OsStr) -> bool {
    value.as_encoded_bytes().starts_with(b"--")
}

fn print_usage() {
    eprintln!(
        "Usage: {APPLICATION} [CHIPTUNE_DIRECTORY]\n\
         Default: {DEFAULT_DIRECTORY}\n\
         {APPLICATION} --probe CHIPTUNE_FILE\n\
         {APPLICATION} --render-preview CHIPTUNE_FILE OUTPUT.ppm"
    );
}

#[derive(Debug)]
struct PlayerRuntime {
    shutdown: ShutdownFlag,
    input_events: Vec<InputEvent>,
    presentation: WaylandPresentation,
    source_dimensions: Dimensions,
    clock: FrameClock,
    model: PlayerModel,
    player: Option<ChiptunePlayer>,
    status: String,
    frame: ChiptuneFrame,
    audio: Option<ApplicationPcm>,
    audio_gate: AudioGate,
    volume_state: Option<PathBuf>,
    dirty: bool,
}

impl PlayerRuntime {
    fn start(directory: &Path) -> Result<Self, RuntimeError> {
        let shutdown = ShutdownFlag::install().map_err(RuntimeError::Signals)?;
        let source_dimensions = Dimensions::new(CANVAS_WIDTH, CANVAS_HEIGHT)
            .ok_or(RuntimeError::InvalidCanvasDimensions)?;
        let presentation =
            WaylandPresentation::connect_application(source_dimensions, GameplayBackground::Plain)
                .map_err(RuntimeError::Presentation)?;

        let (player, status) = load_catalog(directory);
        let volume = configured_volume();
        let model = PlayerModel::new(volume);
        let initial_view = player_view(model, player.as_ref(), &status);
        let frame = ChiptuneFrame::render(initial_view).map_err(RuntimeError::Render)?;
        let rate = FrameRate::new(FRAMES_PER_SECOND).ok_or(RuntimeError::InvalidFrameRate)?;
        let initial_audio_gate = if volume.muted() {
            AudioGate::Muted
        } else {
            AudioGate::Active
        };

        let mut runtime = Self {
            shutdown,
            input_events: Vec::with_capacity(INPUT_EVENT_CAPACITY),
            presentation,
            source_dimensions,
            clock: FrameClock::start(rate),
            model,
            player,
            status,
            frame,
            audio: start_audio(volume),
            audio_gate: initial_audio_gate,
            volume_state: configured_volume_state(),
            dirty: true,
        };
        runtime.sync_audio_gate();
        Ok(runtime)
    }

    fn run(mut self) -> Result<(), RuntimeError> {
        let result = self.event_loop();
        self.finish_audio();
        result
    }

    fn event_loop(&mut self) -> Result<(), RuntimeError> {
        while !self.shutdown.requested() && !self.presentation.shutdown_requested() {
            self.presentation
                .dispatch_nonblocking()
                .map_err(RuntimeError::Presentation)?;
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                break;
            }
            self.sync_audio_gate();
            if self.handle_input() == LoopControl::Exit {
                break;
            }

            if self.clock.wait_duration().is_zero() {
                self.decode_tick();
                if self.dirty && !self.present_frame()? {
                    break;
                }
                self.clock.complete_frame();
            }
            self.report_audio_errors();

            self.presentation
                .dispatch_with_timeout(self.clock.wait_duration())
                .map_err(RuntimeError::Presentation)?;
        }
        Ok(())
    }

    fn handle_input(&mut self) -> LoopControl {
        self.input_events.clear();
        let dropped = self.presentation.drain_input_into(&mut self.input_events);
        if dropped != 0 {
            eprintln!("{APPLICATION}: discarded {dropped} input event(s) after the bounded drain");
        }

        let mut controls = [None; INPUT_EVENT_CAPACITY];
        for (destination, event) in controls.iter_mut().zip(self.input_events.iter().copied()) {
            *destination = match event {
                InputEvent::TouchPressed(point) => touch_control(point.x(), point.y()),
                InputEvent::Controller { button, edge, .. } => controller_control(button, edge),
            };
        }
        for control in controls.into_iter().flatten() {
            let effect = self.model.apply(control);
            if self.apply_effect(effect) == LoopControl::Exit {
                return LoopControl::Exit;
            }
        }
        LoopControl::Continue
    }

    fn apply_effect(&mut self, effect: PlayerEffect) -> LoopControl {
        match effect {
            PlayerEffect::None => {}
            PlayerEffect::Exit => return LoopControl::Exit,
            PlayerEffect::PreviousFile => self.navigate_file(-1),
            PlayerEffect::NextFile => self.navigate_file(1),
            PlayerEffect::PreviousTrack => self.navigate_track(-1),
            PlayerEffect::NextTrack => self.navigate_track(1),
            PlayerEffect::PauseChanged(_) | PlayerEffect::PlaybackModeChanged(_) => {
                self.dirty = true;
            }
            PlayerEffect::VolumeChanged(volume) => {
                if let Some(audio) = &self.audio {
                    audio.set_volume(volume);
                }
                self.persist_volume(volume);
                self.dirty = true;
            }
        }
        self.sync_audio_gate();
        LoopControl::Continue
    }

    fn navigate_file(&mut self, direction: i8) {
        let result = self
            .player
            .as_mut()
            .ok_or(ChiptunePlayerError::NoFiles)
            .and_then(|player| player.change_file(direction));
        self.finish_navigation(result);
    }

    fn navigate_track(&mut self, direction: i8) {
        let result = self
            .player
            .as_mut()
            .ok_or(ChiptunePlayerError::NoFiles)
            .and_then(|player| player.change_track(direction));
        self.finish_navigation(result);
    }

    fn finish_navigation(&mut self, result: Result<(), ChiptunePlayerError>) {
        match result {
            Ok(()) => self.dirty = true,
            Err(ChiptunePlayerError::SingleTrack) => {}
            Err(error) => eprintln!("{APPLICATION}: cannot change track: {error}"),
        }
    }

    fn decode_tick(&mut self) {
        if self.model.paused() || !self.presentation.visible() {
            return;
        }
        let Some(player) = &mut self.player else {
            return;
        };
        let tick = match player.decode_block() {
            Ok(tick) => tick,
            Err(error) => {
                self.pause_after_decoder_error(&error);
                return;
            }
        };
        if let Some(audio) = &self.audio {
            audio.submit_stereo(player.waveform());
        }
        self.dirty = true;
        if tick.ended() {
            let mode = self.model.playback_mode();
            if let Err(error) = player.advance_after_end(mode) {
                self.pause_after_decoder_error(&error);
            }
        }
    }

    fn pause_after_decoder_error(&mut self, error: &ChiptunePlayerError) {
        eprintln!("{APPLICATION}: playback paused after decoder failure: {error}");
        self.status = bounded_status(format!("PLAYBACK ERROR: {error}"));
        if !self.model.paused() {
            let _effect = self.model.apply(PlayerControl::TogglePause);
        }
        self.dirty = true;
        self.sync_audio_gate();
    }

    fn present_frame(&mut self) -> Result<bool, RuntimeError> {
        let view = player_view(self.model, self.player.as_ref(), &self.status);
        self.frame.redraw(view);
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

    fn sync_audio_gate(&mut self) {
        let gate = desired_audio_gate(
            self.presentation.visible(),
            self.player.is_some(),
            self.model.paused(),
            self.model.volume(),
        );
        if gate == self.audio_gate {
            return;
        }
        self.audio_gate = gate;
        if let Some(audio) = &self.audio {
            audio.set_gate(gate);
        }
    }

    fn persist_volume(&self, volume: Volume) {
        let Some(path) = &self.volume_state else {
            return;
        };
        let value = format!("{}\n", volume.percent());
        if let Err(error) = write_private_atomic(path, value.as_bytes(), VOLUME_STATE_MAXIMUM_BYTES)
        {
            eprintln!("{APPLICATION}: cannot save volume: {error}");
        }
    }

    fn report_audio_errors(&self) {
        if let Some(audio) = &self.audio {
            if let Some(error) = audio.take_error() {
                eprintln!("{APPLICATION}: {error}");
            }
        }
    }

    fn finish_audio(&mut self) {
        self.report_audio_errors();
        let Some(audio) = self.audio.take() else {
            return;
        };
        audio.release();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopControl {
    Continue,
    Exit,
}

fn load_catalog(directory: &Path) -> (Option<ChiptunePlayer>, String) {
    let catalog = match ChiptuneCatalog::scan(directory) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            return (
                None,
                bounded_status(format!("ADD MUSIC TO {}", directory.display())),
            );
        }
    };
    eprintln!(
        "{APPLICATION}: found {} supported file(s) in {}; {} inaccessible; truncated={}",
        catalog.files().len(),
        directory.display(),
        catalog.inaccessible_entries(),
        catalog.truncated()
    );
    match ChiptunePlayer::open(catalog, RANDOM_SEED) {
        Ok(player) => (Some(player), String::new()),
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            (None, bounded_status(format!("CANNOT PLAY FILES: {error}")))
        }
    }
}

fn player_view<'view>(
    model: PlayerModel,
    player: Option<&'view ChiptunePlayer>,
    status: &'view str,
) -> ChiptuneView<'view> {
    let content = player.map_or(PlayerContent::Empty { status }, |player| {
        PlayerContent::Track(TrackView {
            title: player.title(),
            subtitle: player.subtitle(),
            system: player.system(),
            file_index: player.file_index(),
            file_count: player.file_count(),
            track_index: player.track_index(),
            track_count: player.track_count(),
            position_milliseconds: player.position_milliseconds(),
            length_milliseconds: player.length_milliseconds(),
            waveform: player.waveform(),
        })
    });
    ChiptuneView {
        content,
        paused: model.paused(),
        playback_mode: model.playback_mode(),
        volume: model.volume(),
    }
}

fn configured_volume() -> Volume {
    match parse_volume(env::var_os("RETRO_DECK_VOLUME_PERCENT").as_deref()) {
        Ok(volume) => volume,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}; audio muted");
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

fn configured_volume_state() -> Option<PathBuf> {
    let path = env::var_os("RETRO_DECK_VOLUME_STATE").map(PathBuf::from)?;
    if !path.is_absolute() {
        eprintln!(
            "{APPLICATION}: RETRO_DECK_VOLUME_STATE must be absolute; volume changes will not persist"
        );
        return None;
    }
    Some(path)
}

fn start_audio(volume: Volume) -> Option<ApplicationPcm> {
    let Some(rate) = SampleRate::new(SAMPLE_RATE_HERTZ) else {
        eprintln!("{APPLICATION}: internal sample rate is invalid; continuing muted");
        return None;
    };
    match ApplicationPcm::from_inherited(rate, volume) {
        Ok(audio) => Some(audio),
        Err(error) => {
            eprintln!("{APPLICATION}: cannot attach BMC audio: {error}; continuing muted");
            None
        }
    }
}

const fn desired_audio_gate(
    visible: bool,
    playable: bool,
    paused: bool,
    volume: Volume,
) -> AudioGate {
    if !visible {
        AudioGate::Hidden
    } else if !playable || paused {
        AudioGate::Paused
    } else if volume.muted() {
        AudioGate::Muted
    } else {
        AudioGate::Active
    }
}

fn bounded_status(mut status: String) -> String {
    const MAXIMUM_STATUS_BYTES: usize = 512;
    if status.len() <= MAXIMUM_STATUS_BYTES {
        return status;
    }
    let mut boundary = MAXIMUM_STATUS_BYTES;
    while !status.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    status.truncate(boundary);
    status
}

fn probe(path: &Path) -> Result<(), ChiptunePlayerError> {
    let mut player = ChiptunePlayer::open_file(path, RANDOM_SEED)?;
    let mut samples = 0_usize;
    let mut peak = 0_u16;
    for _block in 0..PROBE_BLOCKS {
        let tick = player.decode_block()?;
        samples = samples.saturating_add(tick.frames().saturating_mul(2));
        for frame in player.waveform() {
            peak = peak
                .max(frame[0].unsigned_abs())
                .max(frame[1].unsigned_abs());
        }
        if tick.ended() {
            player.advance_after_end(PlaybackMode::LoopOne)?;
        }
    }
    println!(
        "tracks={} samples={samples} peak={peak}",
        player.track_count()
    );
    Ok(())
}

fn render_preview(input: &Path, output: &Path) -> Result<(), PreviewError> {
    let mut player = ChiptunePlayer::open_file(input, RANDOM_SEED).map_err(PreviewError::Player)?;
    for _block in 0..PREVIEW_BLOCKS {
        let tick = player.decode_block().map_err(PreviewError::Player)?;
        if tick.ended() {
            player
                .advance_after_end(PlaybackMode::LoopOne)
                .map_err(PreviewError::Player)?;
        }
    }
    let volume = Volume::new(DEFAULT_VOLUME_PERCENT).ok_or(PreviewError::InvalidVolume)?;
    let model = PlayerModel::new(volume);
    let frame = ChiptuneFrame::render(player_view(model, Some(&player), ""))
        .map_err(PreviewError::Render)?;
    write_ppm(output, frame.pixels()).map_err(PreviewError::Write)
}

fn write_ppm(path: &Path, pixels: &[u16]) -> io::Result<()> {
    let expected = CANVAS_WIDTH
        .checked_mul(CANVAS_HEIGHT)
        .ok_or_else(|| io::Error::other("preview dimensions overflowed"))?;
    if pixels.len() != expected {
        return Err(io::Error::other("preview pixel count does not match"));
    }
    let file = File::create(path)?;
    let mut output = BufWriter::new(file);
    write!(output, "P6\n{CANVAS_WIDTH} {CANVAS_HEIGHT}\n255\n")?;
    let row_bytes = CANVAS_WIDTH
        .checked_mul(3)
        .ok_or_else(|| io::Error::other("preview row size overflowed"))?;
    let mut encoded = vec![0_u8; row_bytes];
    for row in pixels.chunks_exact(CANVAS_WIDTH) {
        for (destination, source) in encoded.chunks_exact_mut(3).zip(row) {
            let [_, red, green, blue] = rgb565_to_xrgb8888(*source).to_be_bytes();
            destination.copy_from_slice(&[red, green, blue]);
        }
        output.write_all(&encoded)?;
    }
    output.flush()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UsageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VolumeConfigError;

impl fmt::Display for VolumeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("volume must be an integer from 0 through 100")
    }
}

#[derive(Debug)]
enum PreviewError {
    Player(ChiptunePlayerError),
    Render(RenderError),
    Write(io::Error),
    InvalidVolume,
}

impl fmt::Display for PreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Player(source) => write!(formatter, "preview decoder failed: {source}"),
            Self::Render(source) => source.fmt(formatter),
            Self::Write(source) => write!(formatter, "cannot write preview: {source}"),
            Self::InvalidVolume => formatter.write_str("preview volume is invalid"),
        }
    }
}

impl Error for PreviewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Player(source) => Some(source),
            Self::Render(source) => Some(source),
            Self::Write(source) => Some(source),
            Self::InvalidVolume => None,
        }
    }
}

#[derive(Debug)]
enum RuntimeError {
    Signals(io::Error),
    InvalidCanvasDimensions,
    InvalidFrameRate,
    Presentation(WaylandPresentationError),
    Render(RenderError),
    Frame(DisplayError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signals(source) => write!(formatter, "cannot install signal handlers: {source}"),
            Self::InvalidCanvasDimensions => {
                formatter.write_str("chiptune canvas dimensions are invalid")
            }
            Self::InvalidFrameRate => formatter.write_str("chiptune frame rate is invalid"),
            Self::Presentation(source) => source.fmt(formatter),
            Self::Render(source) => source.fmt(formatter),
            Self::Frame(source) => source.fmt(formatter),
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
            Self::InvalidCanvasDimensions | Self::InvalidFrameRate => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments<'arguments>(
        values: &'arguments [&'arguments str],
    ) -> impl Iterator<Item = OsString> + 'arguments {
        values.iter().map(OsString::from)
    }

    #[test]
    fn no_arguments_select_the_deployed_catalog() {
        assert_eq!(
            parse_arguments(arguments(&[])),
            Ok(Command::Run(PathBuf::from(DEFAULT_DIRECTORY)))
        );
    }

    #[test]
    fn runtime_and_diagnostic_arguments_are_exact() {
        assert_eq!(
            parse_arguments(arguments(&["/music"])),
            Ok(Command::Run(PathBuf::from("/music")))
        );
        assert_eq!(
            parse_arguments(arguments(&["--probe", "/music/test.nsf"])),
            Ok(Command::Probe(PathBuf::from("/music/test.nsf")))
        );
        assert_eq!(
            parse_arguments(arguments(&[
                "--render-preview",
                "/music/test.ogg",
                "/tmp/player.ppm"
            ])),
            Ok(Command::RenderPreview {
                input: PathBuf::from("/music/test.ogg"),
                output: PathBuf::from("/tmp/player.ppm")
            })
        );
        assert_eq!(parse_arguments(arguments(&["--probe"])), Err(UsageError));
        assert_eq!(parse_arguments(arguments(&["--unknown"])), Err(UsageError));
        assert_eq!(
            parse_arguments(arguments(&["/one", "/two"])),
            Err(UsageError)
        );
    }

    #[test]
    fn volume_configuration_is_strict_and_defaults_safely() {
        assert_eq!(
            parse_volume(None).map(Volume::percent),
            Ok(DEFAULT_VOLUME_PERCENT)
        );
        assert_eq!(parse_volume(Some(OsStr::new("0"))), Ok(Volume::MUTED));
        assert_eq!(
            parse_volume(Some(OsStr::new("100"))).map(Volume::percent),
            Ok(100)
        );
        assert_eq!(
            parse_volume(Some(OsStr::new("101"))),
            Err(VolumeConfigError)
        );
        assert_eq!(
            parse_volume(Some(OsStr::new(" 42"))),
            Err(VolumeConfigError)
        );
    }

    #[test]
    fn status_truncation_preserves_utf8_boundaries() {
        let input = "ž".repeat(300);
        let output = bounded_status(input);
        assert!(output.len() <= 512);
        assert!(output.is_char_boundary(output.len()));
    }

    #[test]
    fn audio_gate_releases_for_every_inactive_player_state() {
        let audible = Volume::new(42).unwrap_or(Volume::MUTED);
        assert_eq!(
            desired_audio_gate(false, true, false, audible),
            AudioGate::Hidden
        );
        assert_eq!(
            desired_audio_gate(true, false, false, audible),
            AudioGate::Paused
        );
        assert_eq!(
            desired_audio_gate(true, true, true, audible),
            AudioGate::Paused
        );
        assert_eq!(
            desired_audio_gate(true, true, false, Volume::MUTED),
            AudioGate::Muted
        );
        assert_eq!(
            desired_audio_gate(true, true, false, audible),
            AudioGate::Active
        );
    }
}
