//! Nonblocking Linux evdev discovery and event normalization.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use evdev::{AbsInfo, AbsoluteAxisCode, Device, EventSummary, KeyCode, SynchronizationCode};
use rustix::event::{PollFd, PollFlags, poll};

use super::{
    AxisRange, ButtonSet, ControllerTracker, InputEvent, LOGICAL_HEIGHT, LOGICAL_WIDTH,
    PhysicalButton, Player, TouchState, TouchTracker,
};
use crate::time::duration_timespec;

const INPUT_DIRECTORY: &str = "/dev/input";
const TOUCHSCREEN_NAME: &str = "Goodix Capacitive TouchScreen";
const THE_GAMEPAD_VENDOR: u16 = 0x1c59;
const THE_GAMEPAD_PRODUCT: u16 = 0x0026;
const MAXIMUM_CONTROLLERS: usize = 2;
const MAXIMUM_EVENTS_PER_DRAIN: usize = 64;
const PLAYERS: [Player; MAXIMUM_CONTROLLERS] = [Player::One, Player::Two];

/// Result of one nonblocking drain across every discovered input device.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DrainStats {
    emitted: usize,
    dropped: usize,
    disconnected_players: u8,
}

impl DrainStats {
    /// Number of normalized events appended to the caller's buffer.
    #[must_use]
    pub const fn emitted(self) -> usize {
        self.emitted
    }

    /// Number of normalized events discarded by the fixed per-drain bound.
    #[must_use]
    pub const fn dropped(self) -> usize {
        self.dropped
    }

    /// Whether one stable controller slot disconnected during this drain.
    #[must_use]
    pub const fn disconnected(self, player: Player) -> bool {
        self.disconnected_players & player_mask(player) != 0
    }

    /// Number of stable controller slots disconnected during this drain.
    #[must_use]
    pub const fn disconnected_count(self) -> u32 {
        self.disconnected_players.count_ones()
    }
}

/// Result of one explicit controller hotplug scan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ControllerScanStats {
    attached: usize,
    connected: usize,
}

impl ControllerScanStats {
    /// Controllers newly attached to stable player slots.
    #[must_use]
    pub const fn attached(self) -> usize {
        self.attached
    }

    /// Controllers connected after the scan.
    #[must_use]
    pub const fn connected(self) -> usize {
        self.connected
    }
}

/// Input discovery or read failure with the affected device class preserved.
#[derive(Debug)]
pub enum InputError {
    /// The evdev directory could not be enumerated safely.
    Scan {
        /// Directory being enumerated.
        path: PathBuf,
        /// Underlying filesystem failure.
        source: io::Error,
    },
    /// No exact, correctly ranged Deck touchscreen was available.
    TouchscreenNotFound,
    /// Touchscreen setup or reading failed.
    Touchscreen {
        /// Operation that failed.
        operation: &'static str,
        /// Underlying evdev failure.
        source: io::Error,
    },
    /// A discovered controller could no longer be read.
    Controller {
        /// Stable controller slot.
        player: Player,
        /// Underlying evdev failure.
        source: io::Error,
    },
    /// Waiting across input and display descriptors failed.
    Poll(rustix::io::Errno),
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan { path, source } => {
                write!(formatter, "cannot scan {}: {source}", path.display())
            }
            Self::TouchscreenNotFound => formatter
                .write_str("Goodix Capacitive TouchScreen with 1280x480 axes was not found"),
            Self::Touchscreen { operation, source } => {
                write!(formatter, "touchscreen {operation} failed: {source}")
            }
            Self::Controller { player, source } => {
                write!(formatter, "controller {player:?} read failed: {source}")
            }
            Self::Poll(source) => write!(formatter, "input poll failed: {source}"),
        }
    }
}

impl Error for InputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Scan { source, .. }
            | Self::Touchscreen { source, .. }
            | Self::Controller { source, .. } => Some(source),
            Self::Poll(source) => Some(source),
            Self::TouchscreenNotFound => None,
        }
    }
}

#[derive(Debug)]
struct TouchDevice {
    device: Device,
    tracker: TouchTracker,
}

