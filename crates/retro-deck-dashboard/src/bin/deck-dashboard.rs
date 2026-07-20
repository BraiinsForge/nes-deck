//! Native Wayland dashboard runtime under staged migration.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::os::fd::AsFd as _;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::time::{Duration, Instant};

use retro_deck_audio::{SampleRate, Volume};
use retro_deck_config::{Catalog, MAXIMUM_CATALOG_BYTES, MAXIMUM_PALETTE_BYTES, Palette};
use retro_deck_dashboard::{
    Action, ArtworkStore, AssetPathError, BrightnessDevicePaths, BrightnessPathError,
    ControllerGuard, CreditsCrawl, DashboardAssetPaths, DashboardAssets, DashboardAssetsError,
    DashboardFrame, DashboardModel, DashboardPreferences, ExitHold, ExitHoldEvent, ExitPolicy,
    Intent, LaunchPlan, LaunchTarget, MenuCue, NetworkView, PreferenceLoad, PreferencePathError,
    PreferencePaths, PreferenceSubmit, PreferenceWorker, PreferenceWorkerReport, RenderError,
    Screen, SettingChange, SettingsView, TerminalMode, TouchCommitter, controller_action,
    menu_notes, parse_volume,
};
use retro_deck_platform::audio::{AudioGate, ToneCueWorker, ToneWorkerReport};
use retro_deck_platform::display::{Dimensions, DisplayError, Frame};
use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};
use retro_deck_platform::input::{ControllerDevices, InputError, InputEvent, TouchscreenDevice};
use retro_deck_platform::process::{ManagedChild, ManagedChildExit};
use retro_deck_platform::shutdown::ShutdownFlag;
use retro_deck_platform::wayland::{PresentOutcome, WaylandPresentation, WaylandPresentationError};

const APPLICATION: &str = "deck-dashboard";
const INPUT_EVENT_CAPACITY: usize = 64;
const IDLE_POLL: Duration = Duration::from_millis(250);
const BUSY_RETRY: Duration = Duration::from_millis(8);
const CREDITS_FRAME: Duration = Duration::from_millis(40);
const CONTROLLER_SCAN: Duration = Duration::from_secs(1);
const CHILD_POLL: Duration = Duration::from_millis(40);
const AUDIO_HANDOFF_TIMEOUT: Duration = Duration::from_secs(2);
const CUE_SAMPLE_RATE: u32 = 44_100;
const COVER_DIRECTORY: &str = "/mnt/data/nes-deck/covers";
const VOLUME_STATE: &str = "/mnt/data/nes-deck/state/menu-volume.state";
const BRIGHTNESS_STATE: &str = "/mnt/data/nes-deck/state/menu-brightness.state";
const KEYMAP_STATE: &str = "/mnt/data/nes-deck/state/terminal-keymap.state";
const BRIGHTNESS_DEVICE: &str = "/sys/class/backlight/display-bl/brightness";
const BRIGHTNESS_MAXIMUM: &str = "/sys/class/backlight/display-bl/max_brightness";
const VOLUME_ENVIRONMENT: &str = "RETRO_DECK_VOLUME_PERCENT";
const KEYMAP_ENVIRONMENT: &str = "RETRO_DECK_KEYMAP";
const EXIT_HINT_ENVIRONMENT: &str = "RETRO_DECK_EXIT_HINT";
const VOLUME_STATE_ENVIRONMENT: &str = "RETRO_DECK_VOLUME_STATE";

fn main() -> ExitCode {
    let command = match parse_arguments(env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}");
            print_usage();
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Help => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Command::GeometryTest => geometry_test(),
        Command::ValidateManifest(path) => validate_manifest(&path),
        Command::ValidatePalette(path) => validate_palette(&path),
        Command::Run(paths) => DashboardRuntime::start(&paths).and_then(DashboardRuntime::run),
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
    Help,
    GeometryTest,
    ValidateManifest(PathBuf),
    ValidatePalette(PathBuf),
    Run(DashboardAssetPaths),
}

fn parse_arguments(arguments: impl Iterator<Item = OsString>) -> Result<Command, UsageError> {
    let arguments = arguments.take(7).collect::<Vec<_>>();
    if arguments.len() > 6 {
        return Err(UsageError::Shape);
    }
    match arguments.as_slice() {
        [option] if option == "--help" => return Ok(Command::Help),
        [option] if option == "--geometry-test" => return Ok(Command::GeometryTest),
        [option, path] if option == "--validate-manifest" => {
            return Ok(Command::ValidateManifest(PathBuf::from(path)));
        }
        [option, path] if option == "--validate-palette" => {
            return Ok(Command::ValidatePalette(PathBuf::from(path)));
        }
        _ => {}
    }

    let mut manifest = None;
    let mut credits = None;
    let mut palette = None;
    let mut pairs = arguments.chunks_exact(2);
    for pair in &mut pairs {
        let [option, value] = pair else {
            return Err(UsageError::Shape);
        };
        let destination = if option == "--manifest" {
            &mut manifest
        } else if option == "--credits" {
            &mut credits
        } else if option == "--palette" {
            &mut palette
        } else {
            return Err(UsageError::Unknown(option.clone()));
        };
        if destination.replace(PathBuf::from(value)).is_some() {
            return Err(UsageError::Duplicate(option.clone()));
        }
    }
    if !pairs.remainder().is_empty() {
        return Err(UsageError::Shape);
    }
    let (Some(manifest), Some(credits), Some(palette)) = (manifest, credits, palette) else {
        return Err(UsageError::MissingRunPaths);
    };
    DashboardAssetPaths::new(manifest, credits, palette)
        .map(Command::Run)
        .map_err(UsageError::Paths)
}

