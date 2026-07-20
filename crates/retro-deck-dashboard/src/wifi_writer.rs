//! Bounded asynchronous handoff to the installed Wi-Fi profile writer.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use retro_deck_platform::process::{ManagedChild, ManagedChildError};

use crate::{
    MAXIMUM_PASSPHRASE_BYTES, MAXIMUM_SSID_BYTES, MINIMUM_PASSPHRASE_BYTES, WifiCredentials,
};

const REQUEST_CAPACITY: usize = 1;
const RESULT_CAPACITY: usize = 4;
const WORKER_NAME: &str = "retro-deck-wifi-writer";
const CHILD_POLL: Duration = Duration::from_millis(20);
const HELPER_DEADLINE: Duration = Duration::from_secs(5);
const CHILD_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

/// Nonblocking outcome of submitting one explicit profile save.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiWriterSubmit {
    /// The validated request entered the bounded worker queue.
    Queued(WifiWriterRequestId),
    /// One request is already waiting for the writer.
    Busy,
    /// Credentials violated the writer's defensive schema check.
    Invalid,
    /// The writer thread is no longer available.
    Disconnected,
    /// The process-lifetime request identifier space was exhausted.
    Exhausted,
}

/// Process-lifetime identity of one accepted profile save.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WifiWriterRequestId(u64);

impl WifiWriterRequestId {
    /// Return the monotonically increasing identifier value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn from_test_serial(serial: u64) -> Self {
        Self(serial)
    }
}

/// Nonblocking result-channel state.
#[derive(Debug)]
pub enum WifiWriterPoll {
    /// One completed writer operation.
    Result(WifiWriterResult),
    /// No result is currently available.
    Empty,
    /// The writer thread has ended and no further result can arrive.
    Disconnected,
}

/// Result of one explicit profile save.
#[derive(Debug)]
pub enum WifiWriterResult {
    /// The installed helper reported success.
    Saved {
        /// Identity returned when this request was submitted.
        request: WifiWriterRequestId,
    },
    /// The request failed without exposing either credential.
    Failed {
        /// Identity returned when this request was submitted.
        request: WifiWriterRequestId,
        /// Redacted helper or supervision failure.
        error: WifiWriteError,
    },
}

/// Input-side handle for the isolated profile writer thread.
///
/// Submitting performs validation plus a bounded `try_send`. Process creation,
/// pipe I/O, waiting, and timeout containment never run on the input thread.
#[derive(Debug)]
pub struct WifiProfileWriter {
    requests: Option<SyncSender<WifiSaveRequest>>,
    results: Receiver<WifiWriterResult>,
    thread: Option<JoinHandle<WifiWriterReport>>,
    next_request: AtomicU64,
}

impl WifiProfileWriter {
    /// Start a writer bound to one reviewed absolute helper path.
    ///
    /// # Errors
    ///
    /// Returns [`WifiWriterStartError`] for an unsafe helper path or when the
    /// named worker thread cannot be created.
    pub fn spawn(helper: impl Into<PathBuf>) -> Result<Self, WifiWriterStartError> {
        Self::spawn_with_timing(helper.into(), WriterTiming::PRODUCTION)
    }

    fn spawn_with_timing(
        helper: PathBuf,
        timing: WriterTiming,
    ) -> Result<Self, WifiWriterStartError> {
        validate_helper_path(&helper)?;
        let (requests, request_receiver) = mpsc::sync_channel(REQUEST_CAPACITY);
        let (result_sender, results) = mpsc::sync_channel(RESULT_CAPACITY);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || run_writer(&helper, timing, &request_receiver, &result_sender))
            .map_err(WifiWriterStartError::Thread)?;
        Ok(Self {
            requests: Some(requests),
            results,
            thread: Some(thread),
            next_request: AtomicU64::new(1),
        })
    }

    /// Queue one validated editor snapshot without waiting for the helper.
    #[must_use]
    pub fn try_save(&self, credentials: &WifiCredentials<'_>) -> WifiWriterSubmit {
        if !WifiSaveRequest::valid(credentials.ssid(), credentials.passphrase()) {
            return WifiWriterSubmit::Invalid;
        }
        let Some(requests) = &self.requests else {
            return WifiWriterSubmit::Disconnected;
        };
        let Ok(serial) =
            self.next_request
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    current.checked_add(1)
                })
        else {
            return WifiWriterSubmit::Exhausted;
        };
        let request_id = WifiWriterRequestId(serial);
        let Some(request) =
            WifiSaveRequest::new(request_id, credentials.ssid(), credentials.passphrase())
        else {
            return WifiWriterSubmit::Invalid;
        };
        match requests.try_send(request) {
            Ok(()) => WifiWriterSubmit::Queued(request_id),
            Err(TrySendError::Full(_)) => WifiWriterSubmit::Busy,
            Err(TrySendError::Disconnected(_)) => WifiWriterSubmit::Disconnected,
        }
    }

    /// Poll one completed operation without waiting.
    #[must_use]
    pub fn try_result(&self) -> WifiWriterPoll {
        match self.results.try_recv() {
            Ok(result) => WifiWriterPoll::Result(result),
            Err(TryRecvError::Empty) => WifiWriterPoll::Empty,
            Err(TryRecvError::Disconnected) => WifiWriterPoll::Disconnected,
        }
    }

    /// Finish queued work, stop the writer, and return final counts.
    #[must_use]
    pub fn shutdown(mut self) -> WifiWriterReport {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> WifiWriterReport {
        let _ = self.requests.take();
        let Some(thread) = self.thread.take() else {
            return WifiWriterReport::default();
        };
        match thread.join() {
            Ok(report) => report,
            Err(_) => WifiWriterReport {
                panicked: true,
                ..WifiWriterReport::default()
            },
        }
    }
}

