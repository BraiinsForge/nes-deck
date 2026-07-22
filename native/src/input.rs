use crate::canvas;
use evdev::raw_stream::RawDevice;
use evdev::{AbsInfo, AbsoluteAxisCode, EventSummary, InputEvent, KeyCode, SynchronizationCode};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{Mode, OFlags, open as open_file};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::io::ErrorKind;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const INPUT_DIRECTORY: &str = "/dev/input";
const TOUCHSCREEN_NAME: &str = "Goodix Capacitive TouchScreen";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchReport {
    pub x: i32,
    pub y: i32,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum TouchAction {
    Ignore,
    Report(TouchReport),
    Resynchronize,
}

#[derive(Debug)]
struct TouchState {
    x: i32,
    y: i32,
    current_down: bool,
    reported_down: bool,
    dropping_events: bool,
}

impl TouchState {
    fn new(x: i32, y: i32, down: bool) -> Self {
        Self {
            x: clamp_x(x),
            y: clamp_y(y),
            current_down: down,
            reported_down: down,
            dropping_events: false,
        }
    }

    fn handle(&mut self, event: InputEvent) -> TouchAction {
        if self.dropping_events {
            return match event.destructure() {
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    TouchAction::Resynchronize
                }
                _ => TouchAction::Ignore,
            };
        }

        match event.destructure() {
            EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                self.dropping_events = true;
                TouchAction::Ignore
            }
            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_X, value) => {
                self.x = clamp_x(value);
                TouchAction::Ignore
            }
            EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_Y, value) => {
                self.y = clamp_y(value);
                TouchAction::Ignore
            }
            EventSummary::Key(_, KeyCode::BTN_TOUCH, value) => {
                self.current_down = value != 0;
                TouchAction::Ignore
            }
            EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                TouchAction::Report(self.report())
            }
            _ => TouchAction::Ignore,
        }
    }

    fn resynchronize(&mut self, x: i32, y: i32, down: bool) -> TouchReport {
        self.x = clamp_x(x);
        self.y = clamp_y(y);
        self.current_down = down;
        self.dropping_events = false;
        self.report()
    }

    fn report(&mut self) -> TouchReport {
        let report = TouchReport {
            x: self.x,
            y: self.y,
            down: self.current_down,
            pressed: self.current_down && !self.reported_down,
            released: !self.current_down && self.reported_down,
        };
        self.reported_down = self.current_down;
        report
    }
}

struct TouchDevice {
    device: RawDevice,
    state: TouchState,
    reports: VecDeque<TouchReport>,
}

impl TouchDevice {
    fn discover() -> Result<Self, String> {
        let entries = fs::read_dir(INPUT_DIRECTORY)
            .map_err(|error| format!("cannot open {INPUT_DIRECTORY}: {error}"))?;
        let mut paths = entries
            .filter_map(Result::ok)
            .filter_map(|entry| event_path(entry.path()))
            .collect::<Vec<_>>();
        paths.sort();

        let mut rejected_reason = None;
        for path in paths {
            let Ok(fd) = open_file(
                &path,
                OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            ) else {
                continue;
            };
            let Ok(mut device) = RawDevice::from_fd(fd) else {
                continue;
            };
            if !device
                .name()
                .is_some_and(|name| name.contains(TOUCHSCREEN_NAME))
            {
                continue;
            }

            let Ok((Some(x), Some(y))) = current_axes(&device) else {
                rejected_reason =
                    Some("Goodix device has unexpected ABS_X/ABS_Y/BTN_TOUCH capabilities");
                continue;
            };
            let has_touch = device
                .supported_keys()
                .is_some_and(|keys| keys.contains(KeyCode::BTN_TOUCH));
            if !valid_capabilities(has_touch, x, y) {
                rejected_reason =
                    Some("Goodix device has unexpected ABS_X/ABS_Y/BTN_TOUCH capabilities");
                continue;
            }
            let down = device
                .get_key_state()
                .map(|keys| keys.contains(KeyCode::BTN_TOUCH))
                .unwrap_or(false);
            if let Err(error) = device.grab() {
                eprintln!(
                    "retrodeck: warning: cannot exclusively grab {}: {error}",
                    path.display()
                );
            }
            return Ok(Self {
                device,
                state: TouchState::new(x.value(), y.value(), down),
                reports: VecDeque::new(),
            });
        }

        Err(rejected_reason
            .unwrap_or("Goodix Capacitive TouchScreen was not found")
            .to_owned())
    }

