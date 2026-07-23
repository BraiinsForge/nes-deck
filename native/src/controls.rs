use evdev::raw_stream::RawDevice;
use evdev::{AbsoluteAxisCode, EventSummary, InputEvent, KeyCode, SynchronizationCode};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{Mode, OFlags, open as open_file};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::io::{self, ErrorKind};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const KEYBOARD_REPORT: u32 = 0;
pub const GAMEPAD_REPORT: u32 = 1;
pub const KEYBOARD_SHIFT: u32 = 1;
pub const KEYBOARD_REPEAT: u32 = 2;

const GAMEPAD_X_NEGATIVE: u32 = 1 << 8;
const GAMEPAD_X_POSITIVE: u32 = 1 << 9;
const GAMEPAD_Y_NEGATIVE: u32 = 1 << 10;
const GAMEPAD_Y_POSITIVE: u32 = 1 << 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlReport {
    pub kind: u32,
    pub value: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlAction {
    Ignore,
    Report(ControlReport),
    Resynchronize,
}

#[derive(Clone, Copy, Debug, Default)]
struct KeyboardState {
    left_shift: bool,
    right_shift: bool,
    dropping_events: bool,
}

impl KeyboardState {
    fn handle(&mut self, event: InputEvent) -> ControlAction {
        if self.dropping_events {
            return match event.destructure() {
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    ControlAction::Resynchronize
                }
                _ => ControlAction::Ignore,
            };
        }

        match event.destructure() {
            EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                self.dropping_events = true;
                ControlAction::Ignore
            }
            EventSummary::Key(_, KeyCode::KEY_LEFTSHIFT, value) => {
                self.left_shift = value != 0;
                ControlAction::Ignore
            }
            EventSummary::Key(_, KeyCode::KEY_RIGHTSHIFT, value) => {
                self.right_shift = value != 0;
                ControlAction::Ignore
            }
            EventSummary::Key(_, code, value) => {
                let repeat = value == 2 && keyboard_key_repeats(code);
                if value != 1 && !repeat {
                    return ControlAction::Ignore;
                }
                let mut flags = 0;
                if self.left_shift || self.right_shift {
                    flags |= KEYBOARD_SHIFT;
                }
                if repeat {
                    flags |= KEYBOARD_REPEAT;
                }
                ControlAction::Report(ControlReport {
                    kind: KEYBOARD_REPORT,
                    value: u32::from(code.0),
                    flags,
                })
            }
            _ => ControlAction::Ignore,
        }
    }

    fn resynchronize(&mut self, left_shift: bool, right_shift: bool) {
        self.left_shift = left_shift;
        self.right_shift = right_shift;
        self.dropping_events = false;
    }
}

fn keyboard_key_repeats(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::KEY_UP | KeyCode::KEY_DOWN | KeyCode::KEY_LEFT | KeyCode::KEY_RIGHT
    )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AxisInfo {
    minimum: i32,
    maximum: i32,
    value: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct GamepadSnapshot {
    x: AxisInfo,
    y: AxisInfo,
    raw_buttons: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct GamepadState {
    snapshot: GamepadSnapshot,
    state: u32,
    dropping_events: bool,
}

impl GamepadState {
    #[cfg(test)]
    fn new(snapshot: GamepadSnapshot) -> Self {
        let mut state = Self::default();
        state.resynchronize(snapshot);
        state
    }

    fn handle(&mut self, event: InputEvent) -> ControlAction {
        if self.dropping_events {
            return match event.destructure() {
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    ControlAction::Resynchronize
                }
                _ => ControlAction::Ignore,
            };
        }

        match event.destructure() {
            EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                self.dropping_events = true;
                ControlAction::Ignore
            }
            EventSummary::Key(_, code, value)
                if code.0 >= KeyCode::BTN_TRIGGER.0 && code.0 <= KeyCode::BTN_BASE2.0 =>
            {
                let bit = 1 << (code.0 - KeyCode::BTN_TRIGGER.0);
                if value != 0 {
                    self.snapshot.raw_buttons |= bit;
                } else {
                    self.snapshot.raw_buttons &= !bit;
                }
                ControlAction::Ignore
            }
            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_X, value) => {
                self.snapshot.x.value = value;
                ControlAction::Ignore
            }
            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_Y, value) => {
                self.snapshot.y.value = value;
                ControlAction::Ignore
            }
            EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                let state = gamepad_state(self.snapshot);
                let pressed = state & !self.state;
                self.state = state;
                if pressed == 0 {
                    ControlAction::Ignore
                } else {
                    ControlAction::Report(ControlReport {
                        kind: GAMEPAD_REPORT,
                        value: pressed,
                        flags: 0,
                    })
                }
            }
            _ => ControlAction::Ignore,
        }
    }

    fn resynchronize(&mut self, snapshot: GamepadSnapshot) {
        self.snapshot = snapshot;
        self.state = gamepad_state(snapshot);
        self.dropping_events = false;
    }
}