#[derive(Debug)]
struct ControllerCandidate {
    device: Device,
    path: PathBuf,
    physical_path: String,
    tracker: ControllerTracker,
}

#[derive(Debug)]
struct ControllerDevice {
    device: Device,
    path: PathBuf,
    player: Player,
    tracker: ControllerTracker,
}

/// Exclusively grabbed touchscreen and up to two ordered `THEGamepads`.
#[derive(Debug)]
pub struct InputDevices {
    touch: TouchDevice,
    controllers: Vec<ControllerDevice>,
}

/// Up to two ordered `THEGamepads` without touchscreen ownership.
///
/// Gameplay processes use this device set so the dashboard supervisor keeps
/// receiving touchscreen reports for its hold-to-exit gesture.
#[derive(Debug)]
pub struct ControllerDevices {
    input_directory: PathBuf,
    controllers: Vec<ControllerDevice>,
    remembered_paths: [Option<String>; MAXIMUM_CONTROLLERS],
}

/// Exclusively grabbed Deck touchscreen without controller descriptors.
#[derive(Debug)]
pub struct TouchscreenDevice {
    touch: TouchDevice,
}

impl InputDevices {
    /// Discover the production Deck input devices below `/dev/input`.
    ///
    /// The touchscreen is grabbed exclusively. Controllers remain shared with
    /// emulator processes and are ordered by physical USB path, then event path.
    /// All descriptors are nonblocking before this function returns.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when the input directory cannot be scanned or the
    /// exact Deck touchscreen cannot be initialized and grabbed.
    pub fn discover() -> Result<Self, InputError> {
        Self::discover_in(Path::new(INPUT_DIRECTORY))
    }

    fn discover_in(input_directory: &Path) -> Result<Self, InputError> {
        let paths = event_paths(input_directory)?;
        let mut touch = None;
        let mut controller_candidates = Vec::new();

        for path in paths {
            let Ok(device) = Device::open(&path) else {
                continue;
            };
            if touch.is_none() && is_deck_touchscreen(&device) {
                touch = Some(configure_touchscreen(device)?);
                continue;
            }
            if is_the_gamepad(&device) {
                if let Some(candidate) = configure_controller(device, path) {
                    controller_candidates.push(candidate);
                }
            }
        }

        let controllers = order_controllers(controller_candidates);

        Ok(Self {
            touch: touch.ok_or(InputError::TouchscreenNotFound)?,
            controllers,
        })
    }

    /// Number of supported controllers ready at discovery time.
    #[must_use]
    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }

    /// Current complete semantic button state for one stable controller slot.
    #[must_use]
    pub fn buttons(&self, player: Player) -> ButtonSet {
        self.controllers
            .iter()
            .find(|controller| controller.player == player)
            .map_or_else(ButtonSet::empty, |controller| controller.tracker.state())
    }

    /// Borrow all descriptors for integration into a platform event poll.
    pub fn file_descriptors(&self) -> impl Iterator<Item = BorrowedFd<'_>> + '_ {
        std::iter::once(self.touch.device.as_fd()).chain(
            self.controllers
                .iter()
                .map(|controller| controller.device.as_fd()),
        )
    }

    /// Wait for input, Wayland, or a fixed runtime deadline without allocating.
    ///
    /// The supplied descriptor normally belongs to the application's Wayland
    /// presentation. A timeout is successful and lets the caller advance its
    /// monotonic model even when no descriptor became readable.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Poll`] for an operating-system polling failure.
    pub fn wait_readable_with(
        &self,
        additional: BorrowedFd<'_>,
        timeout: Duration,
    ) -> Result<(), InputError> {
        let touch = self.touch.device.as_fd();
        match self.controllers.as_slice() {
            [] => wait_on([touch, additional], timeout),
            [first] => wait_on([touch, first.device.as_fd(), additional], timeout),
            [first, second, ..] => wait_on(
                [
                    touch,
                    first.device.as_fd(),
                    second.device.as_fd(),
                    additional,
                ],
                timeout,
            ),
        }
    }

    /// Drain all currently available reports without waiting.
    ///
    /// At most 64 normalized events are appended per call. Device state is
    /// still consumed and synchronized after that bound, preventing a flood
    /// from growing memory or replaying stale edges later.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] if a discovered device disconnects or reports an
    /// unrecoverable read error. `WouldBlock` is normal completion.
    pub fn drain_into(&mut self, output: &mut Vec<InputEvent>) -> Result<DrainStats, InputError> {
        let mut collector = EventCollector::new(output);
        drain_touchscreen(&mut self.touch, &mut |event| collector.emit(event))?;
        for controller in &mut self.controllers {
            drain_controller(controller, &mut |event| collector.emit(event))?;
        }
        Ok(collector.stats())
    }
}