impl Drop for WifiProfileWriter {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

/// Unsafe or unavailable writer setup.
#[derive(Debug)]
pub enum WifiWriterStartError {
    /// The helper path was relative or contained navigation components.
    UnsafePath(PathBuf),
    /// The named writer thread could not be created.
    Thread(io::Error),
}

impl fmt::Display for WifiWriterStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafePath(path) => {
                write!(formatter, "Wi-Fi writer path is unsafe: {}", path.display())
            }
            Self::Thread(error) => write!(formatter, "cannot start Wi-Fi writer thread: {error}"),
        }
    }
}

impl Error for WifiWriterStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Thread(error) => Some(error),
            Self::UnsafePath(_) => None,
        }
    }
}

/// Profile helper failure with all credential values omitted.
#[derive(Debug)]
pub enum WifiWriteError {
    /// The reviewed helper could not be executed.
    Spawn(ManagedChildError),
    /// A piped standard input was unexpectedly unavailable.
    MissingInput,
    /// The bounded request could not be written to the helper.
    Write(io::Error),
    /// The managed child could not be polled or contained.
    Supervision(ManagedChildError),
    /// The helper exceeded its fixed completion deadline.
    Timeout,
    /// The helper returned an unsuccessful status.
    Exit(ExitStatus),
}

impl fmt::Display for WifiWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "cannot start Wi-Fi profile helper: {error}"),
            Self::MissingInput => formatter.write_str("Wi-Fi profile helper has no input pipe"),
            Self::Write(error) => write!(formatter, "cannot write Wi-Fi profile request: {error}"),
            Self::Supervision(error) => {
                write!(formatter, "cannot supervise Wi-Fi profile helper: {error}")
            }
            Self::Timeout => formatter.write_str("Wi-Fi profile helper timed out"),
            Self::Exit(status) => write!(formatter, "Wi-Fi profile helper failed with {status}"),
        }
    }
}

impl Error for WifiWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spawn(error) | Self::Supervision(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::MissingInput | Self::Timeout | Self::Exit(_) => None,
        }
    }
}

/// Final profile-writer diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiWriterReport {
    /// Requests accepted by the worker.
    pub attempts: u64,
    /// Helpers that reported success.
    pub saved: u64,
    /// Helpers that failed or timed out.
    pub failed: u64,
    /// Results omitted because the diagnostic queue was full.
    pub dropped_results: u64,
    /// Whether the writer thread panicked.
    pub panicked: bool,
}

#[derive(Clone, Copy, Debug)]
struct WriterTiming {
    poll: Duration,
    deadline: Duration,
}

impl WriterTiming {
    const PRODUCTION: Self = Self {
        poll: CHILD_POLL,
        deadline: HELPER_DEADLINE,
    };
}

struct WifiSaveRequest {
    id: WifiWriterRequestId,
    ssid: BoundedAscii<MAXIMUM_SSID_BYTES>,
    passphrase: BoundedAscii<MAXIMUM_PASSPHRASE_BYTES>,
}

impl WifiSaveRequest {
    fn valid(ssid: &str, passphrase: &str) -> bool {
        BoundedAscii::<MAXIMUM_SSID_BYTES>::valid(ssid, 1)
            && BoundedAscii::<MAXIMUM_PASSPHRASE_BYTES>::valid(passphrase, MINIMUM_PASSPHRASE_BYTES)
    }

    fn new(id: WifiWriterRequestId, ssid: &str, passphrase: &str) -> Option<Self> {
        Some(Self {
            id,
            ssid: BoundedAscii::new(ssid, 1)?,
            passphrase: BoundedAscii::new(passphrase, MINIMUM_PASSPHRASE_BYTES)?,
        })
    }