fn gamepad_state(snapshot: GamepadSnapshot) -> u32 {
    snapshot.raw_buttons
        | axis_state(snapshot.x, GAMEPAD_X_NEGATIVE, GAMEPAD_X_POSITIVE)
        | axis_state(snapshot.y, GAMEPAD_Y_NEGATIVE, GAMEPAD_Y_POSITIVE)
}

fn axis_state(axis: AxisInfo, negative: u32, positive: u32) -> u32 {
    if axis.maximum <= axis.minimum {
        return 0;
    }
    let span = i64::from(axis.maximum) - i64::from(axis.minimum);
    let low = axis.minimum + (span / 3) as i32;
    let high = axis.maximum - (span / 3) as i32;
    if axis.value <= low {
        negative
    } else if axis.value >= high {
        positive
    } else {
        0
    }
}

const INPUT_DIRECTORY: &str = "/dev/input";
const THE_GAMEPAD_VENDOR: u16 = 0x1c59;
const THE_GAMEPAD_PRODUCT: u16 = 0x0026;
const MAXIMUM_GAMEPADS: usize = 2;
const MAXIMUM_KEYBOARDS: usize = 4;
const MAXIMUM_CONTROL_REPORTS: usize = 64;

struct GamepadDevice {
    path: PathBuf,
    physical_path: String,
    device: RawDevice,
    state: GamepadState,
}

impl GamepadDevice {
    fn open(path: &Path) -> Option<Self> {
        let device = open_device(path)?;
        let identity = device.input_id();
        if identity.vendor() != THE_GAMEPAD_VENDOR || identity.product() != THE_GAMEPAD_PRODUCT {
            return None;
        }
        let (mut x, mut y) = gamepad_axes(&device).ok()?;
        x.value = 0;
        y.value = 0;
        let physical_path = device
            .physical_path()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Some(Self {
            path: path.to_owned(),
            physical_path,
            device,
            state: GamepadState {
                snapshot: GamepadSnapshot {
                    x,
                    y,
                    raw_buttons: 0,
                },
                state: 0,
                dropping_events: false,
            },
        })
    }

    fn resynchronize(&mut self) -> Result<(), String> {
        let snapshot = gamepad_snapshot(&self.device)
            .map_err(|error| format!("cannot resynchronize {}: {error}", self.path.display()))?;
        self.state.resynchronize(snapshot);
        Ok(())
    }

    fn drain(&mut self, reports: &mut VecDeque<ControlReport>) -> bool {
        loop {
            let events = match self.device.fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => return true,
                Err(_) => return false,
            };
            if events.is_empty() {
                return false;
            }
            for event in events {
                match self.state.handle(event) {
                    ControlAction::Ignore => {}
                    ControlAction::Report(report) => queue_report(reports, report),
                    ControlAction::Resynchronize => {
                        if self.resynchronize().is_err() {
                            return false;
                        }
                    }
                }
            }
        }
    }
}

struct KeyboardDevice {
    path: PathBuf,
    device: RawDevice,
    state: KeyboardState,
    grabbed: bool,
}

impl KeyboardDevice {
    fn open(path: &Path) -> Option<Self> {
        let device = open_device(path)?;
        if !keyboard_capabilities(&device) {
            return None;
        }
        Some(Self {
            path: path.to_owned(),
            device,
            state: KeyboardState::default(),
            grabbed: false,
        })
    }

    fn activate(&mut self) {
        match self.device.grab() {
            Ok(()) => self.grabbed = true,
            Err(error) => eprintln!(
                "retrodeck: warning: cannot exclusively grab {}: {error}",
                self.path.display()
            ),
        }
        let _ = self.resynchronize();
    }