impl ControllerDevices {
    /// Discover shared production gamepads below `/dev/input`.
    ///
    /// No touchscreen is opened or grabbed. Controllers are ordered by
    /// physical USB path, then event path, exactly like [`InputDevices`]. An
    /// empty controller set is valid so emulation can continue unattended.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Scan`] when the input directory cannot be
    /// enumerated safely.
    pub fn discover() -> Result<Self, InputError> {
        Self::discover_in(Path::new(INPUT_DIRECTORY))
    }

    fn discover_in(input_directory: &Path) -> Result<Self, InputError> {
        let mut devices = Self {
            input_directory: input_directory.to_owned(),
            controllers: Vec::new(),
            remembered_paths: [None, None],
        };
        let candidates = devices.scan_candidates()?;
        let _attached = devices.attach_candidates(candidates);
        Ok(devices)
    }

    /// Number of supported controllers ready at discovery time.
    #[must_use]
    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }

    /// Current complete semantic button state for one stable controller slot.
    #[must_use]
    pub fn buttons(&self, player: Player) -> ButtonSet {
        self.controllers
            .iter()
            .find(|controller| controller.player == player)
            .map_or_else(ButtonSet::empty, |controller| controller.tracker.state())
    }

    /// Borrow connected controller descriptors for aggregate polling.
    pub fn file_descriptors(&self) -> impl Iterator<Item = BorrowedFd<'_>> + '_ {
        self.controllers
            .iter()
            .map(|controller| controller.device.as_fd())
    }

    /// Scan for newly connected controllers and preserve remembered slots.
    ///
    /// Connected descriptors remain open and are never replaced by a scan.
    /// Reconnected physical USB paths reclaim their previous player before a
    /// new controller may fill an unused or disconnected slot.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Scan`] without disturbing connected controllers
    /// when the input directory cannot be enumerated.
    pub fn rescan(&mut self) -> Result<ControllerScanStats, InputError> {
        let candidates = self.scan_candidates()?;
        let attached = self.attach_candidates(candidates);
        Ok(ControllerScanStats {
            attached,
            connected: self.controllers.len(),
        })
    }

    /// Wait for controller or Wayland readiness until the runtime deadline.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Poll`] for an operating-system polling failure.
    pub fn wait_readable_with(
        &self,
        additional: BorrowedFd<'_>,
        timeout: Duration,
    ) -> Result<(), InputError> {
        match self.controllers.as_slice() {
            [] => wait_on([additional], timeout),
            [first] => wait_on([first.device.as_fd(), additional], timeout),
            [first, second, ..] => wait_on(
                [first.device.as_fd(), second.device.as_fd(), additional],
                timeout,
            ),
        }
    }

    /// Drain all currently available controller reports without waiting.
    ///
    /// At most 64 normalized events are appended per call. Device state is
    /// consumed after that bound so stale edges cannot be replayed later. An
    /// unreadable controller is closed, its complete state becomes empty, and
    /// its stable player slot is reported through [`DrainStats::disconnected`].
    pub fn drain_into(&mut self, output: &mut Vec<InputEvent>) -> DrainStats {
        let mut collector = EventCollector::new(output);
        let mut index = 0;
        while index < self.controllers.len() {
            let drained = self
                .controllers
                .get_mut(index)
                .map_or(Ok(()), |controller| {
                    drain_controller(controller, &mut |event| collector.emit(event))
                });
            if drained.is_ok() {
                index += 1;
            } else {
                let controller = self.controllers.remove(index);
                collector.disconnect(controller.player);
            }
        }
        collector.stats()
    }

    fn scan_candidates(&self) -> Result<Vec<ControllerCandidate>, InputError> {
        let paths = event_paths(&self.input_directory)?;
        let mut candidates = Vec::new();
        for path in paths {
            if self
                .controllers
                .iter()
                .any(|controller| controller.path == path)
            {
                continue;
            }
            let Ok(device) = Device::open(&path) else {
                continue;
            };
            if !is_the_gamepad(&device) {
                continue;
            }
            let Some(candidate) = configure_controller(device, path) else {
                continue;
            };
            if self.connected_physical_path(&candidate.physical_path) {
                continue;
            }
            candidates.push(candidate);
        }
        sort_candidates(&mut candidates);
        Ok(candidates)
    }

    fn connected_physical_path(&self, physical_path: &str) -> bool {
        self.controllers.iter().any(|controller| {
            remembered_path(&self.remembered_paths, controller.player) == Some(physical_path)
        })
    }

    fn attach_candidates(&mut self, candidates: Vec<ControllerCandidate>) -> usize {
        let connected = PLAYERS.map(|player| {
            self.controllers
                .iter()
                .any(|controller| controller.player == player)
        });
        let remembered = PLAYERS.map(|player| remembered_path(&self.remembered_paths, player));
        let physical_paths = candidates
            .iter()
            .map(|candidate| candidate.physical_path.as_str())
            .collect::<Vec<_>>();
        let assignments = plan_controller_slots(connected, remembered, &physical_paths);
        let mut candidates = candidates.into_iter().map(Some).collect::<Vec<_>>();
        let mut attached = 0;

        for (slot, candidate_index) in assignments.into_iter().enumerate() {
            let Some(candidate_index) = candidate_index else {
                continue;
            };
            let Some(candidate) = candidates.get_mut(candidate_index).and_then(Option::take) else {
                continue;
            };
            let Some(player) = PLAYERS.get(slot).copied() else {
                continue;
            };
            if let Some(memory) = self.remembered_paths.get_mut(slot) {
                *memory = Some(candidate.physical_path.clone());
            }
            self.controllers
                .push(assigned_controller(candidate, player));
            attached += 1;
        }
        self.controllers
            .sort_by_key(|controller| player_index(controller.player));
        attached
    }
}

