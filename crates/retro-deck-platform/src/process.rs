//! Bounded Unix child-process ownership for interactive Deck applications.

use std::error::Error;
use std::fmt;
use std::io;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};

const TERMINATION_GRACE: Duration = Duration::from_secs(4);

/// How a managed child reached its reported exit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedExitCause {
    /// The program exited before the supervisor requested termination.
    Natural,
    /// The supervisor sent `SIGTERM` but did not need to escalate.
    TermRequested,
    /// The child outlived its grace period and the supervisor sent `SIGKILL`.
    KillRequired,
}

/// Reaped child status plus the supervisor action that preceded it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedChildExit {
    status: ExitStatus,
    cause: ManagedExitCause,
}

impl ManagedChildExit {
    /// Native process exit status.
    #[must_use]
    pub const fn status(self) -> ExitStatus {
        self.status
    }

    /// Whether and how the supervisor requested termination.
    #[must_use]
    pub const fn cause(self) -> ManagedExitCause {
        self.cause
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildPhase {
    Running,
    TermSent { at: Instant },
    KillSent,
}

impl ChildPhase {
    const fn exit_cause(self) -> ManagedExitCause {
        match self {
            Self::Running => ManagedExitCause::Natural,
            Self::TermSent { .. } => ManagedExitCause::TermRequested,
            Self::KillSent => ManagedExitCause::KillRequired,
        }
    }
}

/// One process-group leader owned until it is reaped.
#[derive(Debug)]
pub struct ManagedChild {
    child: Option<Child>,
    process_group: Pid,
    program: PathBuf,
    phase: ChildPhase,
}

impl ManagedChild {
    /// Spawn an absolute command as the leader of a fresh process group.
    ///
    /// Arguments, environment, and standard streams already configured on
    /// `command` are preserved. The process-group setup runs between fork and
    /// exec through the standard library's Unix command adapter.
    ///
    /// # Errors
    ///
    /// Returns [`ManagedChildError::RelativeProgram`] for a non-absolute
    /// executable or [`ManagedChildError::Spawn`] when setup or exec fails.
    pub fn spawn(command: &mut Command) -> Result<Self, ManagedChildError> {
        let program = PathBuf::from(command.get_program());
        if !program.is_absolute() {
            return Err(ManagedChildError::RelativeProgram(program));
        }
        command.process_group(0);
        let child = command.spawn().map_err(|source| ManagedChildError::Spawn {
            program: program.clone(),
            source,
        })?;
        let process_group = Pid::from_child(&child);
        Ok(Self {
            child: Some(child),
            process_group,
            program,
            phase: ChildPhase::Running,
        })
    }

    /// Absolute executable originally passed to [`Self::spawn`].
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "PathBuf-to-Path dereferencing is not const on Rust 1.86"
    )]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Take the child's piped standard input exactly once.
    ///
    /// Returns `None` when the command did not request
    /// [`std::process::Stdio::piped`], the pipe was already taken, or the child
    /// has already been reaped.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }

    /// Ask the complete child process group to stop gracefully.
    ///
    /// Returns `true` only for the first request. Repeated requests are
    /// idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`ManagedChildError::Signal`] when the group exists but cannot
    /// be signalled.
    pub fn request_termination(&mut self, now: Instant) -> Result<bool, ManagedChildError> {
        if self.child.is_none() || self.phase != ChildPhase::Running {
            return Ok(false);
        }
        self.signal(Signal::TERM, "send SIGTERM")?;
        self.phase = ChildPhase::TermSent { at: now };
        Ok(true)
    }

    /// Reap a finished child or escalate an expired termination request.
    ///
    /// This method never waits for child completion.
    ///
    /// # Errors
    ///
    /// Returns [`ManagedChildError::Wait`] when nonblocking reaping fails or
    /// [`ManagedChildError::Signal`] when escalation cannot be sent.
    pub fn poll(&mut self, now: Instant) -> Result<Option<ManagedChildExit>, ManagedChildError> {
        if let Some(exit) = self.try_reap()? {
            return Ok(Some(exit));
        }
        if matches!(
            self.phase,
            ChildPhase::TermSent { at } if now.saturating_duration_since(at) >= TERMINATION_GRACE
        ) {
            self.signal(Signal::KILL, "send SIGKILL")?;
            self.phase = ChildPhase::KillSent;
        }
        self.try_reap()
    }

    /// Whether a TERM or KILL request is already active.
    #[must_use]
    pub const fn terminating(&self) -> bool {
        !matches!(self.phase, ChildPhase::Running)
    }