    fn resynchronize(&mut self) -> Result<(), String> {
        let (left_shift, right_shift) = keyboard_snapshot(&self.device)
            .map_err(|error| format!("cannot resynchronize {}: {error}", self.path.display()))?;
        self.state.resynchronize(left_shift, right_shift);
        Ok(())
    }

    fn drain(&mut self, reports: &mut VecDeque<ControlReport>) -> bool {
        loop {
            let events = match self.device.fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => return true,
                Err(_) => return false,
            };
            if events.is_empty() {
                return false;
            }
            for event in events {
                match self.state.handle(event) {
                    ControlAction::Ignore => {}
                    ControlAction::Report(report) => queue_report(reports, report),
                    ControlAction::Resynchronize => {
                        if self.resynchronize().is_err() {
                            return false;
                        }
                    }
                }
            }
        }
    }
}

impl Drop for KeyboardDevice {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = self.device.ungrab();
        }
    }
}

#[derive(Default)]
struct Controls {
    gamepads: Vec<GamepadDevice>,
    keyboards: Vec<KeyboardDevice>,
    reports: VecDeque<ControlReport>,
    rescan_requested: bool,
}

impl Controls {
    fn scan(&mut self) -> Result<(usize, usize), String> {
        let paths = match event_paths() {
            Ok(paths) => paths,
            Err(error) => {
                self.rescan_requested = true;
                return Err(error);
            }
        };
        self.rescan_requested = false;
        let mut gamepads = paths
            .iter()
            .filter_map(|path| GamepadDevice::open(path))
            .collect::<Vec<_>>();
        gamepads.sort_by(|left, right| {
            left.physical_path
                .cmp(&right.physical_path)
                .then_with(|| left.path.cmp(&right.path))
        });
        gamepads.truncate(MAXIMUM_GAMEPADS);

        let mut keyboards = paths
            .iter()
            .filter_map(|path| KeyboardDevice::open(path))
            .collect::<Vec<_>>();
        keyboards.sort_by(|left, right| left.path.cmp(&right.path));
        keyboards.truncate(MAXIMUM_KEYBOARDS);

        let gamepads_changed = self.gamepads.len() != gamepads.len()
            || self.gamepads.iter().zip(&gamepads).any(|(left, right)| {
                left.path != right.path || left.physical_path != right.physical_path
            });
        if gamepads_changed {
            self.gamepads.clear();
            self.gamepads = gamepads;
            for gamepad in &mut self.gamepads {
                let _ = gamepad.resynchronize();
            }
        }

        let keyboards_changed = self.keyboards.len() != keyboards.len()
            || self
                .keyboards
                .iter()
                .zip(&keyboards)
                .any(|(left, right)| left.path != right.path);
        if keyboards_changed {
            self.keyboards.clear();
            self.keyboards = keyboards;
            for keyboard in &mut self.keyboards {
                keyboard.activate();
            }
        }

        if gamepads_changed || keyboards_changed {
            self.reports.clear();
        }
        Ok((self.gamepads.len(), self.keyboards.len()))
    }

    fn dispatch(&mut self, timeout_ms: u32) -> Result<(usize, bool), String> {
        if !self.reports.is_empty() || self.gamepads.is_empty() && self.keyboards.is_empty() {
            return Ok((self.reports.len(), self.rescan_requested));
        }
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec {
                tv_sec: remaining.as_secs() as i64,
                tv_nsec: i64::from(remaining.subsec_nanos()),
            };
            let mut descriptors = self
                .gamepads
                .iter()
                .map(|device| {
                    PollFd::from_borrowed_fd(
                        device.device.as_fd(),
                        PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
                    )
                })
                .chain(self.keyboards.iter().map(|device| {
                    PollFd::from_borrowed_fd(
                        device.device.as_fd(),
                        PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
                    )
                }))
                .collect::<Vec<_>>();
            match poll(&mut descriptors, Some(&timeout)) {
                Ok(0) => return Ok((self.reports.len(), self.rescan_requested)),
                Ok(_) => {
                    let ready = descriptors.iter().map(PollFd::revents).collect::<Vec<_>>();
                    drop(descriptors);
                    self.read_ready(&ready);
                    return Ok((self.reports.len(), self.rescan_requested));
                }
                Err(rustix::io::Errno::INTR) if Instant::now() < deadline => continue,
                Err(rustix::io::Errno::INTR) => {
                    return Ok((self.reports.len(), self.rescan_requested));
                }
                Err(error) => return Err(format!("cannot poll controller input: {error}")),
            }
        }
    }

    fn read_ready(&mut self, ready: &[PollFlags]) {
        let first_keyboard = self.gamepads.len();
        let mut lost_gamepads = Vec::new();
        for (index, gamepad) in self.gamepads.iter_mut().enumerate() {
            let flags = ready.get(index).copied().unwrap_or(PollFlags::empty());
            if !flags.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
            {
                continue;
            }
            if flags.contains(PollFlags::IN) && gamepad.drain(&mut self.reports) {
                continue;
            }
            lost_gamepads.push(index);
        }
        for index in lost_gamepads.into_iter().rev() {
            self.gamepads.remove(index);
            self.rescan_requested = true;
        }

        let mut lost_keyboards = Vec::new();
        for (index, keyboard) in self.keyboards.iter_mut().enumerate() {
            let flags = ready
                .get(first_keyboard + index)
                .copied()
                .unwrap_or(PollFlags::empty());
            if !flags.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
            {
                continue;
            }
            if flags.contains(PollFlags::IN) && keyboard.drain(&mut self.reports) {
                continue;
            }
            lost_keyboards.push(index);
        }
        for index in lost_keyboards.into_iter().rev() {
            self.keyboards.remove(index);
            self.rescan_requested = true;
        }
    }
}