fn print_usage() {
    eprintln!(
        "Usage: {APPLICATION} --manifest PATH --credits PATH --palette PATH\n\
         {APPLICATION} --validate-manifest PATH\n\
         {APPLICATION} --validate-palette PATH\n\
         {APPLICATION} --geometry-test"
    );
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UsageError {
    Shape,
    Unknown(OsString),
    Duplicate(OsString),
    MissingRunPaths,
    Paths(AssetPathError),
}

impl fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape => formatter.write_str("invalid argument shape"),
            Self::Unknown(option) => {
                write!(formatter, "unknown option {}", option.to_string_lossy())
            }
            Self::Duplicate(option) => {
                write!(formatter, "duplicate option {}", option.to_string_lossy())
            }
            Self::MissingRunPaths => {
                formatter.write_str("--manifest, --credits, and --palette are required")
            }
            Self::Paths(error) => error.fmt(formatter),
        }
    }
}

impl Error for UsageError {}

fn geometry_test() -> Result<(), RuntimeError> {
    let dimensions = source_dimensions()?;
    println!(
        "logical={}x{} stride={}",
        dimensions.width(),
        dimensions.height(),
        DashboardFrame::stride_bytes()
    );
    Ok(())
}

fn validate_manifest(path: &Path) -> Result<(), RuntimeError> {
    require_absolute(path, "manifest")?;
    let bytes =
        read_regular_bounded(path, MAXIMUM_CATALOG_BYTES).map_err(RuntimeError::ValidationRead)?;
    let catalog = Catalog::parse(&bytes).map_err(RuntimeError::ManifestValidation)?;
    if catalog.is_empty() {
        return Err(RuntimeError::EmptyManifest);
    }
    let _catalog = retro_deck_dashboard::DashboardCatalog::with_standard_apps(&catalog)
        .map_err(RuntimeError::CatalogValidation)?;
    Ok(())
}

fn validate_palette(path: &Path) -> Result<(), RuntimeError> {
    require_absolute(path, "palette")?;
    let bytes =
        read_regular_bounded(path, MAXIMUM_PALETTE_BYTES).map_err(RuntimeError::ValidationRead)?;
    let _palette = Palette::parse_tsv(&bytes).map_err(RuntimeError::PaletteValidation)?;
    Ok(())
}

fn require_absolute(path: &Path, role: &'static str) -> Result<(), RuntimeError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(RuntimeError::RelativeValidationPath(role))
    }
}

#[derive(Debug)]
struct PendingLaunch {
    command: ProcessCommand,
    label: String,
    exit_policy: ExitPolicy,
    queued_at: Instant,
}

impl PendingLaunch {
    fn from_plan(plan: LaunchPlan<'_>, label: String) -> Self {
        let mut command = ProcessCommand::new(plan.program());
        command.current_dir("/");
        for name in [
            VOLUME_ENVIRONMENT,
            KEYMAP_ENVIRONMENT,
            EXIT_HINT_ENVIRONMENT,
            VOLUME_STATE_ENVIRONMENT,
        ] {
            command.env_remove(name);
        }
        if let Some(argument) = plan.argument() {
            command.arg(argument);
        }
        if let Some(percent) = plan.volume_percent() {
            command.env(VOLUME_ENVIRONMENT, percent.to_string());
        }
        if let Some(keymap) = plan.keymap() {
            command.env(KEYMAP_ENVIRONMENT, keymap.as_str());
        }
        if plan.exit_hint() {
            command.env(EXIT_HINT_ENVIRONMENT, "1");
        }
        if let Some(path) = plan.volume_state() {
            command.env(VOLUME_STATE_ENVIRONMENT, path);
        }
        Self {
            command,
            label,
            exit_policy: plan.exit_policy(),
            queued_at: Instant::now(),
        }
    }
}

#[derive(Debug)]
struct DashboardRuntime {
    shutdown: ShutdownFlag,
    controllers: ControllerDevices,
    input_events: Vec<InputEvent>,
    presentation: WaylandPresentation,
    source_dimensions: Dimensions,
    model: DashboardModel,
    credits: CreditsCrawl,
    artwork: ArtworkStore,
    palette: Palette,
    frame: DashboardFrame,
    touch: TouchCommitter,
    controller_guard: ControllerGuard,
    audio: Option<ToneCueWorker<MenuCue>>,
    audio_gate: AudioGate,
    preferences: Option<PreferenceWorker>,
    pending_launch: Option<PendingLaunch>,
    started_at: Instant,
    credits_started_at: Instant,
    last_credits_frame: Instant,
    last_controller_scan: Instant,
    reduced_motion: bool,
    dirty: bool,
}

