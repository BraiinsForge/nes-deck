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
    AxisRange, ControllerTracker, InputEvent, LOGICAL_HEIGHT, LOGICAL_WIDTH, PhysicalButton,
    Player, TouchTracker,
};
use crate::time::duration_timespec;

const INPUT_DIRECTORY: &str = "/dev/input";
const TOUCHSCREEN_NAME: &str = "Goodix Capacitive TouchScreen";
const THE_GAMEPAD_VENDOR: u16 = 0x1c59;
const THE_GAMEPAD_PRODUCT: u16 = 0x0026;
const MAXIMUM_CONTROLLERS: usize = 2;
const MAXIMUM_EVENTS_PER_DRAIN: usize = 64;

/// Result of one nonblocking drain across every discovered input device.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DrainStats {
    emitted: usize,
    dropped: usize,
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
    player: Player,
    tracker: ControllerTracker,
}

/// Exclusively grabbed touchscreen and up to two ordered `THEGamepads`.
#[derive(Debug)]
pub struct InputDevices {
    touch: TouchDevice,
    controllers: Vec<ControllerDevice>,
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

        controller_candidates.sort_by(|left, right| {
            left.physical_path
                .cmp(&right.physical_path)
                .then_with(|| left.path.cmp(&right.path))
        });
        let controllers = controller_candidates
            .into_iter()
            .take(MAXIMUM_CONTROLLERS)
            .enumerate()
            .map(|(index, candidate)| ControllerDevice {
                device: candidate.device,
                player: if index == 0 { Player::One } else { Player::Two },
                tracker: candidate.tracker,
            })
            .collect();

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
}

impl<'output> EventCollector<'output> {
    const fn new(output: &'output mut Vec<InputEvent>) -> Self {
        Self {
            output,
            emitted: 0,
            dropped: 0,
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

    const fn stats(&self) -> DrainStats {
        DrainStats {
            emitted: self.emitted,
            dropped: self.dropped,
        }
    }
}

fn event_paths(input_directory: &Path) -> Result<Vec<PathBuf>, InputError> {
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
    let physical_path = device.physical_path().unwrap_or_default().to_owned();
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
            }
        );
        assert_eq!(output.len(), MAXIMUM_EVENTS_PER_DRAIN + 1);
    }
}