impl TouchscreenDevice {
    /// Discover and exclusively grab the production Deck touchscreen.
    ///
    /// # Errors
    ///
    /// Returns [`InputError`] when input discovery fails, no exact Deck
    /// touchscreen exists, or the touchscreen cannot be configured.
    pub fn discover() -> Result<Self, InputError> {
        Self::discover_in(Path::new(INPUT_DIRECTORY))
    }

    fn discover_in(input_directory: &Path) -> Result<Self, InputError> {
        for path in event_paths(input_directory)? {
            let Ok(device) = Device::open(path) else {
                continue;
            };
            if is_deck_touchscreen(&device) {
                return configure_touchscreen(device).map(|touch| Self { touch });
            }
        }
        Err(InputError::TouchscreenNotFound)
    }

    /// Wait for touchscreen or another runtime descriptor until a deadline.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Poll`] for an operating-system polling failure.
    pub fn wait_readable_with(
        &self,
        additional: BorrowedFd<'_>,
        timeout: Duration,
    ) -> Result<(), InputError> {
        wait_on([self.touch.device.as_fd(), additional], timeout)
    }

    /// Drain available reports and return the latest complete touch state.
    ///
    /// # Errors
    ///
    /// Returns [`InputError::Touchscreen`] when the device disconnects or a
    /// non-recoverable read fails.
    pub fn drain(&mut self) -> Result<TouchState, InputError> {
        drain_touchscreen(&mut self.touch, &mut |_event| {})?;
        Ok(self.touch.tracker.state())
    }

    /// Touch state captured during discovery or the latest drain.
    #[must_use]
    pub const fn state(&self) -> TouchState {
        self.touch.tracker.state()
    }
}

impl Default for ControllerDevices {
    fn default() -> Self {
        Self {
            input_directory: PathBuf::from(INPUT_DIRECTORY),
            controllers: Vec::new(),
            remembered_paths: [None, None],
        }
    }
}