impl DashboardRuntime {
    fn start(paths: &DashboardAssetPaths) -> Result<Self, RuntimeError> {
        let shutdown = ShutdownFlag::install().map_err(RuntimeError::Signals)?;
        let assets = DashboardAssets::load(paths).map_err(RuntimeError::Assets)?;
        if let Some(error) = assets.credits_fallback() {
            eprintln!("{APPLICATION}: {error}; using the unavailable credits view");
        }
        if let Some(error) = assets.palette_fallback() {
            eprintln!("{APPLICATION}: {error}; using compiled dashboard colors");
        }
        let preference_paths = standard_preference_paths()?;
        let preference_load = PreferenceLoad::load(&preference_paths);
        for issue in preference_load.issues() {
            eprintln!("{APPLICATION}: {issue}");
        }
        let preferences = preference_load.preferences();
        let brightness_paths = standard_brightness_paths()?;
        let preference_worker =
            start_preference_worker(preference_paths, brightness_paths, preferences);
        let model = DashboardModel::new(
            assets.catalog().clone(),
            preferences.volume(),
            preferences.brightness(),
            preferences.keymap(),
        );
        let artwork = load_artwork(assets.catalog().entries());
        let palette = *assets.palette();
        let frame = DashboardFrame::render_menu_with_artwork(&model, &palette, &artwork)
            .map_err(RuntimeError::Render)?;
        let source_dimensions = source_dimensions()?;
        let presentation = WaylandPresentation::connect_widget(source_dimensions)
            .map_err(RuntimeError::Presentation)?;
        let controllers = ControllerDevices::discover().map_err(RuntimeError::Input)?;
        let volume = Volume::new(model.volume().percent()).ok_or(RuntimeError::InvalidDefaults)?;
        let audio_gate = desired_audio_gate(presentation.visible(), volume.muted());
        let audio = start_audio(volume, audio_gate);
        eprintln!(
            "{APPLICATION}: native navigation runtime started with {} controller(s); Wi-Fi editing remains isolated",
            controllers.controller_count()
        );
        let now = Instant::now();
        Ok(Self {
            shutdown,
            controllers,
            input_events: Vec::with_capacity(INPUT_EVENT_CAPACITY),
            presentation,
            source_dimensions,
            model,
            credits: assets.credits().clone(),
            artwork,
            palette,
            frame,
            touch: TouchCommitter::default(),
            controller_guard: ControllerGuard::new(),
            audio,
            audio_gate,
            preferences: preference_worker,
            pending_launch: None,
            started_at: now,
            credits_started_at: now,
            last_credits_frame: now,
            last_controller_scan: now,
            reduced_motion: env::var_os("RETRO_DECK_REDUCED_MOTION").is_some(),
            dirty: true,
        })
    }

    fn run(mut self) -> Result<(), RuntimeError> {
        let result = self.event_loop();
        self.finish_audio();
        self.finish_preferences();
        result
    }

