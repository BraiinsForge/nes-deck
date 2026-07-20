//! Native Wayland dashboard runtime under staged migration.

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::os::fd::AsFd as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use retro_deck_config::{Catalog, MAXIMUM_CATALOG_BYTES, MAXIMUM_PALETTE_BYTES, Palette};
use retro_deck_dashboard::{
    Action, AssetPathError, Brightness, ControllerGuard, CreditsCrawl, DashboardAssetPaths,
    DashboardAssets, DashboardAssetsError, DashboardFrame, DashboardModel, Intent, Keymap,
    NetworkView, RenderError, Screen, SettingChange, SettingsView, TouchCommitter, VolumeState,
    controller_action,
};
use retro_deck_platform::display::{Dimensions, DisplayError, Frame};
use retro_deck_platform::file::{BoundedReadError, read_regular_bounded};
use retro_deck_platform::input::{ControllerDevices, InputError, InputEvent};
use retro_deck_platform::shutdown::ShutdownFlag;
use retro_deck_platform::wayland::{PresentOutcome, WaylandPresentation, WaylandPresentationError};

const APPLICATION: &str = "deck-dashboard";
const INPUT_EVENT_CAPACITY: usize = 64;
const IDLE_POLL: Duration = Duration::from_millis(250);
const BUSY_RETRY: Duration = Duration::from_millis(8);
const CREDITS_FRAME: Duration = Duration::from_millis(40);
const CONTROLLER_SCAN: Duration = Duration::from_secs(1);

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
struct DashboardRuntime {
    shutdown: ShutdownFlag,
    controllers: ControllerDevices,
    input_events: Vec<InputEvent>,
    presentation: WaylandPresentation,
    source_dimensions: Dimensions,
    model: DashboardModel,
    credits: CreditsCrawl,
    palette: Palette,
    frame: DashboardFrame,
    touch: TouchCommitter,
    controller_guard: ControllerGuard,
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
        let model = DashboardModel::new(
            assets.catalog().clone(),
            VolumeState::new(42, 42).map_err(|_| RuntimeError::InvalidDefaults)?,
            Brightness::new(60).map_err(|_| RuntimeError::InvalidDefaults)?,
            Keymap::Us,
        );
        let palette = *assets.palette();
        let frame = DashboardFrame::render_menu(&model, &palette).map_err(RuntimeError::Render)?;
        let source_dimensions = source_dimensions()?;
        let presentation = WaylandPresentation::connect_widget(source_dimensions)
            .map_err(RuntimeError::Presentation)?;
        let controllers = ControllerDevices::discover().map_err(RuntimeError::Input)?;
        eprintln!(
            "{APPLICATION}: native navigation runtime started with {} controller(s); external effects remain disabled",
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
            palette,
            frame,
            touch: TouchCommitter::default(),
            controller_guard: ControllerGuard::new(),
            started_at: now,
            credits_started_at: now,
            last_credits_frame: now,
            last_controller_scan: now,
            reduced_motion: env::var_os("RETRO_DECK_REDUCED_MOTION").is_some(),
            dirty: true,
        })
    }

    fn run(mut self) -> Result<(), RuntimeError> {
        while !self.shutdown.requested() && !self.presentation.shutdown_requested() {
            self.presentation
                .dispatch_nonblocking()
                .map_err(RuntimeError::Presentation)?;
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                break;
            }
            self.scan_controllers();
            self.recover_controller();
            self.handle_touch();
            self.handle_controllers();
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
        let transition = self.model.apply(action);
        if previous_screen != Screen::Credits && self.model.screen() == Screen::Credits {
            let now = Instant::now();
            self.credits_started_at = now;
            self.last_credits_frame = now;
        }
        self.dirty |= transition.redraw;
        if let Some(intent) = transition.intent {
            report_disabled_intent(intent);
        }
        if let Some(setting) = transition.setting {
            report_unpersisted_setting(setting);
        }
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
            Screen::Dashboard => self.frame.redraw_menu(&self.model, &self.palette),
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

fn report_disabled_intent(intent: Intent) {
    eprintln!(
        "{APPLICATION}: staged runtime ignored external intent {intent:?}; launch integration is not enabled"
    );
}

fn report_unpersisted_setting(setting: SettingChange) {
    eprintln!(
        "{APPLICATION}: staged runtime applied {setting:?} in memory only; persistence is not enabled"
    );
}

#[derive(Debug)]
enum RuntimeError {
    Signals(io::Error),
    Assets(DashboardAssetsError),
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
    use std::ffi::OsString;
    use std::path::Path;

    use super::{Command, UsageError, parse_arguments};

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
}
