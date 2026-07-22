use crate::{fbdev, input, wayland};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(40);
const TOUCH_HOLD: Duration = Duration::from_secs(2);
const TERM_GRACE: Duration = Duration::from_secs(4);
const TOUCH_RECONNECT: Duration = Duration::from_secs(1);
const CONSOLE_FRAME_INTERVAL: Duration = Duration::from_millis(100);
const KDGKBMODE: libc::c_ulong = 0x4b44;
const KDSKBMODE: libc::c_ulong = 0x4b45;
const DISPLAY_STATE: &[u8] = b"\x1b[?25l\x1b[13]\x1b[9;0]";
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn request_shutdown(_: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn install_signal_handlers() -> Result<(), String> {
    SHUTDOWN_REQUESTED.store(false, Ordering::Relaxed);
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = request_shutdown as *const () as usize;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
            return Err(format!(
                "cannot install signal handler: {}",
                io::Error::last_os_error()
            ));
        }
    }
    if unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) } == libc::SIG_ERR {
        return Err(format!(
            "cannot ignore SIGPIPE: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct ChildResult {
    pub started: bool,
    pub exited_for_touch: bool,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub error: Option<String>,
}

struct TtySnapshot {
    file: Option<File>,
    termios: Option<libc::termios>,
    keyboard_mode: Option<libc::c_int>,
}

impl TtySnapshot {
    fn capture() -> Self {
        let Ok(file) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("/dev/tty0")
        else {
            return Self {
                file: None,
                termios: None,
                keyboard_mode: None,
            };
        };
        let fd = file.as_raw_fd();
        let mut termios = MaybeUninit::<libc::termios>::uninit();
        let termios = (unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } == 0)
            .then(|| unsafe { termios.assume_init() });
        let mut keyboard_mode = 0;
        let keyboard_mode = (unsafe { libc::ioctl(fd, KDGKBMODE, &mut keyboard_mode) } == 0)
            .then_some(keyboard_mode);
        Self {
            file: Some(file),
            termios,
            keyboard_mode,
        }
    }

    fn restore(&self) {
        if let Some(file) = &self.file {
            let fd = file.as_raw_fd();
            if let Some(mode) = self.keyboard_mode {
                unsafe {
                    libc::ioctl(fd, KDSKBMODE, mode);
                }
            }
            if let Some(termios) = &self.termios {
                unsafe {
                    libc::tcsetattr(fd, libc::TCSAFLUSH, termios);
                }
            }
        }
        if let Ok(mut console) = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open("/dev/tty0")
        {
            let _ = console.write_all(DISPLAY_STATE);
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum StopRequest {
    None,
    Touch,
    Shutdown,
}

struct TouchHold {
    active_since: Option<Instant>,
}

impl TouchHold {
    fn new() -> Self {
        Self { active_since: None }
    }

    fn update(&mut self, down: bool, x: i32, y: i32) {
        let inside = down
            && x >= 0
            && x < crate::canvas::WIDTH as i32
            && y >= 0
            && y < crate::canvas::HEIGHT as i32;
        if !inside {
            if self.active_since.take().is_some() {
                eprintln!("retrodeck: return hold cancelled at {x},{y}");
            }
        } else if self.active_since.is_none() {
            self.active_since = Some(Instant::now());
            eprintln!("retrodeck: return hold started at {x},{y}");
        }
    }

    fn reset(&mut self) {
        self.active_since = None;
    }

    fn complete(&self, now: Instant) -> bool {
        self.active_since
            .is_some_and(|started| now.duration_since(started) >= TOUCH_HOLD)
    }
}

struct TerminalInteraction {
    uses_wayland: bool,
    mirror_console: bool,
    next_console_frame: Instant,
    last_touch_attempt: Option<Instant>,
    last_touch_error: String,
    hold: TouchHold,
}

impl TerminalInteraction {
    fn new() -> Self {
        let uses_wayland = wayland::size().is_some();
        if !uses_wayland {
            fbdev::close();
        }
        Self {
            uses_wayland,
            mirror_console: uses_wayland,
            next_console_frame: Instant::now(),
            last_touch_attempt: None,
            last_touch_error: String::new(),
            hold: TouchHold::new(),
        }
    }

    fn step(&mut self, timeout: Duration) -> StopRequest {
        if self.uses_wayland {
            if let Err(error) = wayland::dispatch(duration_ms(timeout)) {
                eprintln!("retrodeck: {error}");
                self.hold.reset();
            }
            while let Some(report) = wayland::next_touch() {
                self.hold.update(report.down, report.x, report.y);
            }
        } else {
            self.poll_evdev(timeout);
        }

        let now = Instant::now();
        if self.mirror_console && now >= self.next_console_frame {
            let result =
                fbdev::read_console_scanout().and_then(|frame| wayland::present_rgb565(&frame));
            if let Err(error) = result {
                eprintln!("retrodeck: terminal display unavailable: {error}");
                self.mirror_console = false;
            }
            self.next_console_frame = now + CONSOLE_FRAME_INTERVAL;
        }
        if shutdown_requested() || (self.uses_wayland && wayland::shutdown_requested()) {
            StopRequest::Shutdown
        } else if self.hold.complete(now) {
            StopRequest::Touch
        } else {
            StopRequest::None
        }
    }

    fn poll_evdev(&mut self, timeout: Duration) {
        let now = Instant::now();
        let reconnect_due = self
            .last_touch_attempt
            .is_none_or(|last| now.duration_since(last) >= TOUCH_RECONNECT);
        if !input::touch_open() && reconnect_due {
            self.last_touch_attempt = Some(now);
            if let Err(error) = input::open_touch()
                && error != self.last_touch_error
            {
                eprintln!("retrodeck: {error}");
                self.last_touch_error = error;
            }
        }
        if input::touch_open() {
            if let Err(error) = input::dispatch_touch(duration_ms(timeout)) {
                eprintln!("retrodeck: {error}");
                input::close_touch();
                self.hold.reset();
            }
            let mut received_report = false;
            while let Some(report) = input::next_touch() {
                received_report = true;
                self.hold.update(report.down, report.x, report.y);
            }
            if !received_report && let Some(report) = input::current_touch() {
                self.hold.update(report.down, report.x, report.y);
            }
        } else {
            thread::sleep(timeout);
        }
    }
}

pub fn run_terminal(executable: &Path, keymap: &OsStr, mode: &OsStr, label: &str) -> ChildResult {
    let mut interaction = TerminalInteraction::new();
    let tty = TtySnapshot::capture();
    eprintln!("retrodeck: launching {label}");
    let arguments = [OsString::from(mode)];
    let environment = [(OsString::from("RETRO_DECK_KEYMAP"), OsString::from(keymap))];
    let result = spawn_and_supervise(
        executable,
        &arguments,
        &environment,
        label,
        |timeout| interaction.step(timeout),
        POLL_INTERVAL,
        TERM_GRACE,
    );
    tty.restore();
    result
}

fn spawn_and_supervise<F>(
    executable: &Path,
    arguments: &[OsString],
    environment: &[(OsString, OsString)],
    label: &str,
    mut step: F,
    poll_interval: Duration,
    term_grace: Duration,
) -> ChildResult
where
    F: FnMut(Duration) -> StopRequest,
{
    let mut command = Command::new(executable);
    command.args(arguments).envs(environment.iter().cloned());
    // signal(2) and setpgid(2) are async-signal-safe; the hook only reports errno.
    unsafe {
        command.pre_exec(reset_child_process);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ChildResult {
                error: Some(format!("cannot start {label}: {error}")),
                ..ChildResult::default()
            };
        }
    };
    let mut result = ChildResult {
        started: true,
        ..ChildResult::default()
    };
    let Ok(pid) = libc::pid_t::try_from(child.id()) else {
        result.error = Some("child process id is out of range".to_owned());
        let _ = child.kill();
        let _ = child.wait();
        return result;
    };
    if unsafe { libc::setpgid(pid, pid) } != 0 {
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::EACCES | libc::ESRCH)) {
            eprintln!("retrodeck: warning: cannot establish child process group: {error}");
        }
    }

    let mut term_sent_at = None;
    let mut kill_sent = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                set_status(&mut result, status);
                break;
            }
            Ok(None) => {}
            Err(error) => {
                result.error = Some(format!("waitpid failed: {error}"));
                signal_child_group(pid, libc::SIGKILL);
                if let Ok(status) = child.wait() {
                    set_status(&mut result, status);
                }
                break;
            }
        }

        let request = step(poll_interval);
        if term_sent_at.is_none() && request != StopRequest::None {
            eprintln!("retrodeck: stopping {label}");
            signal_child_group(pid, libc::SIGTERM);
            term_sent_at = Some(Instant::now());
            result.exited_for_touch = request == StopRequest::Touch;
        }
        if !kill_sent && term_sent_at.is_some_and(|sent| sent.elapsed() >= term_grace) {
            signal_child_group(pid, libc::SIGKILL);
            kill_sent = true;
        }
    }

    if let Some(code) = result.exit_code {
        eprintln!("retrodeck: {label} exited with status {code}");
    } else if let Some(signal) = result.signal {
        eprintln!("retrodeck: {label} stopped by signal {signal}");
    }
    result
}