    fn event_loop(&mut self) -> Result<(), RuntimeError> {
        while !self.shutdown.requested() && !self.presentation.shutdown_requested() {
            self.presentation
                .dispatch_nonblocking()
                .map_err(RuntimeError::Presentation)?;
            self.sync_audio_gate();
            self.report_audio_errors();
            self.report_preference_errors();
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                break;
            }
            let suppress_menu_input = self.service_pending_launch();
            if suppress_menu_input {
                self.discard_menu_input();
            } else {
                self.scan_controllers();
                self.recover_controller();
                self.handle_touch();
                self.handle_controllers();
            }
            let now_ms = self.monotonic_ms();
            self.dirty |= self.model.advance_time(now_ms);
            self.schedule_credits_frame();
            if self.dirty && self.presentation.visible() && !self.present()? {
                break;
            }
            self.controllers
                .wait_readable_with(self.presentation.as_fd(), self.wait_duration())
                .map_err(RuntimeError::Input)?;
        }
        Ok(())
    }

    fn scan_controllers(&mut self) {
        if self.last_controller_scan.elapsed() < CONTROLLER_SCAN {
            return;
        }
        self.last_controller_scan = Instant::now();
        match self.controllers.rescan() {
            Ok(stats) if stats.attached() > 0 => eprintln!(
                "{APPLICATION}: attached {} controller(s); {} connected",
                stats.attached(),
                stats.connected()
            ),
            Ok(_) => {}
            Err(error) => eprintln!("{APPLICATION}: controller rescan failed: {error}"),
        }
    }

    fn recover_controller(&mut self) {
        if self.controller_guard.recover_if_quiet(self.monotonic_ms()) {
            eprintln!("{APPLICATION}: controller input resumed after one quiet second");
        }
    }

    fn handle_controllers(&mut self) {
        self.input_events.clear();
        let stats = self.controllers.drain_into(&mut self.input_events);
        if stats.dropped() > 0 {
            eprintln!(
                "{APPLICATION}: discarded {} controller event(s) after the bounded drain",
                stats.dropped()
            );
        }
        if stats.disconnected_count() > 0 {
            eprintln!(
                "{APPLICATION}: {} controller(s) disconnected",
                stats.disconnected_count()
            );
        }

        let events = std::mem::take(&mut self.input_events);
        for event in events.iter().copied() {
            let InputEvent::Controller { button, edge, .. } = event else {
                continue;
            };
            let Some(action) = controller_action(self.model.screen(), button, edge) else {
                continue;
            };
            let was_suspended = self.controller_guard.suspended();
            if !self.controller_guard.accept(self.monotonic_ms()) {
                if !was_suspended && self.controller_guard.suspended() {
                    eprintln!(
                        "{APPLICATION}: controller input suspended after a burst; waiting for quiet"
                    );
                }
                continue;
            }
            self.touch.cancel();
            self.apply_action(action);
            if self.pending_launch.is_some() {
                break;
            }
        }
        self.input_events = events;
    }

    fn handle_touch(&mut self) {
        let (reports, dropped) = self.presentation.take_touch_reports();
        if dropped > 0 {
            eprintln!(
                "{APPLICATION}: discarded {dropped} stale touch report(s) after the bounded queue"
            );
            self.touch.cancel();
        }
        for report in reports {
            let target = self
                .frame
                .action_at(usize::from(report.x()), usize::from(report.y()));
            let Some(action) = self
                .touch
                .update(report.pressed(), report.released(), target)
            else {
                continue;
            };
            self.apply_action(action);
            self.touch.cancel();
            break;
        }
    }

    fn apply_action(&mut self, action: Action) {
        let previous_screen = self.model.screen();
        let now_ms = self.monotonic_ms();
        let transition = self.model.apply_at(action, now_ms);
        if previous_screen != Screen::Credits && self.model.screen() == Screen::Credits {
            let now = Instant::now();
            self.credits_started_at = now;
            self.last_credits_frame = now;
        }
        self.dirty |= transition.redraw;
        if let Some(intent) = transition.intent {
            self.queue_intent(intent);
        }
        if let Some(setting) = transition.setting {
            self.apply_setting(setting);
        }
        self.sync_audio_gate();
        if let (Some(audio), Some(cue)) = (&self.audio, transition.cue) {
            let _outcome = audio.try_play(cue);
        }
    }

    fn apply_setting(&self, setting: SettingChange) {
        if let SettingChange::Volume(percent) = setting {
            let Some(volume) = Volume::new(percent) else {
                eprintln!("{APPLICATION}: rejected invalid in-memory audio volume {percent}");
                return;
            };
            if let Some(audio) = &self.audio {
                audio.set_volume(volume);
            }
        }
        let Some(preferences) = &self.preferences else {
            return;
        };
        match preferences.try_submit(setting) {
            PreferenceSubmit::Accepted | PreferenceSubmit::Coalesced => {}
            PreferenceSubmit::Invalid => {
                eprintln!("{APPLICATION}: rejected invalid preference effect {setting:?}");
            }
            PreferenceSubmit::Disconnected => {
                eprintln!(
                    "{APPLICATION}: preference worker is unavailable; {setting:?} remains in memory only"
                );
            }
        }
    }

    fn queue_intent(&mut self, intent: Intent) {
        if self.pending_launch.is_some() {
            return;
        }
        match intent {
            Intent::Launch(index) => self.queue_catalog_launch(index),
            Intent::OpenTerminal => {
                let plan = LaunchPlan::from_target(
                    LaunchTarget::Terminal(TerminalMode::Shell),
                    self.model.volume(),
                    self.model.keymap(),
                );
                match plan {
                    Ok(plan) => self
                        .begin_audio_handoff(PendingLaunch::from_plan(plan, "terminal".to_owned())),
                    Err(error) => {
                        eprintln!("{APPLICATION}: cannot plan terminal launch: {error}");
                    }
                }
            }
            Intent::OpenWifi => eprintln!(
                "{APPLICATION}: Wi-Fi editor intent remains isolated from the staged Rust runtime"
            ),
        }
    }

    fn queue_catalog_launch(&mut self, index: usize) {
        let Some(entry) = self.model.catalog().entry(index) else {
            eprintln!("{APPLICATION}: rejected missing catalog launch index {index}");
            return;
        };
        let label = entry.title().to_owned();
        let target = match LaunchTarget::from_entry(entry) {
            Ok(target) => target,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot classify {label}: {error}");
                return;
            }
        };
        let plan = if matches!(target, LaunchTarget::Reboot) {
            LaunchPlan::confirmed_reboot()
        } else {
            match LaunchPlan::from_target(target, self.model.volume(), self.model.keymap()) {
                Ok(plan) => plan,
                Err(error) => {
                    eprintln!("{APPLICATION}: cannot plan {label}: {error}");
                    return;
                }
            }
        };
        let pending = PendingLaunch::from_plan(plan, label);
        self.begin_audio_handoff(pending);
    }

    fn begin_audio_handoff(&mut self, pending: PendingLaunch) {
        eprintln!(
            "{APPLICATION}: preparing managed launch for {}",
            pending.label
        );
        self.pending_launch = Some(pending);
        self.audio_gate = AudioGate::Hidden;
        if let Some(audio) = &self.audio {
            audio.set_gate(AudioGate::Hidden);
        }
    }

    fn service_pending_launch(&mut self) -> bool {
        let Some(pending) = self.pending_launch.as_ref() else {
            return false;
        };
        let audio_released = self
            .audio
            .as_ref()
            .is_none_or(ToneCueWorker::device_released);
        if !audio_released {
            if pending.queued_at.elapsed() < AUDIO_HANDOFF_TIMEOUT {
                return true;
            }
            let Some(cancelled) = self.pending_launch.take() else {
                return true;
            };
            eprintln!(
                "{APPLICATION}: cancelled {} because menu audio did not release in time",
                cancelled.label
            );
            self.restore_audio_gate();
            self.dirty = true;
            return true;
        }

        let Some(pending) = self.pending_launch.take() else {
            return false;
        };
        self.run_pending_launch(pending);
        self.reload_child_volume();
        self.restore_audio_gate();
        self.dirty = true;
        true
    }

    fn run_pending_launch(&mut self, mut pending: PendingLaunch) {
        let mut touchscreen = match pending.exit_policy {
            ExitPolicy::SupervisorTouchHold => match TouchscreenDevice::discover() {
                Ok(touchscreen) => Some(touchscreen),
                Err(error) => {
                    eprintln!(
                        "{APPLICATION}: cannot start {} without supervised exit touch: {error}",
                        pending.label
                    );
                    return;
                }
            },
            ExitPolicy::ChildOwnsTouch | ExitPolicy::None => None,
        };
        let mut exit_hold = touchscreen
            .as_ref()
            .map(|touchscreen| ExitHold::new(touchscreen.state().down()));
        let mut child = match ManagedChild::spawn(&mut pending.command) {
            Ok(child) => child,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot launch {}: {error}", pending.label);
                return;
            }
        };
        eprintln!(
            "{APPLICATION}: launched {} as {}",
            pending.label,
            child.program().display()
        );
        let child_started = Instant::now();

        loop {
            if let Err(error) = self.presentation.dispatch_nonblocking() {
                eprintln!(
                    "{APPLICATION}: display failed while supervising {}: {error}",
                    pending.label
                );
                return;
            }
            let now = Instant::now();
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                request_child_termination(&mut child, now, &pending.label, "dashboard shutdown");
            }
            match child.poll(now) {
                Ok(Some(exit)) => {
                    report_child_exit(&pending.label, exit);
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "{APPLICATION}: cannot supervise {}: {error}; forcing containment",
                        pending.label
                    );
                    return;
                }
            }

            let touch_available = match (touchscreen.as_mut(), exit_hold.as_mut()) {
                (Some(device), Some(hold)) => update_supervised_touch(
                    device,
                    hold,
                    &mut child,
                    now,
                    child_started,
                    &pending.label,
                ),
                _ => true,
            };
            if !touch_available {
                touchscreen = None;
                exit_hold = None;
            }
            self.discard_menu_input();

            let wait = if let Some(touchscreen) = &touchscreen {
                touchscreen.wait_readable_with(self.presentation.as_fd(), CHILD_POLL)
            } else {
                self.controllers
                    .wait_readable_with(self.presentation.as_fd(), CHILD_POLL)
            };
            if let Err(error) = wait {
                eprintln!(
                    "{APPLICATION}: wait failed while supervising {}: {error}; forcing containment",
                    pending.label
                );
                return;
            }
        }
    }

    fn discard_menu_input(&mut self) {
        self.input_events.clear();
        let _stats = self.controllers.drain_into(&mut self.input_events);
        self.input_events.clear();
        let _reports = self.presentation.take_touch_reports();
        self.touch.cancel();
    }

    fn reload_child_volume(&mut self) {
        let bytes = match read_regular_bounded(
            Path::new(VOLUME_STATE),
            retro_deck_dashboard::MAXIMUM_PREFERENCE_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot reload child volume: {error}");
                return;
            }
        };
        let volume = match parse_volume(&bytes) {
            Ok(volume) => volume,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot reload child volume: {error}");
                return;
            }
        };
        if !self.model.adopt_volume(volume) {
            return;
        }
        if let Some(audio) = &self.audio {
            let Some(volume) = Volume::new(volume.percent()) else {
                return;
            };
            audio.set_volume(volume);
        }
        eprintln!(
            "{APPLICATION}: managed child updated game volume to {}%",
            volume.percent()
        );
    }

    fn restore_audio_gate(&mut self) {
        let requested =
            desired_audio_gate(self.presentation.visible(), self.model.volume().is_muted());
        self.audio_gate = requested;
        if let Some(audio) = &self.audio {
            audio.set_gate(requested);
        }
    }

    fn sync_audio_gate(&mut self) {
        let requested = if self.pending_launch.is_some() {
            AudioGate::Hidden
        } else {
            desired_audio_gate(self.presentation.visible(), self.model.volume().is_muted())
        };
        if requested == self.audio_gate {
            return;
        }
        self.audio_gate = requested;
        if let Some(audio) = &self.audio {
            audio.set_gate(requested);
        }
    }

    fn report_audio_errors(&self) {
        let Some(audio) = &self.audio else {
            return;
        };
        for error in audio.take_errors() {
            eprintln!("{APPLICATION}: menu sound unavailable for now: {error}");
        }
    }

    fn finish_audio(&mut self) {
        self.report_audio_errors();
        let Some(audio) = self.audio.take() else {
            return;
        };
        audio.set_gate(AudioGate::Hidden);
        report_audio_shutdown(audio.shutdown());
    }

    fn report_preference_errors(&self) {
        let Some(preferences) = &self.preferences else {
            return;
        };
        for error in preferences.take_errors() {
            eprintln!("{APPLICATION}: {error}");
        }
    }

    fn finish_preferences(&mut self) {
        self.report_preference_errors();
        let Some(preferences) = self.preferences.take() else {
            return;
        };
        report_preference_shutdown(preferences.shutdown());
    }

    fn schedule_credits_frame(&mut self) {
        if self.model.screen() == Screen::Credits
            && !self.reduced_motion
            && self.last_credits_frame.elapsed() >= CREDITS_FRAME
        {
            self.dirty = true;
        }
    }

    fn present(&mut self) -> Result<bool, RuntimeError> {
        self.redraw();
        let frame = Frame::rgb565(
            self.frame.pixels(),
            self.source_dimensions,
            retro_deck_dashboard::CANVAS_WIDTH,
        )
        .map_err(RuntimeError::Frame)?;
        match self.presentation.present(frame) {
            Ok(PresentOutcome::Submitted) => {
                self.dirty = false;
                if self.model.screen() == Screen::Credits {
                    self.last_credits_frame = Instant::now();
                }
                Ok(true)
            }
            Ok(PresentOutcome::Busy) => Ok(true),
            Err(WaylandPresentationError::SurfaceClosed) => Ok(false),
            Err(error) => Err(RuntimeError::Presentation(error)),
        }
    }

    fn redraw(&mut self) {
        match self.model.screen() {
            Screen::Dashboard => {
                self.frame
                    .redraw_menu_with_artwork(&self.model, &self.palette, &self.artwork);
            }
            Screen::Settings => self.frame.redraw_settings(
                &self.model,
                &self.palette,
                SettingsView::new(NetworkView::unavailable(), "/BIN/ASH"),
            ),
            Screen::Credits => self.frame.redraw_credits(
                &self.credits,
                &self.palette,
                self.reduced_motion,
                elapsed_milliseconds(self.credits_started_at),
            ),
        }
    }

    fn wait_duration(&self) -> Duration {
        if self.pending_launch.is_some() {
            return BUSY_RETRY;
        }
        if !self.presentation.visible() {
            return IDLE_POLL;
        }
        if self.dirty {
            return BUSY_RETRY;
        }
        if self.model.screen() != Screen::Credits || self.reduced_motion {
            return IDLE_POLL;
        }
        CREDITS_FRAME.saturating_sub(self.last_credits_frame.elapsed())
    }

    fn monotonic_ms(&self) -> u64 {
        elapsed_milliseconds(self.started_at)
    }
}