    fn try_reap(&mut self) -> Result<Option<ManagedChildExit>, ManagedChildError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let status = child.try_wait().map_err(|source| ManagedChildError::Wait {
            program: self.program.clone(),
            source,
        })?;
        let Some(status) = status else {
            return Ok(None);
        };
        self.child = None;
        Ok(Some(ManagedChildExit {
            status,
            cause: self.phase.exit_cause(),
        }))
    }

    fn signal(&self, signal: Signal, operation: &'static str) -> Result<(), ManagedChildError> {
        match kill_process_group(self.process_group, signal) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(source) => Err(ManagedChildError::Signal {
                program: self.program.clone(),
                operation,
                source,
            }),
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ignored = kill_process_group(self.process_group, Signal::KILL);
        let _ignored = child.wait();
    }
}

/// Managed process setup, signalling, or reaping failure.
#[derive(Debug)]
pub enum ManagedChildError {
    /// Only absolute, reviewed program identities may be launched.
    RelativeProgram(PathBuf),
    /// Process-group setup or exec failed.
    Spawn {
        /// Requested absolute executable.
        program: PathBuf,
        /// Operating-system failure.
        source: io::Error,
    },
    /// A child could not be checked and reaped.
    Wait {
        /// Requested absolute executable.
        program: PathBuf,
        /// Operating-system failure.
        source: io::Error,
    },
    /// A live process group could not be signalled.
    Signal {
        /// Requested absolute executable.
        program: PathBuf,
        /// Signal operation.
        operation: &'static str,
        /// Operating-system failure.
        source: rustix::io::Errno,
    },
}

impl fmt::Display for ManagedChildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativeProgram(program) => {
                write!(
                    formatter,
                    "managed program is not absolute: {}",
                    program.display()
                )
            }
            Self::Spawn { program, source } => {
                write!(formatter, "cannot start {}: {source}", program.display())
            }
            Self::Wait { program, source } => {
                write!(formatter, "cannot reap {}: {source}", program.display())
            }
            Self::Signal {
                program,
                operation,
                source,
            } => write!(
                formatter,
                "cannot {operation} for {}: {source}",
                program.display()
            ),
        }
    }
}

impl Error for ManagedChildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::Wait { source, .. } => Some(source),
            Self::Signal { source, .. } => Some(source),
            Self::RelativeProgram(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{ManagedChild, ManagedChildError, ManagedExitCause, TERMINATION_GRACE};

    #[test]
    fn relative_and_missing_programs_fail_before_supervision() {
        let mut relative = Command::new("true");
        assert!(matches!(
            ManagedChild::spawn(&mut relative),
            Err(ManagedChildError::RelativeProgram(_))
        ));

        let mut missing = Command::new("/definitely/missing/retro-deck-test");
        assert!(matches!(
            ManagedChild::spawn(&mut missing),
            Err(ManagedChildError::Spawn { .. })
        ));
    }

    #[test]
    fn short_child_is_reaped_without_a_termination_request() {
        let mut command = Command::new("/bin/true");
        let Some(mut child) = ManagedChild::spawn(&mut command).ok() else {
            return;
        };
        let Some(exit) = wait_for_exit(&mut child, Instant::now()) else {
            return;
        };
        assert!(exit.status().success());
        assert_eq!(exit.cause(), ManagedExitCause::Natural);
    }

    #[test]
    fn piped_stdin_is_taken_once_without_exposing_child_ownership() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("IFS= read -r value && test \"$value\" = bounded")
            .stdin(Stdio::piped());
        let Some(mut child) = ManagedChild::spawn(&mut command).ok() else {
            return;
        };
        let Some(mut input) = child.take_stdin() else {
            return;
        };
        assert!(child.take_stdin().is_none());
        assert!(input.write_all(b"bounded\n").is_ok());
        drop(input);
        let Some(exit) = wait_for_exit(&mut child, Instant::now()) else {
            return;
        };
        assert!(exit.status().success());
        assert_eq!(exit.cause(), ManagedExitCause::Natural);
    }

    #[test]
    fn termination_is_idempotent_and_escalates_after_its_deadline() {
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        let Some(mut child) = ManagedChild::spawn(&mut command).ok() else {
            return;
        };
        let requested_at = Instant::now();
        assert!(matches!(child.request_termination(requested_at), Ok(true)));
        assert!(matches!(child.request_termination(requested_at), Ok(false)));
        assert!(child.terminating());
        let Some(exit) = wait_for_exit(&mut child, requested_at + TERMINATION_GRACE) else {
            return;
        };
        assert!(matches!(
            exit.cause(),
            ManagedExitCause::TermRequested | ManagedExitCause::KillRequired
        ));
        assert!(!exit.status().success());
    }

    fn wait_for_exit(
        child: &mut ManagedChild,
        logical_now: Instant,
    ) -> Option<super::ManagedChildExit> {
        for _ in 0..100 {
            if let Some(exit) = child.poll(logical_now).ok().flatten() {
                return Some(exit);
            }
            thread::sleep(Duration::from_millis(2));
        }
        None
    }
}