fn reset_child_process() -> io::Result<()> {
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGPIPE] {
        if unsafe { libc::signal(signal, libc::SIG_DFL) } == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
    }
    if unsafe { libc::setpgid(0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn signal_child_group(pid: libc::pid_t, signal: libc::c_int) {
    if unsafe { libc::kill(-pid, signal) } != 0
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    {
        unsafe {
            libc::kill(pid, signal);
        }
    }
}

fn set_status(result: &mut ChildResult, status: ExitStatus) {
    result.exit_code = status.code();
    result.signal = status.signal();
}

fn duration_ms(duration: Duration) -> u32 {
    duration.as_millis().clamp(1, u128::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn managed_child_fixture() {
        let Ok(action) = env::var("RETRODECK_PROCESS_FIXTURE") else {
            return;
        };
        assert_eq!(env::var("RETRO_DECK_KEYMAP").unwrap(), "cz");
        match action.as_str() {
            "clean" => {}
            "exit-7" => std::process::exit(7),
            "signal" => unsafe {
                libc::raise(libc::SIGUSR1);
            },
            "group" => unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
                let grandchild = libc::fork();
                if grandchild == 0 {
                    loop {
                        libc::pause();
                    }
                }
                assert!(grandchild > 0);
                std::fs::write(
                    env::var_os("RETRODECK_PROCESS_PIDS").unwrap(),
                    format!("{} {grandchild}\n", std::process::id()),
                )
                .unwrap();
                loop {
                    libc::pause();
                }
            },
            other => panic!("unknown fixture {other}"),
        }
    }

    fn fixture_command(
        action: &str,
        extra: &[(OsString, OsString)],
    ) -> (PathBuf, Vec<OsString>, Vec<(OsString, OsString)>) {
        let executable = env::current_exe().unwrap();
        let arguments = [
            "--exact",
            "process::tests::managed_child_fixture",
            "--test-threads=1",
        ]
        .map(OsString::from)
        .to_vec();
        let mut environment = vec![
            (
                OsString::from("RETRODECK_PROCESS_FIXTURE"),
                OsString::from(action),
            ),
            (OsString::from("RETRO_DECK_KEYMAP"), OsString::from("cz")),
        ];
        environment.extend_from_slice(extra);
        (executable, arguments, environment)
    }

    fn run_fixture(action: &str) -> ChildResult {
        let (executable, arguments, environment) = fixture_command(action, &[]);
        spawn_and_supervise(
            &executable,
            &arguments,
            &environment,
            "fixture",
            |timeout| {
                thread::sleep(timeout);
                StopRequest::None
            },
            Duration::from_millis(5),
            Duration::from_millis(50),
        )
    }

    #[test]
    fn classifies_clean_nonzero_signal_and_exec_failure() {
        assert_eq!(run_fixture("clean").exit_code, Some(0));
        assert_eq!(run_fixture("exit-7").exit_code, Some(7));
        assert_eq!(run_fixture("signal").signal, Some(libc::SIGUSR1));
        let result = spawn_and_supervise(
            Path::new("/no/such/retrodeck-terminal"),
            &[],
            &[],
            "terminal",
            |_| StopRequest::None,
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert!(!result.started);
        assert!(result.error.unwrap().starts_with("cannot start terminal:"));
    }

    #[test]
    fn terminates_the_complete_child_process_group_after_touch() {
        let path = env::temp_dir().join(format!(
            "retrodeck-process-pids-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let extra = [(
            OsString::from("RETRODECK_PROCESS_PIDS"),
            path.clone().into(),
        )];
        let (executable, arguments, environment) = fixture_command("group", &extra);
        let started = Instant::now();
        let result = spawn_and_supervise(
            &executable,
            &arguments,
            &environment,
            "fixture",
            |timeout| {
                if path.exists() {
                    StopRequest::Touch
                } else {
                    thread::sleep(timeout);
                    assert!(started.elapsed() < Duration::from_secs(2));
                    StopRequest::None
                }
            },
            Duration::from_millis(5),
            Duration::from_millis(50),
        );
        assert!(result.started);
        assert!(result.exited_for_touch);
        assert_eq!(result.signal, Some(libc::SIGKILL));
        let pids = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        for pid in pids.split_whitespace().map(|pid| pid.parse().unwrap()) {
            let deadline = Instant::now() + Duration::from_secs(1);
            while process_alive(pid) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(!process_alive(pid));
        }
    }

    fn process_alive(pid: libc::pid_t) -> bool {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        status
            .rsplit_once(") ")
            .and_then(|(_, fields)| fields.chars().next())
            != Some('Z')
    }

    #[test]
    fn requires_an_uninterrupted_two_second_touch_hold() {
        let mut hold = TouchHold::new();
        hold.update(true, 12, 34);
        let started = hold.active_since.unwrap();
        assert!(!hold.complete(started + TOUCH_HOLD - Duration::from_millis(1)));
        hold.update(false, 12, 34);
        assert!(!hold.complete(started + TOUCH_HOLD));
        hold.update(true, 1279, 479);
        let restarted = hold.active_since.unwrap();
        assert!(hold.complete(restarted + TOUCH_HOLD));
    }
}