fn order_controllers(mut candidates: Vec<ControllerCandidate>) -> Vec<ControllerDevice> {
    sort_candidates(&mut candidates);
    candidates
        .into_iter()
        .take(MAXIMUM_CONTROLLERS)
        .enumerate()
        .filter_map(|(index, candidate)| {
            PLAYERS
                .get(index)
                .copied()
                .map(|player| assigned_controller(candidate, player))
        })
        .collect()
}

fn sort_candidates(candidates: &mut Vec<ControllerCandidate>) {
    candidates.sort_by(|left, right| {
        left.physical_path
            .cmp(&right.physical_path)
            .then_with(|| left.path.cmp(&right.path))
    });
    candidates.dedup_by(|left, right| left.physical_path == right.physical_path);
}

fn assigned_controller(candidate: ControllerCandidate, player: Player) -> ControllerDevice {
    ControllerDevice {
        device: candidate.device,
        path: candidate.path,
        player,
        tracker: candidate.tracker,
    }
}

fn plan_controller_slots(
    connected: [bool; MAXIMUM_CONTROLLERS],
    remembered: [Option<&str>; MAXIMUM_CONTROLLERS],
    candidates: &[&str],
) -> [Option<usize>; MAXIMUM_CONTROLLERS] {
    let mut used = vec![false; candidates.len()];
    let mut assignments = [None; MAXIMUM_CONTROLLERS];

    for (slot, physical_path) in remembered.iter().copied().enumerate() {
        if connected.get(slot).copied().unwrap_or(true) {
            continue;
        }
        let Some(physical_path) = physical_path else {
            continue;
        };
        if let Some(assignment) = assignments.get_mut(slot) {
            *assignment = claim_candidate(&mut used, candidates, |candidate| {
                candidate == physical_path
            });
        }
    }
    for (slot, physical_path) in remembered.iter().enumerate() {
        if connected.get(slot).copied().unwrap_or(true) || physical_path.is_some() {
            continue;
        }
        if let Some(assignment) = assignments.get_mut(slot) {
            *assignment = claim_candidate(&mut used, candidates, |_| true);
        }
    }
    for slot in 0..MAXIMUM_CONTROLLERS {
        let already_filled = assignments.get(slot).copied().flatten().is_some();
        if connected.get(slot).copied().unwrap_or(true) || already_filled {
            continue;
        }
        if let Some(assignment) = assignments.get_mut(slot) {
            *assignment = claim_candidate(&mut used, candidates, |_| true);
        }
    }
    assignments
}

fn claim_candidate(
    used: &mut [bool],
    candidates: &[&str],
    mut matches: impl FnMut(&str) -> bool,
) -> Option<usize> {
    let index = candidates
        .iter()
        .enumerate()
        .find_map(|(index, candidate)| {
            (!used.get(index).copied().unwrap_or(true) && matches(candidate)).then_some(index)
        })?;
    let claimed = used.get_mut(index)?;
    *claimed = true;
    Some(index)
}

const fn player_index(player: Player) -> usize {
    match player {
        Player::One => 0,
        Player::Two => 1,
    }
}

const fn player_mask(player: Player) -> u8 {
    1 << player_index(player)
}

fn remembered_path(
    remembered_paths: &[Option<String>; MAXIMUM_CONTROLLERS],
    player: Player,
) -> Option<&str> {
    remembered_paths
        .get(player_index(player))
        .and_then(Option::as_deref)
}

fn wait_on<const COUNT: usize>(
    descriptors: [BorrowedFd<'_>; COUNT],
    timeout: Duration,
) -> Result<(), InputError> {
    let flags = PollFlags::IN | PollFlags::ERR | PollFlags::HUP;
    let mut descriptors = descriptors.map(|descriptor| PollFd::from_borrowed_fd(descriptor, flags));
    match poll(&mut descriptors, Some(&duration_timespec(timeout))) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
        Err(source) => Err(InputError::Poll(source)),
    }
}