fn standard_preference_paths() -> Result<PreferencePaths, RuntimeError> {
    PreferencePaths::new(VOLUME_STATE, BRIGHTNESS_STATE, KEYMAP_STATE)
        .map_err(RuntimeError::PreferencePaths)
}

fn standard_brightness_paths() -> Result<BrightnessDevicePaths, RuntimeError> {
    BrightnessDevicePaths::new(BRIGHTNESS_DEVICE, BRIGHTNESS_MAXIMUM)
        .map_err(RuntimeError::BrightnessPaths)
}

fn start_preference_worker(
    state_paths: PreferencePaths,
    brightness_paths: BrightnessDevicePaths,
    initial: DashboardPreferences,
) -> Option<PreferenceWorker> {
    match PreferenceWorker::spawn(state_paths, brightness_paths, initial) {
        Ok(worker) => Some(worker),
        Err(error) => {
            eprintln!(
                "{APPLICATION}: cannot start preference worker: {error}; settings remain in memory only"
            );
            None
        }
    }
}

fn load_artwork(entries: &[retro_deck_config::CatalogEntry]) -> ArtworkStore {
    let store = match ArtworkStore::load(COVER_DIRECTORY, entries) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{APPLICATION}: {error}; continuing without cover art");
            return ArtworkStore::default();
        }
    };
    for issue in store.issues() {
        eprintln!("{APPLICATION}: {issue}");
    }
    let report = store.report();
    if report.capacity_skipped != 0 {
        eprintln!(
            "{APPLICATION}: skipped {} cover(s) after the decoded-art budget was full",
            report.capacity_skipped
        );
    }
    eprintln!(
        "{APPLICATION}: loaded {} cached cover(s); {} missing and {} invalid",
        report.loaded, report.missing, report.invalid
    );
    store
}