fn queue_report(reports: &mut VecDeque<ControlReport>, report: ControlReport) {
    if report.kind == GAMEPAD_REPORT {
        if let Some(current) = reports
            .iter_mut()
            .find(|current| current.kind == GAMEPAD_REPORT)
        {
            current.value |= report.value;
            return;
        }
    }
    if reports.len() < MAXIMUM_CONTROL_REPORTS && !reports.contains(&report) {
        reports.push_back(report);
    }
}

thread_local! {
    static CONTROLS: RefCell<Controls> = RefCell::new(Controls::default());
}

pub fn scan() -> Result<(usize, usize), String> {
    CONTROLS.with(|controls| controls.borrow_mut().scan())
}

pub fn close() {
    CONTROLS.with(|controls| *controls.borrow_mut() = Controls::default());
}

pub fn dispatch(timeout_ms: u32) -> Result<(usize, bool), String> {
    CONTROLS.with(|controls| controls.borrow_mut().dispatch(timeout_ms))
}

pub fn next_report() -> Option<ControlReport> {
    CONTROLS.with(|controls| controls.borrow_mut().reports.pop_front())
}

fn event_paths() -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(INPUT_DIRECTORY)
        .map_err(|error| format!("cannot open {INPUT_DIRECTORY}: {error}"))?;
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| event_path(entry.path()))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn event_path(path: PathBuf) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let suffix = name.strip_prefix("event")?;
    (!suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())).then_some(path)
}

fn open_device(path: &Path) -> Option<RawDevice> {
    let fd = open_file(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    RawDevice::from_fd(fd).ok()
}

fn keyboard_capabilities(device: &RawDevice) -> bool {
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    keys.contains(KeyCode::KEY_ENTER)
        && keys.contains(KeyCode::KEY_ESC)
        && keys.contains(KeyCode::KEY_TAB)
        && keys.contains(KeyCode::KEY_UP)
        && keys.contains(KeyCode::KEY_DOWN)
        && keys.contains(KeyCode::KEY_LEFT)
        && keys.contains(KeyCode::KEY_RIGHT)
        && (keys.contains(KeyCode::KEY_LEFTSHIFT) || keys.contains(KeyCode::KEY_RIGHTSHIFT))
}

fn keyboard_snapshot(device: &RawDevice) -> io::Result<(bool, bool)> {
    let keys = device.get_key_state()?;
    Ok((
        keys.contains(KeyCode::KEY_LEFTSHIFT),
        keys.contains(KeyCode::KEY_RIGHTSHIFT),
    ))
}

fn gamepad_axes(device: &RawDevice) -> io::Result<(AxisInfo, AxisInfo)> {
    let mut x = None;
    let mut y = None;
    for (code, info) in device.get_absinfo()? {
        let axis = AxisInfo {
            minimum: info.minimum(),
            maximum: info.maximum(),
            value: info.value(),
        };
        match code {
            AbsoluteAxisCode::ABS_X => x = Some(axis),
            AbsoluteAxisCode::ABS_Y => y = Some(axis),
            _ => {}
        }
    }
    let x = x.ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "ABS_X is unavailable"))?;
    let y = y.ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "ABS_Y is unavailable"))?;
    Ok((x, y))
}