struct EventCollector<'output> {
    output: &'output mut Vec<InputEvent>,
    emitted: usize,
    dropped: usize,
    disconnected_players: u8,
}

impl<'output> EventCollector<'output> {
    const fn new(output: &'output mut Vec<InputEvent>) -> Self {
        Self {
            output,
            emitted: 0,
            dropped: 0,
            disconnected_players: 0,
        }
    }

    fn emit(&mut self, event: InputEvent) {
        if self.emitted < MAXIMUM_EVENTS_PER_DRAIN {
            self.output.push(event);
            self.emitted += 1;
        } else {
            self.dropped += 1;
        }
    }

    const fn disconnect(&mut self, player: Player) {
        self.disconnected_players |= player_mask(player);
    }

    const fn stats(&self) -> DrainStats {
        DrainStats {
            emitted: self.emitted,
            dropped: self.dropped,
            disconnected_players: self.disconnected_players,
        }
    }
}

pub(super) fn event_paths(input_directory: &Path) -> Result<Vec<PathBuf>, InputError> {
    let entries = fs::read_dir(input_directory).map_err(|source| InputError::Scan {
        path: input_directory.to_owned(),
        source,
    })?;
    let mut indexed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| InputError::Scan {
            path: input_directory.to_owned(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| InputError::Scan {
            path: input_directory.to_owned(),
            source,
        })?;
        if file_type.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(index) = event_index(&name) else {
            continue;
        };
        indexed.push((index, entry.path()));
    }
    indexed.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(indexed.into_iter().map(|(_, path)| path).collect())
}

fn event_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("event")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn is_deck_touchscreen(device: &Device) -> bool {
    if device.name() != Some(TOUCHSCREEN_NAME) {
        return false;
    }
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    if !keys.contains(KeyCode::BTN_TOUCH) {
        return false;
    }
    let Ok(Some(x)) = axis_info(device, AbsoluteAxisCode::ABS_X) else {
        return false;
    };
    let Ok(Some(y)) = axis_info(device, AbsoluteAxisCode::ABS_Y) else {
        return false;
    };
    x.minimum() == 0
        && x.maximum() == i32::from(LOGICAL_WIDTH - 1)
        && y.minimum() == 0
        && y.maximum() == i32::from(LOGICAL_HEIGHT - 1)
}

fn configure_touchscreen(mut device: Device) -> Result<TouchDevice, InputError> {
    device
        .set_nonblocking(true)
        .map_err(|source| InputError::Touchscreen {
            operation: "nonblocking setup",
            source,
        })?;
    let x = required_axis_info(&device, AbsoluteAxisCode::ABS_X, "X axis query")?;
    let y = required_axis_info(&device, AbsoluteAxisCode::ABS_Y, "Y axis query")?;
    let down = device
        .get_key_state()
        .map_err(|source| InputError::Touchscreen {
            operation: "key state query",
            source,
        })?
        .contains(KeyCode::BTN_TOUCH);
    device.grab().map_err(|source| InputError::Touchscreen {
        operation: "exclusive grab",
        source,
    })?;
    Ok(TouchDevice {
        device,
        tracker: TouchTracker::deck(x.value(), y.value(), down),
    })
}

fn required_axis_info(
    device: &Device,
    axis: AbsoluteAxisCode,
    operation: &'static str,
) -> Result<AbsInfo, InputError> {
    axis_info(device, axis)
        .map_err(|source| InputError::Touchscreen { operation, source })?
        .ok_or_else(|| InputError::Touchscreen {
            operation,
            source: io::Error::new(io::ErrorKind::InvalidData, "required axis disappeared"),
        })
}

fn is_the_gamepad(device: &Device) -> bool {
    let identity = device.input_id();
    identity.vendor() == THE_GAMEPAD_VENDOR && identity.product() == THE_GAMEPAD_PRODUCT
}