fn start_audio(volume: Volume, gate: AudioGate) -> Option<ToneCueWorker<MenuCue>> {
    let Some(rate) = SampleRate::new(CUE_SAMPLE_RATE) else {
        eprintln!("{APPLICATION}: internal cue sample rate is invalid; menu sound disabled");
        return None;
    };
    match ToneCueWorker::spawn(rate, volume, menu_notes) {
        Ok(worker) => {
            worker.set_gate(gate);
            Some(worker)
        }
        Err(error) => {
            eprintln!("{APPLICATION}: cannot start audio worker: {error}; menu sound disabled");
            None
        }
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

fn report_audio_shutdown(report: ToneWorkerReport) {
    if report.panicked {
        eprintln!("{APPLICATION}: audio worker panicked during shutdown");
    }
    if report.errors != 0 || report.dropped_errors != 0 {
        eprintln!(
            "{APPLICATION}: audio worker stopped with {} error(s), including {} unreported",
            report.errors, report.dropped_errors
        );
    }
}

fn report_preference_shutdown(report: PreferenceWorkerReport) {
    if report.panicked {
        eprintln!("{APPLICATION}: preference worker panicked during shutdown");
    }
    if report.errors != 0 || report.dropped_errors != 0 {
        eprintln!(
            "{APPLICATION}: preference worker stopped with {} error(s), including {} unreported",
            report.errors, report.dropped_errors
        );
    }
}

fn source_dimensions() -> Result<Dimensions, RuntimeError> {
    Dimensions::new(
        retro_deck_dashboard::CANVAS_WIDTH,
        retro_deck_dashboard::CANVAS_HEIGHT,
    )
    .ok_or(RuntimeError::InvalidDimensions)
}

fn elapsed_milliseconds(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn update_supervised_touch(
    touchscreen: &mut TouchscreenDevice,
    exit_hold: &mut ExitHold,
    child: &mut ManagedChild,
    now: Instant,
    child_started: Instant,
    label: &str,
) -> bool {
    let state = match touchscreen.drain() {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "{APPLICATION}: supervised touch failed for {label}: {error}; stopping child"
            );
            request_child_termination(child, now, label, "touch failure");
            return false;
        }
    };
    let point = state.point();
    match exit_hold.update(
        state.down(),
        point.x(),
        point.y(),
        elapsed_milliseconds(child_started),
    ) {
        Some(ExitHoldEvent::Started) => {
            eprintln!("{APPLICATION}: exit hold started for {label}");
        }
        Some(ExitHoldEvent::Cancelled) => {
            eprintln!("{APPLICATION}: exit hold cancelled for {label}");
        }
        Some(ExitHoldEvent::Completed) => {
            request_child_termination(child, now, label, "touch hold");
        }
        None => {}
    }
    true
}

fn request_child_termination(child: &mut ManagedChild, now: Instant, label: &str, reason: &str) {
    match child.request_termination(now) {
        Ok(true) => eprintln!("{APPLICATION}: stopping {label} after {reason}"),
        Ok(false) => {}
        Err(error) => eprintln!("{APPLICATION}: cannot stop {label} after {reason}: {error}"),
    }
}

fn report_child_exit(label: &str, exit: ManagedChildExit) {
    eprintln!(
        "{APPLICATION}: {label} ended with {} ({:?})",
        exit.status(),
        exit.cause()
    );
}

#[derive(Debug)]
enum RuntimeError {
    Signals(io::Error),
    Assets(DashboardAssetsError),
    PreferencePaths(PreferencePathError),
    BrightnessPaths(BrightnessPathError),
    InvalidDefaults,
    InvalidDimensions,
    Presentation(WaylandPresentationError),
    Input(InputError),
    Render(RenderError),
    Frame(DisplayError),
    RelativeValidationPath(&'static str),
    ValidationRead(BoundedReadError),
    ManifestValidation(retro_deck_config::CatalogError),
    PaletteValidation(retro_deck_config::PaletteError),
    CatalogValidation(retro_deck_dashboard::DashboardCatalogError),
    EmptyManifest,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signals(error) => write!(formatter, "cannot install shutdown handlers: {error}"),
            Self::Assets(error) => error.fmt(formatter),
            Self::PreferencePaths(error) => error.fmt(formatter),
            Self::BrightnessPaths(error) => error.fmt(formatter),
            Self::InvalidDefaults => formatter.write_str("compiled dashboard defaults are invalid"),
            Self::InvalidDimensions => {
                formatter.write_str("dashboard canvas dimensions are invalid")
            }
            Self::Presentation(error) => write!(formatter, "Wayland presentation failed: {error}"),
            Self::Input(error) => write!(formatter, "controller input failed: {error}"),
            Self::Render(error) => error.fmt(formatter),
            Self::Frame(error) => write!(formatter, "dashboard frame is invalid: {error}"),
            Self::RelativeValidationPath(role) => {
                write!(formatter, "{role} validation path is not absolute")
            }
            Self::ValidationRead(error) => {
                write!(formatter, "cannot read validation input: {error}")
            }
            Self::ManifestValidation(error) => write!(formatter, "invalid manifest: {error}"),
            Self::PaletteValidation(error) => write!(formatter, "invalid palette: {error}"),
            Self::CatalogValidation(error) => {
                write!(formatter, "invalid dashboard catalog: {error}")
            }
            Self::EmptyManifest => formatter.write_str("manifest contains no entries"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Signals(error) => Some(error),
            Self::Assets(error) => Some(error),
            Self::PreferencePaths(error) => Some(error),
            Self::BrightnessPaths(error) => Some(error),
            Self::Presentation(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::ValidationRead(error) => Some(error),
            Self::ManifestValidation(error) => Some(error),
            Self::PaletteValidation(error) => Some(error),
            Self::CatalogValidation(error) => Some(error),
            Self::InvalidDefaults
            | Self::InvalidDimensions
            | Self::RelativeValidationPath(_)
            | Self::EmptyManifest => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::process::Command as ProcessCommand;

    use retro_deck_config::System;
    use retro_deck_dashboard::{
        ExitPolicy, Keymap, LaunchPlan, LaunchTarget, TerminalMode, VolumeState,
    };
    use retro_deck_platform::audio::AudioGate;

    use super::{
        Command, EXIT_HINT_ENVIRONMENT, KEYMAP_ENVIRONMENT, PendingLaunch, UsageError,
        VOLUME_ENVIRONMENT, VOLUME_STATE_ENVIRONMENT, desired_audio_gate, parse_arguments,
    };

    fn parse(arguments: &[&str]) -> Result<Command, UsageError> {
        parse_arguments(arguments.iter().map(OsString::from))
    }

    #[test]
    fn command_line_is_strict_order_independent_and_absolute() {
        let parsed = parse(&[
            "--palette",
            "/tmp/palette",
            "--manifest",
            "/tmp/manifest",
            "--credits",
            "/tmp/credits",
        ]);
        assert!(matches!(
            parsed,
            Ok(Command::Run(ref paths))
                if paths.manifest() == Path::new("/tmp/manifest")
                    && paths.credits() == Path::new("/tmp/credits")
                    && paths.palette() == Path::new("/tmp/palette")
        ));
        assert!(matches!(
            parse(&["--validate-manifest", "/tmp/manifest"]),
            Ok(Command::ValidateManifest(_))
        ));
        assert!(parse(&["--manifest", "relative"]).is_err());
        assert!(parse(&["--unknown", "/tmp/value"]).is_err());
        assert!(parse(&["--manifest", "/a", "--manifest", "/b"]).is_err());
    }

    #[test]
    fn audio_gate_releases_hidden_and_muted_dashboard_sound() {
        assert_eq!(desired_audio_gate(true, false), AudioGate::Active);
        assert_eq!(desired_audio_gate(true, true), AudioGate::Muted);
        assert_eq!(desired_audio_gate(false, false), AudioGate::Hidden);
        assert_eq!(desired_audio_gate(false, true), AudioGate::Hidden);
    }

    #[test]
    fn pending_commands_own_fixed_program_arguments_and_environment() {
        let Some(volume) = VolumeState::new(55, 55).ok() else {
            return;
        };
        let plan = LaunchPlan::from_target(
            LaunchTarget::Emulator {
                system: System::Nes,
                content: Path::new("/mnt/data/roms/nes/test.nes"),
            },
            volume,
            Keymap::Czech,
        );
        let Some(plan) = plan.ok() else {
            return;
        };
        let pending = PendingLaunch::from_plan(plan, "TEST".to_owned());
        assert_eq!(
            pending.command.get_program(),
            OsStr::new("/mnt/data/nes-deck/nes-deck")
        );
        assert_eq!(
            pending.command.get_args().collect::<Vec<_>>(),
            [OsStr::new("/mnt/data/roms/nes/test.nes")]
        );
        assert_eq!(pending.command.get_current_dir(), Some(Path::new("/")));
        assert_eq!(
            environment_value(&pending.command, VOLUME_ENVIRONMENT),
            Some(OsStr::new("55"))
        );
        assert_eq!(
            environment_value(&pending.command, EXIT_HINT_ENVIRONMENT),
            Some(OsStr::new("1"))
        );
        assert!(environment_removed(&pending.command, KEYMAP_ENVIRONMENT));
        assert!(environment_removed(
            &pending.command,
            VOLUME_STATE_ENVIRONMENT
        ));
        assert_eq!(pending.exit_policy, ExitPolicy::SupervisorTouchHold);

        let terminal = LaunchPlan::from_target(
            LaunchTarget::Terminal(TerminalMode::Lisp),
            volume,
            Keymap::Czech,
        );
        let Some(terminal) = terminal.ok() else {
            return;
        };
        let terminal = PendingLaunch::from_plan(terminal, "LISP".to_owned());
        assert_eq!(
            terminal.command.get_args().collect::<Vec<_>>(),
            [OsStr::new("lisp")]
        );
        assert_eq!(
            environment_value(&terminal.command, KEYMAP_ENVIRONMENT),
            Some(OsStr::new("cz"))
        );
        assert!(environment_removed(&terminal.command, VOLUME_ENVIRONMENT));
    }

    fn environment_value<'command>(
        command: &'command ProcessCommand,
        name: &str,
    ) -> Option<&'command OsStr> {
        command
            .get_envs()
            .find_map(|(key, value)| (key == name).then_some(value).flatten())
    }

    fn environment_removed(command: &ProcessCommand, name: &str) -> bool {
        command
            .get_envs()
            .any(|(key, value)| key == name && value.is_none())
    }
}