    fn write_to(&self, output: &mut impl io::Write) -> io::Result<()> {
        output.write_all(self.ssid.as_bytes())?;
        output.write_all(b"\n")?;
        output.write_all(self.passphrase.as_bytes())?;
        output.write_all(b"\n")
    }
}

impl fmt::Debug for WifiSaveRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WifiSaveRequest")
            .field("id", &self.id)
            .field("ssid_bytes", &self.ssid.len())
            .field("passphrase_bytes", &self.passphrase.len())
            .finish()
    }
}

struct BoundedAscii<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    len: usize,
}

impl<const CAPACITY: usize> BoundedAscii<CAPACITY> {
    fn valid(value: &str, minimum: usize) -> bool {
        let bytes = value.as_bytes();
        bytes.len() >= minimum
            && bytes.len() <= CAPACITY
            && bytes.iter().all(|byte| (b' '..=b'~').contains(byte))
    }

    fn new(value: &str, minimum: usize) -> Option<Self> {
        let bytes = value.as_bytes();
        if !Self::valid(value, minimum) {
            return None;
        }
        let mut output = Self {
            bytes: [0; CAPACITY],
            len: bytes.len(),
        };
        output.bytes.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(output)
    }

    const fn len(&self) -> usize {
        self.len
    }

    fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or_default()
    }
}

impl<const CAPACITY: usize> Drop for BoundedAscii<CAPACITY> {
    fn drop(&mut self) {
        self.bytes.fill(0);
        self.len = 0;
    }
}

fn validate_helper_path(path: &Path) -> Result<(), WifiWriterStartError> {
    let mut saw_root = false;
    let mut saw_name = false;
    for component in path.components() {
        match component {
            Component::RootDir if !saw_root && !saw_name => saw_root = true,
            Component::Normal(_) if saw_root => saw_name = true,
            _ => return Err(WifiWriterStartError::UnsafePath(path.to_path_buf())),
        }
    }
    if saw_root && saw_name {
        Ok(())
    } else {
        Err(WifiWriterStartError::UnsafePath(path.to_path_buf()))
    }
}

fn run_writer(
    helper: &Path,
    timing: WriterTiming,
    requests: &Receiver<WifiSaveRequest>,
    results: &SyncSender<WifiWriterResult>,
) -> WifiWriterReport {
    let mut report = WifiWriterReport::default();
    while let Ok(request) = requests.recv() {
        report.attempts = report.attempts.saturating_add(1);
        let result = match write_profile(helper, timing, &request) {
            Ok(()) => {
                report.saved = report.saved.saturating_add(1);
                WifiWriterResult::Saved {
                    request: request.id,
                }
            }
            Err(error) => {
                report.failed = report.failed.saturating_add(1);
                WifiWriterResult::Failed {
                    request: request.id,
                    error,
                }
            }
        };
        match results.try_send(result) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                report.dropped_results = report.dropped_results.saturating_add(1);
            }
        }
    }
    report
}