    fn dispatch(&mut self, timeout_ms: u32) -> Result<usize, String> {
        if !self.reports.is_empty() {
            return Ok(self.reports.len());
        }
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec {
                tv_sec: remaining.as_secs() as i64,
                tv_nsec: i64::from(remaining.subsec_nanos()),
            };
            let mut descriptors = [PollFd::from_borrowed_fd(
                self.device.as_fd(),
                PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
            )];
            match poll(&mut descriptors, Some(&timeout)) {
                Ok(0) => return Ok(0),
                Ok(_) => return self.read_available(),
                Err(rustix::io::Errno::INTR) if Instant::now() < deadline => continue,
                Err(rustix::io::Errno::INTR) => return Ok(0),
                Err(error) => return Err(format!("cannot poll touchscreen: {error}")),
            }
        }
    }

    fn read_available(&mut self) -> Result<usize, String> {
        let initial_count = self.reports.len();
        loop {
            let events = match self.device.fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(format!("touchscreen read failed: {error}")),
            };
            if events.is_empty() {
                return Err("touchscreen disconnected".to_owned());
            }
            for event in events {
                match self.state.handle(event) {
                    TouchAction::Ignore => {}
                    TouchAction::Report(report) => self.reports.push_back(report),
                    TouchAction::Resynchronize => {
                        let report = self.resynchronize();
                        self.reports.push_back(report);
                    }
                }
            }
        }
        Ok(self.reports.len() - initial_count)
    }

    fn resynchronize(&mut self) -> TouchReport {
        let mut x = self.state.x;
        let mut y = self.state.y;
        let mut down = self.state.current_down;
        if let Ok((x_info, y_info)) = current_axes(&self.device) {
            if let Some(info) = x_info {
                x = info.value();
            }
            if let Some(info) = y_info {
                y = info.value();
            }
        }
        if let Ok(keys) = self.device.get_key_state() {
            down = keys.contains(KeyCode::BTN_TOUCH);
        }
        self.state.resynchronize(x, y, down)
    }
}

thread_local! {
    static TOUCH: RefCell<Option<TouchDevice>> = const { RefCell::new(None) };
}

pub fn open_touch() -> Result<(), String> {
    close_touch();
    let touch = TouchDevice::discover()?;
    TOUCH.with(|current| *current.borrow_mut() = Some(touch));
    Ok(())
}

pub fn close_touch() {
    TOUCH.with(|current| {
        current.borrow_mut().take();
    });
}

pub fn dispatch_touch(timeout_ms: u32) -> Result<usize, String> {
    with_touch(|touch| touch.dispatch(timeout_ms))
}

pub fn next_touch() -> Option<TouchReport> {
    TOUCH.with(|current| {
        current
            .borrow_mut()
            .as_mut()
            .and_then(|touch| touch.reports.pop_front())
    })
}

fn with_touch<T>(
    function: impl FnOnce(&mut TouchDevice) -> Result<T, String>,
) -> Result<T, String> {
    TOUCH.with(|current| {
        let mut current = current.borrow_mut();
        let touch = current
            .as_mut()
            .ok_or_else(|| "touchscreen is not open".to_owned())?;
        function(touch)
    })
}

fn event_path(path: PathBuf) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let suffix = name.strip_prefix("event")?;
    (!suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())).then_some(path)
}

fn current_axes(device: &RawDevice) -> std::io::Result<(Option<AbsInfo>, Option<AbsInfo>)> {
    let mut x = None;
    let mut y = None;
    for (code, info) in device.get_absinfo()? {
        match code {
            AbsoluteAxisCode::ABS_X => x = Some(info),
            AbsoluteAxisCode::ABS_Y => y = Some(info),
            _ => {}
        }
    }
    Ok((x, y))
}