fn configure_controller(device: Device, path: PathBuf) -> Option<ControllerCandidate> {
    device.set_nonblocking(true).ok()?;
    let x = axis_info(&device, AbsoluteAxisCode::ABS_X).ok()??;
    let y = axis_info(&device, AbsoluteAxisCode::ABS_Y).ok()??;
    let x_range = AxisRange::new(x.minimum(), x.maximum())?;
    let y_range = AxisRange::new(y.minimum(), y.maximum())?;
    let key_state = device.get_key_state().ok()?;
    let pressed = physical_buttons().filter(|(code, _)| key_state.contains(*code));
    let tracker = ControllerTracker::new(
        x_range,
        y_range,
        x.value(),
        y.value(),
        pressed.map(|(_, button)| button),
    );
    let physical_path = device
        .physical_path()
        .filter(|physical_path| !physical_path.is_empty())
        .map_or_else(|| path.to_string_lossy().into_owned(), str::to_owned);
    Some(ControllerCandidate {
        device,
        path,
        physical_path,
        tracker,
    })
}

fn axis_info(device: &Device, axis: AbsoluteAxisCode) -> io::Result<Option<AbsInfo>> {
    Ok(device
        .get_absinfo()?
        .find_map(|(candidate, info)| (candidate == axis).then_some(info)))
}

fn physical_buttons() -> impl Iterator<Item = (KeyCode, PhysicalButton)> {
    [
        (KeyCode::BTN_TRIGGER, PhysicalButton::Y),
        (KeyCode::BTN_THUMB, PhysicalButton::B),
        (KeyCode::BTN_THUMB2, PhysicalButton::A),
        (KeyCode::BTN_TOP, PhysicalButton::X),
        (KeyCode::BTN_TOP2, PhysicalButton::L),
        (KeyCode::BTN_PINKIE, PhysicalButton::R),
        (KeyCode::BTN_BASE, PhysicalButton::Back),
        (KeyCode::BTN_BASE2, PhysicalButton::Start),
    ]
    .into_iter()
}

fn physical_button(code: KeyCode) -> Option<PhysicalButton> {
    physical_buttons().find_map(|(candidate, button)| (candidate == code).then_some(button))
}

fn drain_touchscreen(
    touch: &mut TouchDevice,
    emit: &mut impl FnMut(InputEvent),
) -> Result<(), InputError> {
    let TouchDevice { device, tracker } = touch;
    loop {
        let events = match device.fetch_events() {
            Ok(events) => events,
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(source) => {
                return Err(InputError::Touchscreen {
                    operation: "read",
                    source,
                });
            }
        };
        let mut count = 0_usize;
        for event in events {
            count += 1;
            match event.destructure() {
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_X, value) => {
                    tracker.set_x(value);
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_Y, value) => {
                    tracker.set_y(value);
                }
                EventSummary::Key(_, KeyCode::BTN_TOUCH, value) => {
                    tracker.set_down(value != 0);
                }
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    tracker.finish_report(emit);
                }
                _ => {}
            }
        }
        if count == 0 {
            return Err(InputError::Touchscreen {
                operation: "read",
                source: io::Error::new(io::ErrorKind::UnexpectedEof, "device disconnected"),
            });
        }
    }
}

