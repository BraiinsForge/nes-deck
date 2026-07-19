//! Serialized, deadline-bounded dashboard process control.

use std::{
    fmt, io,
    os::unix::process::CommandExt as _,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use crate::application::DashboardRestarter;

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAXIMUM_TIMEOUT: Duration = Duration::from_secs(60);

/// Fixed `restart` command guarded against overlap and hangs.
pub struct CommandRestarter {
    program: PathBuf,
    timeout: Duration,
    gate: Mutex<()>,
}

impl CommandRestarter {
    /// Configure an absolute, traversal-free executable and finite deadline.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] for an unsafe path, zero
    /// timeout, or timeout longer than one minute.
    pub fn new(program: impl Into<PathBuf>, timeout: Duration) -> io::Result<Self> {
        let program = program.into();
        if !safe_absolute(&program) || timeout.is_zero() || timeout > MAXIMUM_TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "dashboard restart configuration is invalid",
            ));
        }
        Ok(Self {
            program,
            timeout,
            gate: Mutex::new(()),
        })
    }
}

impl DashboardRestarter for CommandRestarter {
    fn restart(&self) -> io::Result<()> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| io::Error::other("dashboard restart lock was poisoned"))?;
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or_else(|| io::Error::other("dashboard restart deadline overflow"))?;
        let mut command = Command::new(&self.program);
        command
            .arg("restart")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn()?;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(io::Error::other(format!(
                        "dashboard restart exited with {status}"
                    )));
                }
                Ok(None) => {}
                Err(error) => {
                    terminate(&mut child);
                    return Err(error);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                terminate(&mut child);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "dashboard restart timed out",
                ));
            }
            thread::sleep(deadline.saturating_duration_since(now).min(POLL_INTERVAL));
        }
    }
}

impl fmt::Debug for CommandRestarter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandRestarter")
            .field("program", &self.program)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

fn terminate(child: &mut std::process::Child) {
    let group = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw);
    if let Some(group) = group {
        let _group_result =
            rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    } else {
        let _kill_result = child.kill();
    }
    let _wait_result = child.wait();
}

fn safe_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _};

    use super::*;

    #[test]
    fn accepts_success_and_reports_failure() {
        let success = CommandRestarter::new("/bin/true", Duration::from_secs(1));
        assert!(matches!(success, Ok(restarter) if restarter.restart().is_ok()));
        let failure = CommandRestarter::new("/bin/false", Duration::from_secs(1));
        assert!(matches!(failure, Ok(restarter) if restarter.restart().is_err()));
    }

    #[test]
    fn kills_a_restart_that_exceeds_its_deadline() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let script = directory.path().join("slow-restart");
        assert!(fs::write(&script, b"#!/bin/sh\nsleep 2\n").is_ok());
        assert!(fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).is_ok());
        let restarter = CommandRestarter::new(&script, Duration::from_millis(20));
        assert!(matches!(
            restarter.and_then(|restarter| restarter.restart()),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
    }

    #[test]
    fn rejects_unsafe_or_unbounded_configuration() {
        for (path, timeout) in [
            ("relative", Duration::from_secs(1)),
            ("/tmp/../bin/true", Duration::from_secs(1)),
            ("/bin/true", Duration::ZERO),
            ("/bin/true", Duration::from_secs(61)),
        ] {
            assert!(CommandRestarter::new(path, timeout).is_err());
        }
    }
}