fn valid_capabilities(has_touch: bool, x: AbsInfo, y: AbsInfo) -> bool {
    has_touch
        && x.minimum() == 0
        && x.maximum() == canvas::WIDTH as i32 - 1
        && y.minimum() == 0
        && y.maximum() == canvas::HEIGHT as i32 - 1
}

fn clamp_x(value: i32) -> i32 {
    value.clamp(0, canvas::WIDTH as i32 - 1)
}

fn clamp_y(value: i32) -> i32 {
    value.clamp(0, canvas::HEIGHT as i32 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::EventType;
    use std::path::Path;

    fn event(event_type: EventType, code: u16, value: i32) -> InputEvent {
        InputEvent::new(event_type.0, code, value)
    }

    #[test]
    fn reports_exact_goodix_press_motion_release() {
        let mut state = TouchState::new(0, 0, false);
        assert_eq!(
            state.handle(event(EventType::ABSOLUTE, AbsoluteAxisCode::ABS_X.0, 1400)),
            TouchAction::Ignore
        );
        state.handle(event(EventType::ABSOLUTE, AbsoluteAxisCode::ABS_Y.0, -20));
        state.handle(event(EventType::KEY, KeyCode::BTN_TOUCH.0, 1));
        assert_eq!(
            state.handle(event(
                EventType::SYNCHRONIZATION,
                SynchronizationCode::SYN_REPORT.0,
                0
            )),
            TouchAction::Report(TouchReport {
                x: 1279,
                y: 0,
                down: true,
                pressed: true,
                released: false,
            })
        );
        state.handle(event(EventType::ABSOLUTE, AbsoluteAxisCode::ABS_X.0, 42));
        assert_eq!(
            state.handle(event(
                EventType::SYNCHRONIZATION,
                SynchronizationCode::SYN_REPORT.0,
                0
            )),
            TouchAction::Report(TouchReport {
                x: 42,
                y: 0,
                down: true,
                pressed: false,
                released: false,
            })
        );
        state.handle(event(EventType::KEY, KeyCode::BTN_TOUCH.0, 0));
        assert_eq!(
            state.handle(event(
                EventType::SYNCHRONIZATION,
                SynchronizationCode::SYN_REPORT.0,
                0
            )),
            TouchAction::Report(TouchReport {
                x: 42,
                y: 0,
                down: false,
                pressed: false,
                released: true,
            })
        );
    }

    #[test]
    fn resynchronizes_only_after_dropped_report_boundary() {
        let mut state = TouchState::new(7, 9, false);
        assert_eq!(
            state.handle(event(
                EventType::SYNCHRONIZATION,
                SynchronizationCode::SYN_DROPPED.0,
                0
            )),
            TouchAction::Ignore
        );
        assert_eq!(
            state.handle(event(EventType::ABSOLUTE, AbsoluteAxisCode::ABS_X.0, 900)),
            TouchAction::Ignore
        );
        assert_eq!(
            state.handle(event(
                EventType::SYNCHRONIZATION,
                SynchronizationCode::SYN_REPORT.0,
                0
            )),
            TouchAction::Resynchronize
        );
        assert_eq!(
            state.resynchronize(1300, 480, true),
            TouchReport {
                x: 1279,
                y: 479,
                down: true,
                pressed: true,
                released: false,
            }
        );
    }

    #[test]
    fn validates_exact_dimensions_and_event_paths() {
        let x = AbsInfo::new(17, 0, 1279, 0, 0, 0);
        let y = AbsInfo::new(19, 0, 479, 0, 0, 0);
        assert!(valid_capabilities(true, x, y));
        assert!(!valid_capabilities(false, x, y));
        assert!(!valid_capabilities(
            true,
            AbsInfo::new(0, 0, 1280, 0, 0, 0),
            y
        ));
        assert_eq!(
            event_path(Path::new("/dev/input/event12").to_path_buf()),
            Some(Path::new("/dev/input/event12").to_path_buf())
        );
        assert_eq!(
            event_path(Path::new("/dev/input/eventx").to_path_buf()),
            None
        );
    }
}