fn drain_controller(
    controller: &mut ControllerDevice,
    emit: &mut impl FnMut(InputEvent),
) -> Result<(), InputError> {
    let ControllerDevice {
        device,
        player,
        tracker,
        ..
    } = controller;
    loop {
        let events = match device.fetch_events() {
            Ok(events) => events,
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(source) => {
                return Err(InputError::Controller {
                    player: *player,
                    source,
                });
            }
        };
        let mut count = 0_usize;
        for event in events {
            count += 1;
            match event.destructure() {
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_X, value) => {
                    tracker.set_x(value);
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_Y, value) => {
                    tracker.set_y(value);
                }
                EventSummary::Key(_, code, value) => {
                    if let Some(button) = physical_button(code) {
                        tracker.set_button(button, value != 0);
                    }
                }
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    tracker.finish_report(*player, emit);
                }
                _ => {}
            }
        }
        if count == 0 {
            return Err(InputError::Controller {
                player: *player,
                source: io::Error::new(io::ErrorKind::UnexpectedEof, "device disconnected"),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_strict_and_numeric() {
        assert_eq!(event_index("event0"), Some(0));
        assert_eq!(event_index("event19"), Some(19));
        assert_eq!(event_index("event"), None);
        assert_eq!(event_index("event-1"), None);
        assert_eq!(event_index("event1x"), None);
        assert_eq!(event_index("mouse0"), None);
    }

    #[test]
    fn physical_linux_mapping_matches_the_gamepad() {
        assert_eq!(
            physical_button(KeyCode::BTN_TRIGGER),
            Some(PhysicalButton::Y)
        );
        assert_eq!(physical_button(KeyCode::BTN_THUMB), Some(PhysicalButton::B));
        assert_eq!(
            physical_button(KeyCode::BTN_THUMB2),
            Some(PhysicalButton::A)
        );
        assert_eq!(physical_button(KeyCode::BTN_TOP), Some(PhysicalButton::X));
        assert_eq!(physical_button(KeyCode::BTN_TOP2), Some(PhysicalButton::L));
        assert_eq!(
            physical_button(KeyCode::BTN_PINKIE),
            Some(PhysicalButton::R)
        );
        assert_eq!(
            physical_button(KeyCode::BTN_BASE),
            Some(PhysicalButton::Back)
        );
        assert_eq!(
            physical_button(KeyCode::BTN_BASE2),
            Some(PhysicalButton::Start)
        );
        assert_eq!(physical_button(KeyCode::KEY_SPACE), None);
    }

    #[test]
    fn collector_has_a_fixed_per_drain_bound() {
        let mut output = vec![InputEvent::TouchPressed(super::super::TouchPoint {
            x: 1,
            y: 2,
        })];
        let mut collector = EventCollector::new(&mut output);
        for _ in 0..(MAXIMUM_EVENTS_PER_DRAIN + 3) {
            collector.emit(InputEvent::TouchPressed(super::super::TouchPoint {
                x: 3,
                y: 4,
            }));
        }
        assert_eq!(
            collector.stats(),
            DrainStats {
                emitted: MAXIMUM_EVENTS_PER_DRAIN,
                dropped: 3,
                disconnected_players: 0,
            }
        );
        assert_eq!(output.len(), MAXIMUM_EVENTS_PER_DRAIN + 1);
    }

    #[test]
    fn empty_controller_set_is_a_valid_gameplay_fallback() {
        let mut controllers = ControllerDevices::default();
        assert_eq!(controllers.controller_count(), 0);
        assert_eq!(controllers.buttons(Player::One), ButtonSet::empty());
        assert_eq!(controllers.buttons(Player::Two), ButtonSet::empty());
        let mut events = Vec::new();
        assert_eq!(controllers.drain_into(&mut events), DrainStats::default());
        assert!(events.is_empty());
    }

    #[test]
    fn initial_controllers_fill_player_slots_in_candidate_order() {
        assert_eq!(
            plan_controller_slots([false, false], [None, None], &["usb-a", "usb-b", "usb-c"]),
            [Some(0), Some(1)]
        );
    }

    #[test]
    fn remembered_physical_paths_reclaim_their_player_slots() {
        assert_eq!(
            plan_controller_slots(
                [false, false],
                [Some("usb-a"), Some("usb-b")],
                &["usb-b", "usb-a"]
            ),
            [Some(1), Some(0)]
        );
        assert_eq!(
            plan_controller_slots([true, false], [Some("usb-a"), Some("usb-b")], &["usb-b"]),
            [None, Some(0)]
        );
    }

    #[test]
    fn a_different_controller_can_replace_a_missing_remembered_path() {
        assert_eq!(
            plan_controller_slots(
                [false, true],
                [Some("missing"), Some("connected")],
                &["replacement"]
            ),
            [Some(0), None]
        );
        assert_eq!(
            plan_controller_slots([false, false], [Some("missing"), Some("usb-b")], &["usb-b"]),
            [None, Some(0)]
        );
    }

    #[test]
    fn drain_stats_identify_each_disconnected_player() {
        let stats = DrainStats {
            disconnected_players: player_mask(Player::One) | player_mask(Player::Two),
            ..DrainStats::default()
        };
        assert!(stats.disconnected(Player::One));
        assert!(stats.disconnected(Player::Two));
        assert_eq!(stats.disconnected_count(), 2);
    }
}