fn gamepad_snapshot(device: &RawDevice) -> io::Result<GamepadSnapshot> {
    let (x, y) = gamepad_axes(device)?;
    let keys = device.get_key_state()?;
    let mut raw_buttons = 0;
    for index in 0..8 {
        if keys.contains(KeyCode(KeyCode::BTN_TRIGGER.0 + index)) {
            raw_buttons |= 1 << index;
        }
    }
    Ok(GamepadSnapshot { x, y, raw_buttons })
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::EventType;

    fn event(event_type: EventType, code: u16, value: i32) -> InputEvent {
        InputEvent::new(event_type.0, code, value)
    }

    fn key(code: KeyCode, value: i32) -> InputEvent {
        event(EventType::KEY, code.0, value)
    }

    fn syn(code: SynchronizationCode) -> InputEvent {
        event(EventType::SYNCHRONIZATION, code.0, 0)
    }

    fn axis(code: AbsoluteAxisCode, value: i32) -> InputEvent {
        event(EventType::ABSOLUTE, code.0, value)
    }

    fn gamepad_snapshot(x: i32, y: i32, raw_buttons: u32) -> GamepadSnapshot {
        GamepadSnapshot {
            x: AxisInfo {
                minimum: 0,
                maximum: 255,
                value: x,
            },
            y: AxisInfo {
                minimum: 0,
                maximum: 255,
                value: y,
            },
            raw_buttons,
        }
    }

    #[test]
    fn keyboard_tracks_shift_and_accepts_only_arrow_repeats() {
        let mut state = KeyboardState::default();
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_TAB.0),
                flags: 0,
            })
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_LEFTSHIFT, 1)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_TAB.0),
                flags: KEYBOARD_SHIFT,
            })
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 2)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_RIGHT, 2)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_RIGHT.0),
                flags: KEYBOARD_SHIFT | KEYBOARD_REPEAT,
            })
        );
        state.handle(key(KeyCode::KEY_RIGHTSHIFT, 1));
        state.handle(key(KeyCode::KEY_LEFTSHIFT, 0));
        assert_eq!(
            state.handle(key(KeyCode::KEY_ENTER, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_ENTER.0),
                flags: KEYBOARD_SHIFT,
            })
        );
        state.handle(key(KeyCode::KEY_RIGHTSHIFT, 0));
        assert_eq!(
            state.handle(key(KeyCode::KEY_ENTER, 0)),
            ControlAction::Ignore
        );
    }

    #[test]
    fn keyboard_drop_waits_for_report_and_resynchronizes_without_an_edge() {
        let mut state = KeyboardState::default();
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_DROPPED)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_LEFTSHIFT, 1)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Resynchronize
        );
        state.resynchronize(false, true);
        assert_eq!(
            state.handle(key(KeyCode::KEY_TAB, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_TAB.0),
                flags: KEYBOARD_SHIFT,
            })
        );
    }

    #[test]
    fn gamepad_uses_exact_thirds_and_reports_rising_edges_on_syn_report() {
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 84
                },
                1,
                2
            ),
            1
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 85
                },
                1,
                2
            ),
            1
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 86
                },
                1,
                2
            ),
            0
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 169
                },
                1,
                2
            ),
            0
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 0,
                    maximum: 255,
                    value: 170
                },
                1,
                2
            ),
            2
        );
        assert_eq!(
            axis_state(
                AxisInfo {
                    minimum: 4,
                    maximum: 4,
                    value: 4
                },
                1,
                2
            ),
            0
        );

        let mut state = GamepadState::new(gamepad_snapshot(127, 127, 0));
        state.handle(axis(AbsoluteAxisCode::ABS_X, 255));
        state.handle(key(KeyCode::BTN_THUMB2, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Report(ControlReport {
                kind: GAMEPAD_REPORT,
                value: GAMEPAD_X_POSITIVE | (1 << (KeyCode::BTN_THUMB2.0 - KeyCode::BTN_TRIGGER.0)),
                flags: 0,
            })
        );
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Ignore
        );
        state.handle(key(KeyCode::BTN_THUMB2, 0));
        state.handle(syn(SynchronizationCode::SYN_REPORT));
        state.handle(key(KeyCode::BTN_THUMB2, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Report(ControlReport {
                kind: GAMEPAD_REPORT,
                value: 1 << (KeyCode::BTN_THUMB2.0 - KeyCode::BTN_TRIGGER.0),
                flags: 0,
            })
        );
    }

    #[test]
    fn keyboard_emits_unknown_presses_but_filters_non_arrow_repeats() {
        let mut state = KeyboardState::default();
        assert_eq!(
            state.handle(key(KeyCode::KEY_KPENTER, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_KPENTER.0),
                flags: 0,
            })
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_KPENTER, 2)),
            ControlAction::Ignore
        );
        assert_eq!(
            state.handle(key(KeyCode::KEY_A, 1)),
            ControlAction::Report(ControlReport {
                kind: KEYBOARD_REPORT,
                value: u32::from(KeyCode::KEY_A.0),
                flags: 0,
            })
        );
    }

    #[test]
    fn report_queue_merges_gamepads_deduplicates_keys_and_stays_bounded() {
        let mut reports = VecDeque::new();
        queue_report(
            &mut reports,
            ControlReport {
                kind: GAMEPAD_REPORT,
                value: 1,
                flags: 0,
            },
        );
        queue_report(
            &mut reports,
            ControlReport {
                kind: GAMEPAD_REPORT,
                value: 4,
                flags: 0,
            },
        );
        let key_report = ControlReport {
            kind: KEYBOARD_REPORT,
            value: 28,
            flags: 0,
        };
        queue_report(&mut reports, key_report);
        queue_report(&mut reports, key_report);
        for value in 0..100 {
            queue_report(
                &mut reports,
                ControlReport {
                    kind: KEYBOARD_REPORT,
                    value,
                    flags: KEYBOARD_REPEAT,
                },
            );
        }
        assert_eq!(reports.len(), MAXIMUM_CONTROL_REPORTS);
        assert_eq!(
            reports.front(),
            Some(&ControlReport {
                kind: GAMEPAD_REPORT,
                value: 5,
                flags: 0,
            })
        );
        assert_eq!(
            reports
                .iter()
                .filter(|report| **report == key_report)
                .count(),
            1
        );
    }

    #[test]
    fn event_nodes_require_a_numeric_suffix() {
        assert_eq!(
            event_path(PathBuf::from("/dev/input/event12")),
            Some(PathBuf::from("/dev/input/event12"))
        );
        for path in [
            "/dev/input/event",
            "/dev/input/event2a",
            "/dev/input/mouse0",
        ] {
            assert_eq!(event_path(PathBuf::from(path)), None);
        }
    }

    #[test]
    fn gamepad_preserves_all_eight_raw_button_edges() {
        for index in 0..8 {
            let mut state = GamepadState::new(gamepad_snapshot(127, 127, 0));
            state.handle(key(KeyCode(KeyCode::BTN_TRIGGER.0 + index), 1));
            assert_eq!(
                state.handle(syn(SynchronizationCode::SYN_REPORT)),
                ControlAction::Report(ControlReport {
                    kind: GAMEPAD_REPORT,
                    value: 1 << index,
                    flags: 0,
                })
            );
        }
        let mut state = GamepadState::new(gamepad_snapshot(127, 127, 0));
        state.handle(key(KeyCode::BTN_DEAD, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Ignore
        );
    }

    #[test]
    fn gamepad_drop_uses_snapshot_without_synthesizing_an_edge() {
        let mut state = GamepadState::new(gamepad_snapshot(127, 127, 0));
        state.handle(syn(SynchronizationCode::SYN_DROPPED));
        state.handle(axis(AbsoluteAxisCode::ABS_Y, 255));
        state.handle(key(KeyCode::BTN_BASE, 1));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Resynchronize
        );
        state.resynchronize(gamepad_snapshot(127, 255, 1 << 6));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Ignore
        );
        state.handle(axis(AbsoluteAxisCode::ABS_Y, 127));
        state.handle(syn(SynchronizationCode::SYN_REPORT));
        state.handle(axis(AbsoluteAxisCode::ABS_Y, 255));
        assert_eq!(
            state.handle(syn(SynchronizationCode::SYN_REPORT)),
            ControlAction::Report(ControlReport {
                kind: GAMEPAD_REPORT,
                value: GAMEPAD_Y_POSITIVE,
                flags: 0,
            })
        );
    }
}