fn write_profile(
    helper: &Path,
    timing: WriterTiming,
    request: &WifiSaveRequest,
) -> Result<(), WifiWriteError> {
    let mut command = Command::new(helper);
    command
        .current_dir("/")
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = ManagedChild::spawn(&mut command).map_err(WifiWriteError::Spawn)?;
    let Some(mut input) = child.take_stdin() else {
        return Err(WifiWriteError::MissingInput);
    };
    request
        .write_to(&mut input)
        .map_err(WifiWriteError::Write)?;
    drop(input);

    let started = Instant::now();
    let mut timed_out = false;
    loop {
        let now = Instant::now();
        match child.poll(now).map_err(WifiWriteError::Supervision)? {
            Some(_) if timed_out => return Err(WifiWriteError::Timeout),
            Some(exit) if exit.status().success() => return Ok(()),
            Some(exit) => return Err(WifiWriteError::Exit(exit.status())),
            None => {}
        }
        if !timed_out && now.saturating_duration_since(started) >= timing.deadline {
            child
                .request_termination(now)
                .map_err(WifiWriteError::Supervision)?;
            timed_out = true;
        }
        thread::sleep(timing.poll);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        WifiProfileWriter, WifiWriteError, WifiWriterPoll, WifiWriterRequestId, WifiWriterResult,
        WifiWriterStartError, WifiWriterSubmit, WriterTiming,
    };
    use crate::{WifiAction, WifiEditor, WifiField};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture {
        root: std::path::PathBuf,
        helper: std::path::PathBuf,
    }

    impl Fixture {
        fn new(body: &str) -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "retro-deck-wifi-writer-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("Wi-Fi writer fixture is created");
            let helper = root.join("helper");
            fs::write(&helper, body).expect("Wi-Fi writer helper is created");
            let mut permissions = fs::metadata(&helper)
                .expect("Wi-Fi writer helper metadata exists")
                .permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&helper, permissions).expect("Wi-Fi writer helper is executable");
            Self { root, helper }
        }

        fn output(&self) -> std::path::PathBuf {
            self.root.join("helper.input")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn helper_receives_exact_credentials_only_on_standard_input() {
        let fixture = Fixture::new(
            "#!/bin/sh\numask 077\nIFS= read -r ssid || exit 10\nIFS= read -r password || exit 11\nIFS= read -r extra && exit 12\nprintf '%s\\n%s\\n' \"$ssid\" \"$password\" > \"$0.input\"\n",
        );
        let writer =
            WifiProfileWriter::spawn(&fixture.helper).expect("Wi-Fi writer starts for the fixture");
        let mut editor = credentials_editor();
        let Some(credentials) = editor.credentials() else {
            return;
        };
        let submission = writer.try_save(&credentials);
        assert_eq!(submission, WifiWriterSubmit::Queued(WifiWriterRequestId(1)));
        assert!(matches!(
            wait_result(&writer),
            Some(WifiWriterResult::Saved {
                request: WifiWriterRequestId(1)
            })
        ));
        let report = writer.shutdown();
        assert_eq!(report.attempts, 1);
        assert_eq!(report.saved, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(
            fs::read(fixture.output()).ok().as_deref(),
            Some(b"test net\nsecret!9\n".as_slice())
        );

        let diagnostics = format!("{editor:?}");
        assert!(!diagnostics.contains("secret!9"));
        let _ = editor.resolve_save(true);
    }

    #[test]
    fn failed_and_timed_out_helpers_are_reported_without_credentials() {
        let failed = Fixture::new(
            "#!/bin/sh\nIFS= read -r ssid || exit 10\nIFS= read -r password || exit 11\nexit 9\n",
        );
        let writer =
            WifiProfileWriter::spawn(&failed.helper).expect("failing Wi-Fi writer fixture starts");
        submit_fixture_request(&writer);
        let result = wait_result(&writer);
        assert!(matches!(
            result,
            Some(WifiWriterResult::Failed {
                request: WifiWriterRequestId(1),
                error: WifiWriteError::Exit(_),
            })
        ));
        let diagnostics = format!("{result:?}");
        assert!(!diagnostics.contains("secret!9"));
        assert_eq!(writer.shutdown().failed, 1);

        let hanging = Fixture::new(
            "#!/bin/sh\nIFS= read -r ssid || exit 10\nIFS= read -r password || exit 11\nsleep 30\n",
        );
        let writer = WifiProfileWriter::spawn_with_timing(
            hanging.helper.clone(),
            WriterTiming {
                poll: std::time::Duration::from_millis(2),
                deadline: std::time::Duration::from_millis(20),
            },
        )
        .expect("hanging Wi-Fi writer fixture starts");
        submit_fixture_request(&writer);
        let result = wait_result(&writer);
        assert!(matches!(
            result,
            Some(WifiWriterResult::Failed {
                request: WifiWriterRequestId(1),
                error: WifiWriteError::Timeout,
            })
        ));
        assert_eq!(writer.shutdown().failed, 1);
    }

    #[test]
    fn helper_identity_is_absolute_and_navigation_free() {
        assert!(matches!(
            WifiProfileWriter::spawn("relative/helper"),
            Err(WifiWriterStartError::UnsafePath(_))
        ));
        assert!(matches!(
            WifiProfileWriter::spawn("/usr/../tmp/helper"),
            Err(WifiWriterStartError::UnsafePath(_))
        ));
    }

    fn credentials_editor() -> WifiEditor {
        let mut editor = WifiEditor::new();
        for byte in b"test net" {
            let _ = editor.apply(WifiAction::TypeAscii(*byte));
        }
        let _ = editor.apply(WifiAction::SelectField(WifiField::Passphrase));
        for byte in b"secret!9" {
            let _ = editor.apply(WifiAction::TypeAscii(*byte));
        }
        editor
    }

    fn submit_fixture_request(writer: &WifiProfileWriter) {
        let editor = credentials_editor();
        let Some(credentials) = editor.credentials() else {
            return;
        };
        assert!(matches!(
            writer.try_save(&credentials),
            WifiWriterSubmit::Queued(_)
        ));
    }

    fn wait_result(writer: &WifiProfileWriter) -> Option<WifiWriterResult> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match writer.try_result() {
                WifiWriterPoll::Result(result) => return Some(result),
                WifiWriterPoll::Empty if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                WifiWriterPoll::Empty | WifiWriterPoll::Disconnected => return None,
            }
        }
    }
}
