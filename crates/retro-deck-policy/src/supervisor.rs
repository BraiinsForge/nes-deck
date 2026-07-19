//! Nonblocking client and process supervisor for the Common Lisp worker.

use std::{
    ffi::OsString,
    fmt,
    io::{self, BufReader, Read as _, Write as _},
    num::NonZeroUsize,
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU8, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    DEFAULT_MAX_BYTES, MessageError, PolicyRequest, PolicyResponse, RequestId, Value, decode_ready,
};

const FIRST_REQUEST_ID: i64 = 1;
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(10);

const STATUS_STARTING: u8 = 0;
const STATUS_READY: u8 = 1;
const STATUS_UNAVAILABLE: u8 = 2;
const STATUS_STOPPED: u8 = 3;

/// Process invocation for the trusted Common Lisp worker.
///
/// The environment is cleared by default. Production should use absolute
/// paths and add only the ECL runtime and local policy variables it needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerCommand {
    program: PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    current_directory: Option<PathBuf>,
    inherit_environment: bool,
}

impl WorkerCommand {
    /// Construct a worker command with an empty environment and no arguments.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            environment: Vec::new(),
            current_directory: None,
            inherit_environment: false,
        }
    }

    /// Append one process argument.
    #[must_use]
    pub fn arg(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    /// Add or replace one environment variable in the child.
    #[must_use]
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let name = name.into();
        self.environment.retain(|(existing, _)| existing != &name);
        self.environment.push((name, value.into()));
        self
    }

    /// Set the child's working directory.
    #[must_use]
    pub fn current_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.current_directory = Some(directory.into());
        self
    }

    /// Preserve the parent's environment before applying explicit entries.
    ///
    /// This is intended for development tools. The appliance worker should
    /// keep the default empty environment.
    #[must_use]
    pub const fn inherit_environment(mut self, inherit: bool) -> Self {
        self.inherit_environment = inherit;
        self
    }

    fn build(&self) -> Command {
        let mut command = Command::new(&self.program);
        if !self.inherit_environment {
            command.env_clear();
        }
        command
            .args(&self.arguments)
            .envs(self.environment.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(directory) = &self.current_directory {
            command.current_dir(directory);
        }
        command
    }
}

/// Resource and deadline policy for one worker process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerConfig {
    command: WorkerCommand,
    startup_timeout: Duration,
    request_timeout: Duration,
    request_capacity: NonZeroUsize,
    event_capacity: NonZeroUsize,
}

impl WorkerConfig {
    /// Construct production-oriented defaults for `command`.
    #[must_use]
    pub fn new(command: WorkerCommand) -> Self {
        Self {
            command,
            startup_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_millis(250),
            request_capacity: nonzero(8),
            event_capacity: nonzero(16),
        }
    }

    /// Replace the startup readiness deadline.
    #[must_use]
    pub const fn startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    /// Replace the deadline for one request and response exchange.
    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Replace the number of policy calls that may wait behind the worker.
    #[must_use]
    pub const fn request_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.request_capacity = capacity;
        self
    }

    /// Replace the number of unconsumed worker events retained for the host.
    #[must_use]
    pub const fn event_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.event_capacity = capacity;
        self
    }
}

/// Current coarse worker state, available even if an event was dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerStatus {
    /// The child has not produced a valid readiness message yet.
    Starting,
    /// The child is ready and accepting queued requests.
    Ready,
    /// Startup or request processing failed. Built-in behavior must be used.
    Unavailable,
    /// The client deliberately shut the worker down.
    Stopped,
}

impl WorkerStatus {
    const fn from_wire(value: u8) -> Self {
        match value {
            STATUS_STARTING => Self::Starting,
            STATUS_READY => Self::Ready,
            STATUS_UNAVAILABLE => Self::Unavailable,
            _ => Self::Stopped,
        }
    }

    const fn to_wire(self) -> u8 {
        match self {
            Self::Starting => STATUS_STARTING,
            Self::Ready => STATUS_READY,
            Self::Unavailable => STATUS_UNAVAILABLE,
            Self::Stopped => STATUS_STOPPED,
        }
    }
}

/// Result of nonblocking policy submission from an application event path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicySubmit {
    /// The request is queued and may later yield a [`PolicyEvent::Response`].
    Queued(RequestId),
    /// The bounded request queue is full. The caller must use built-in policy.
    DroppedFull,
    /// The worker is unavailable or shutting down. Use built-in policy.
    Unavailable,
}

/// Best-effort event emitted by the policy supervisor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyEvent {
    /// The child loaded all tracked and local startup files successfully.
    Ready,
    /// A validated response arrived for the only in-flight request.
    Response(PolicyResponse),
    /// The child failed and was terminated.
    Unavailable(WorkerFailure),
}

/// Result of polling the bounded supervisor event queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyEventPoll {
    /// One event was waiting.
    Event(PolicyEvent),
    /// The worker remains connected but no event is waiting.
    Empty,
    /// The supervisor thread has ended and no event remains.
    Disconnected,
}

/// Reason a worker was made unavailable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerFailure {
    /// The process could not be created.
    Spawn(String),
    /// A required standard-I/O pipe was absent.
    MissingPipe(&'static str),
    /// A dedicated I/O thread could not be created.
    IoThread(String),
    /// No valid readiness line arrived before the startup deadline.
    StartupTimeout,
    /// No response arrived before this request's deadline.
    RequestTimeout(RequestId),
    /// The child closed stdout before the expected protocol line.
    OutputEnded,
    /// The child closed stdout partway through a protocol line.
    TruncatedOutput,
    /// One output line exceeded the shared protocol byte limit.
    OversizedOutput,
    /// Worker stdout could not be read as UTF-8 or another read failed.
    Output(String),
    /// A request could not be written to the worker.
    Input(String),
    /// The readiness line violated the typed message schema.
    InvalidReady(MessageError),
    /// A response line violated the typed message schema.
    InvalidResponse(MessageError),
    /// A response did not match the only in-flight request.
    UnexpectedResponse {
        /// ID of the in-flight request.
        expected: RequestId,
        /// ID returned by the child.
        received: RequestId,
    },
    /// The child emitted a line while no request was in flight.
    UnsolicitedOutput,
    /// The child exited before orderly client shutdown.
    ProcessExited(Option<i32>),
    /// Inspecting or terminating the child failed.
    Process(String),
}

impl fmt::Display for WorkerFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "cannot spawn policy worker: {error}"),
            Self::MissingPipe(name) => write!(formatter, "policy worker has no {name} pipe"),
            Self::IoThread(error) => write!(formatter, "cannot start policy I/O thread: {error}"),
            Self::StartupTimeout => formatter.write_str("policy worker startup timed out"),
            Self::RequestTimeout(id) => {
                write!(formatter, "policy request {} timed out", id.get())
            }
            Self::OutputEnded => formatter.write_str("policy worker stdout ended"),
            Self::TruncatedOutput => {
                formatter.write_str("policy worker stdout ended inside a line")
            }
            Self::OversizedOutput => formatter.write_str("policy worker output is too large"),
            Self::Output(error) => write!(formatter, "cannot read policy worker output: {error}"),
            Self::Input(error) => write!(formatter, "cannot write policy worker input: {error}"),
            Self::InvalidReady(error) => {
                write!(formatter, "invalid policy readiness message: {error}")
            }
            Self::InvalidResponse(error) => write!(formatter, "invalid policy response: {error}"),
            Self::UnexpectedResponse { expected, received } => write!(
                formatter,
                "policy response ID {} does not match request {}",
                received.get(),
                expected.get()
            ),
            Self::UnsolicitedOutput => {
                formatter.write_str("policy worker emitted unsolicited output")
            }
            Self::ProcessExited(code) => {
                write!(formatter, "policy worker exited with status {code:?}")
            }
            Self::Process(error) => write!(formatter, "cannot inspect policy worker: {error}"),
        }
    }
}

impl std::error::Error for WorkerFailure {}

/// Asynchronous policy client safe to call from application event handling.
#[derive(Debug)]
pub struct PolicyClient {
    request_sender: Option<SyncSender<WireRequest>>,
    event_receiver: Receiver<PolicyEvent>,
    shutdown_sender: mpsc::Sender<()>,
    status: Arc<AtomicU8>,
    next_request_id: AtomicI64,
    supervisor: Option<JoinHandle<()>>,
}

impl PolicyClient {
    /// Start a process supervisor and return without waiting for Lisp startup.
    ///
    /// # Errors
    ///
    /// Returns an I/O error only if the Rust supervisor thread itself cannot
    /// be created. Child startup failures arrive asynchronously as
    /// [`PolicyEvent::Unavailable`] and [`WorkerStatus::Unavailable`].
    pub fn spawn(config: WorkerConfig) -> io::Result<Self> {
        let (request_sender, request_receiver) = mpsc::sync_channel(config.request_capacity.get());
        let (event_sender, event_receiver) = mpsc::sync_channel(config.event_capacity.get());
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let status = Arc::new(AtomicU8::new(STATUS_STARTING));
        let supervisor_status = Arc::clone(&status);
        let supervisor = thread::Builder::new()
            .name("retro-deck-policy".to_owned())
            .spawn(move || {
                supervise(
                    &config,
                    &request_receiver,
                    &event_sender,
                    &shutdown_receiver,
                    &supervisor_status,
                );
            })?;
        Ok(Self {
            request_sender: Some(request_sender),
            event_receiver,
            shutdown_sender,
            status,
            next_request_id: AtomicI64::new(FIRST_REQUEST_ID),
            supervisor: Some(supervisor),
        })
    }

    /// Return the worker state without waiting for the supervisor.
    #[must_use]
    pub fn status(&self) -> WorkerStatus {
        WorkerStatus::from_wire(self.status.load(Ordering::Acquire))
    }

    /// Encode and try to queue one policy call without waiting for Lisp.
    ///
    /// Encoding is bounded by the wire limits. Queue saturation or worker
    /// failure is reported as a normal fallback outcome.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] if the hook or arguments cannot be encoded.
    pub fn try_submit(&self, hook: &str, arguments: Value) -> Result<PolicySubmit, MessageError> {
        if matches!(
            self.status(),
            WorkerStatus::Unavailable | WorkerStatus::Stopped
        ) {
            return Ok(PolicySubmit::Unavailable);
        }
        let id = self.allocate_request_id();
        let line = PolicyRequest::new(id, hook, arguments)?.encode()?;
        let Some(sender) = &self.request_sender else {
            return Ok(PolicySubmit::Unavailable);
        };
        match sender.try_send(WireRequest { id, line }) {
            Ok(()) => Ok(PolicySubmit::Queued(id)),
            Err(TrySendError::Full(_)) => Ok(PolicySubmit::DroppedFull),
            Err(TrySendError::Disconnected(_)) => Ok(PolicySubmit::Unavailable),
        }
    }

    /// Poll one supervisor event without waiting.
    #[must_use]
    pub fn try_event(&self) -> PolicyEventPoll {
        match self.event_receiver.try_recv() {
            Ok(event) => PolicyEventPoll::Event(event),
            Err(TryRecvError::Empty) => PolicyEventPoll::Empty,
            Err(TryRecvError::Disconnected) => PolicyEventPoll::Disconnected,
        }
    }

    fn allocate_request_id(&self) -> RequestId {
        loop {
            let current = self.next_request_id.load(Ordering::Relaxed);
            let next = if current == i64::MAX {
                FIRST_REQUEST_ID
            } else {
                current + 1
            };
            if self
                .next_request_id
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return RequestId::new(current).unwrap_or(RequestId::ZERO);
            }
        }
    }
}

impl Drop for PolicyClient {
    fn drop(&mut self) {
        let _ = self.shutdown_sender.send(());
        self.request_sender.take();
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

#[derive(Debug)]
struct WireRequest {
    id: RequestId,
    line: String,
}

#[derive(Debug)]
enum ReaderEvent {
    Line(String),
    Ended,
    Truncated,
    Oversized,
    Failed(String),
}

#[derive(Debug)]
enum WaitResult {
    Line(String),
    Shutdown,
    TimedOut,
    Failed(WorkerFailure),
}

fn supervise(
    config: &WorkerConfig,
    request_receiver: &Receiver<WireRequest>,
    event_sender: &SyncSender<PolicyEvent>,
    shutdown_receiver: &Receiver<()>,
    status: &AtomicU8,
) {
    let mut child = match config.command.build().spawn() {
        Ok(child) => child,
        Err(error) => {
            fail(
                status,
                event_sender,
                WorkerFailure::Spawn(error.to_string()),
            );
            return;
        }
    };
    let Some(stdin) = child.stdin.take() else {
        fail(status, event_sender, WorkerFailure::MissingPipe("stdin"));
        terminate_child(&mut child);
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        fail(status, event_sender, WorkerFailure::MissingPipe("stdout"));
        terminate_child(&mut child);
        return;
    };

    let (line_sender, line_receiver) = mpsc::sync_channel(2);
    let reader = match spawn_reader(stdout, line_sender) {
        Ok(reader) => reader,
        Err(error) => {
            fail(status, event_sender, WorkerFailure::IoThread(error));
            terminate_child(&mut child);
            return;
        }
    };
    let (write_sender, write_receiver) = mpsc::sync_channel(1);
    let (write_error_sender, write_error_receiver) = mpsc::sync_channel(1);
    let writer = match spawn_writer(stdin, write_receiver, write_error_sender) {
        Ok(writer) => writer,
        Err(error) => {
            fail(status, event_sender, WorkerFailure::IoThread(error));
            terminate_child(&mut child);
            let _ = reader.join();
            return;
        }
    };

    let outcome = run_ready_worker(
        config,
        request_receiver,
        event_sender,
        shutdown_receiver,
        status,
        &mut child,
        &line_receiver,
        &write_sender,
        &write_error_receiver,
    );

    drop(write_sender);
    terminate_child(&mut child);
    let _ = writer.join();
    let _ = reader.join();

    match outcome {
        Ok(()) => status.store(WorkerStatus::Stopped.to_wire(), Ordering::Release),
        Err(failure) => fail(status, event_sender, failure),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the supervisor loop keeps each owned channel and child explicit"
)]
fn run_ready_worker(
    config: &WorkerConfig,
    request_receiver: &Receiver<WireRequest>,
    event_sender: &SyncSender<PolicyEvent>,
    shutdown_receiver: &Receiver<()>,
    status: &AtomicU8,
    child: &mut Child,
    line_receiver: &Receiver<ReaderEvent>,
    write_sender: &SyncSender<Vec<u8>>,
    write_error_receiver: &Receiver<String>,
) -> Result<(), WorkerFailure> {
    match wait_for_line(
        line_receiver,
        write_error_receiver,
        shutdown_receiver,
        child,
        config.startup_timeout,
    ) {
        WaitResult::Line(line) => decode_ready(&line).map_err(WorkerFailure::InvalidReady)?,
        WaitResult::Shutdown => return Ok(()),
        WaitResult::TimedOut => return Err(WorkerFailure::StartupTimeout),
        WaitResult::Failed(failure) => return Err(failure),
    }

    status.store(WorkerStatus::Ready.to_wire(), Ordering::Release);
    publish(event_sender, PolicyEvent::Ready);

    loop {
        if shutdown_requested(shutdown_receiver) {
            return Ok(());
        }
        if let Some(failure) = idle_failure(line_receiver, write_error_receiver, child) {
            return Err(failure);
        }
        match request_receiver.recv_timeout(SUPERVISOR_POLL_INTERVAL) {
            Ok(request) => {
                let mut bytes = request.line.into_bytes();
                bytes.push(b'\n');
                match write_sender.try_send(bytes) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        return Err(WorkerFailure::Input(
                            "policy writer queue is unexpectedly full".to_owned(),
                        ));
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        return Err(WorkerFailure::Input(
                            "policy writer thread ended".to_owned(),
                        ));
                    }
                }
                match wait_for_line(
                    line_receiver,
                    write_error_receiver,
                    shutdown_receiver,
                    child,
                    config.request_timeout,
                ) {
                    WaitResult::Line(line) => {
                        let response = PolicyResponse::decode(&line)
                            .map_err(WorkerFailure::InvalidResponse)?;
                        if response.id() != request.id {
                            return Err(WorkerFailure::UnexpectedResponse {
                                expected: request.id,
                                received: response.id(),
                            });
                        }
                        publish(event_sender, PolicyEvent::Response(response));
                    }
                    WaitResult::Shutdown => return Ok(()),
                    WaitResult::TimedOut => {
                        return Err(WorkerFailure::RequestTimeout(request.id));
                    }
                    WaitResult::Failed(failure) => return Err(failure),
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn wait_for_line(
    line_receiver: &Receiver<ReaderEvent>,
    write_error_receiver: &Receiver<String>,
    shutdown_receiver: &Receiver<()>,
    child: &mut Child,
    timeout: Duration,
) -> WaitResult {
    let started = Instant::now();
    loop {
        if shutdown_requested(shutdown_receiver) {
            return WaitResult::Shutdown;
        }
        if let Ok(error) = write_error_receiver.try_recv() {
            return WaitResult::Failed(WorkerFailure::Input(error));
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                return WaitResult::Failed(WorkerFailure::ProcessExited(exit.code()));
            }
            Ok(None) => {}
            Err(error) => return WaitResult::Failed(WorkerFailure::Process(error.to_string())),
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return WaitResult::TimedOut;
        }
        let interval = remaining.min(SUPERVISOR_POLL_INTERVAL);
        match line_receiver.recv_timeout(interval) {
            Ok(event) => return reader_wait_result(event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return WaitResult::Failed(WorkerFailure::OutputEnded);
            }
        }
    }
}

fn reader_wait_result(event: ReaderEvent) -> WaitResult {
    match event {
        ReaderEvent::Line(line) => WaitResult::Line(line),
        ReaderEvent::Ended => WaitResult::Failed(WorkerFailure::OutputEnded),
        ReaderEvent::Truncated => WaitResult::Failed(WorkerFailure::TruncatedOutput),
        ReaderEvent::Oversized => WaitResult::Failed(WorkerFailure::OversizedOutput),
        ReaderEvent::Failed(error) => WaitResult::Failed(WorkerFailure::Output(error)),
    }
}

fn idle_failure(
    line_receiver: &Receiver<ReaderEvent>,
    write_error_receiver: &Receiver<String>,
    child: &mut Child,
) -> Option<WorkerFailure> {
    if let Ok(error) = write_error_receiver.try_recv() {
        return Some(WorkerFailure::Input(error));
    }
    match line_receiver.try_recv() {
        Ok(ReaderEvent::Line(_)) => return Some(WorkerFailure::UnsolicitedOutput),
        Ok(event) => {
            if let WaitResult::Failed(failure) = reader_wait_result(event) {
                return Some(failure);
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => return Some(WorkerFailure::OutputEnded),
    }
    match child.try_wait() {
        Ok(Some(exit)) => Some(WorkerFailure::ProcessExited(exit.code())),
        Ok(None) => None,
        Err(error) => Some(WorkerFailure::Process(error.to_string())),
    }
}

fn spawn_reader(
    stdout: ChildStdout,
    sender: SyncSender<ReaderEvent>,
) -> Result<JoinHandle<()>, String> {
    thread::Builder::new()
        .name("retro-deck-policy-out".to_owned())
        .spawn(move || read_lines(stdout, &sender))
        .map_err(|error| error.to_string())
}

fn read_lines(stdout: ChildStdout, sender: &SyncSender<ReaderEvent>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut bytes = Vec::with_capacity(256);
        loop {
            let mut byte = [0_u8; 1];
            match reader.read(&mut byte) {
                Ok(0) => {
                    let event = if bytes.is_empty() {
                        ReaderEvent::Ended
                    } else {
                        ReaderEvent::Truncated
                    };
                    let _ = sender.try_send(event);
                    return;
                }
                Ok(_) if byte[0] == b'\n' => {
                    let event = match String::from_utf8(bytes) {
                        Ok(line) => ReaderEvent::Line(line),
                        Err(error) => ReaderEvent::Failed(error.to_string()),
                    };
                    if sender.try_send(event).is_err() {
                        return;
                    }
                    break;
                }
                Ok(_) => {
                    if bytes.len() >= DEFAULT_MAX_BYTES {
                        let _ = sender.try_send(ReaderEvent::Oversized);
                        return;
                    }
                    bytes.push(byte[0]);
                }
                Err(error) => {
                    let _ = sender.try_send(ReaderEvent::Failed(error.to_string()));
                    return;
                }
            }
        }
    }
}

fn spawn_writer(
    stdin: ChildStdin,
    receiver: Receiver<Vec<u8>>,
    error_sender: SyncSender<String>,
) -> Result<JoinHandle<()>, String> {
    thread::Builder::new()
        .name("retro-deck-policy-in".to_owned())
        .spawn(move || write_lines(stdin, &receiver, &error_sender))
        .map_err(|error| error.to_string())
}

fn write_lines(
    mut stdin: ChildStdin,
    receiver: &Receiver<Vec<u8>>,
    error_sender: &SyncSender<String>,
) {
    while let Ok(line) = receiver.recv() {
        if let Err(error) = stdin.write_all(&line).and_then(|()| stdin.flush()) {
            let _ = error_sender.try_send(error.to_string());
            return;
        }
    }
}

fn shutdown_requested(receiver: &Receiver<()>) -> bool {
    matches!(
        receiver.try_recv(),
        Ok(()) | Err(TryRecvError::Disconnected)
    )
}

fn publish(sender: &SyncSender<PolicyEvent>, event: PolicyEvent) {
    let _ = sender.try_send(event);
}

fn fail(status: &AtomicU8, sender: &SyncSender<PolicyEvent>, failure: WorkerFailure) {
    status.store(WorkerStatus::Unavailable.to_wire(), Ordering::Release);
    publish(sender, PolicyEvent::Unavailable(failure));
}

fn terminate_child(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            let _ = child.kill();
        }
    }
    let _ = child.wait();
}

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap_or(NonZeroUsize::MIN)
}

#[cfg(test)]
mod tests {
    use super::{
        PolicyClient, PolicyEvent, PolicyEventPoll, PolicySubmit, WorkerCommand, WorkerConfig,
        WorkerFailure, WorkerStatus, nonzero,
    };
    use crate::{PolicyResponse, Value};
    use std::{
        env,
        path::PathBuf,
        thread,
        time::{Duration, Instant},
    };

    fn shell_worker(script: &str) -> WorkerCommand {
        WorkerCommand::new("/bin/sh").arg("-c").arg(script)
    }

    fn test_config(command: WorkerCommand) -> WorkerConfig {
        WorkerConfig::new(command)
            .startup_timeout(Duration::from_millis(500))
            .request_timeout(Duration::from_millis(150))
            .request_capacity(nonzero(2))
            .event_capacity(nonzero(8))
    }

    fn spawn_test_client(command: WorkerCommand) -> Option<PolicyClient> {
        let result = PolicyClient::spawn(test_config(command));
        assert!(result.is_ok(), "cannot spawn test supervisor: {result:?}");
        result.ok()
    }

    fn wait_event(client: &PolicyClient, timeout: Duration) -> Option<PolicyEvent> {
        let started = Instant::now();
        loop {
            match client.try_event() {
                PolicyEventPoll::Event(event) => return Some(event),
                PolicyEventPoll::Disconnected => return None,
                PolicyEventPoll::Empty => {}
            }
            if started.elapsed() >= timeout {
                return None;
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn wait_ready(client: &PolicyClient) -> bool {
        matches!(
            wait_event(client, Duration::from_secs(1)),
            Some(PolicyEvent::Ready)
        )
    }

    fn exercise_common_lisp_worker(command: WorkerCommand) {
        let config = WorkerConfig::new(command)
            .startup_timeout(Duration::from_secs(5))
            .request_timeout(Duration::from_secs(1))
            .request_capacity(nonzero(2))
            .event_capacity(nonzero(8));
        let result = PolicyClient::spawn(config);
        assert!(result.is_ok(), "cannot spawn Common Lisp test worker");
        let Some(client) = result.ok() else {
            return;
        };
        assert!(matches!(
            wait_event(&client, Duration::from_secs(5)),
            Some(PolicyEvent::Ready)
        ));
        let arguments = Value::List(vec![
            Value::Keyword("elapsed-centiseconds".to_owned()),
            Value::Integer(1_000),
            Value::Keyword("input".to_owned()),
            Value::Keyword("touch".to_owned()),
        ]);
        assert!(matches!(
            client.try_submit("ten-seconds/result", arguments),
            Ok(PolicySubmit::Queued(_))
        ));
        assert!(matches!(
            wait_event(&client, Duration::from_secs(2)),
            Some(PolicyEvent::Response(PolicyResponse::Ok {
                value: Value::List(values),
                ..
            })) if values == vec![
                Value::Keyword("display-centiseconds".to_owned()),
                Value::Integer(1_000),
                Value::Keyword("cue".to_owned()),
                Value::Keyword("exact".to_owned()),
            ]
        ));
    }

    #[test]
    fn valid_worker_round_trip_is_typed() {
        let script = "printf '(:ready :version 1)\\n'; \
                      IFS= read -r request; \
                      printf '(:response :version 1 :id 1 :status :ok :value (:answer 42))\\n'; \
                      IFS= read -r rest";
        let Some(client) = spawn_test_client(shell_worker(script)) else {
            return;
        };
        assert!(wait_ready(&client));
        assert_eq!(
            client.try_submit("test", Value::Nil),
            Ok(PolicySubmit::Queued(
                crate::RequestId::new(1).unwrap_or(crate::RequestId::ZERO)
            ))
        );
        assert!(matches!(
            wait_event(&client, Duration::from_secs(1)),
            Some(PolicyEvent::Response(PolicyResponse::Ok {
                value: Value::List(values),
                ..
            })) if values == vec![Value::Keyword("answer".to_owned()), Value::Integer(42)]
        ));
    }

    #[test]
    fn malformed_readiness_fails_closed() {
        let Some(client) = spawn_test_client(shell_worker(
            "printf '(:ready :version 2)\\n'; IFS= read -r rest",
        )) else {
            return;
        };
        assert!(matches!(
            wait_event(&client, Duration::from_secs(1)),
            Some(PolicyEvent::Unavailable(WorkerFailure::InvalidReady(_)))
        ));
        assert_eq!(client.status(), WorkerStatus::Unavailable);
        assert_eq!(
            client.try_submit("test", Value::Nil),
            Ok(PolicySubmit::Unavailable)
        );
    }

    #[test]
    fn unresponsive_request_is_killed_at_its_deadline() {
        let Some(client) = spawn_test_client(shell_worker(
            "printf '(:ready :version 1)\\n'; IFS= read -r request; IFS= read -r rest",
        )) else {
            return;
        };
        assert!(wait_ready(&client));
        assert!(matches!(
            client.try_submit("test", Value::Nil),
            Ok(PolicySubmit::Queued(_))
        ));
        assert!(matches!(
            wait_event(&client, Duration::from_secs(1)),
            Some(PolicyEvent::Unavailable(WorkerFailure::RequestTimeout(_)))
        ));
        assert_eq!(client.status(), WorkerStatus::Unavailable);
    }

    #[test]
    fn request_submission_is_bounded_during_startup() {
        let config = test_config(shell_worker("IFS= read -r rest"))
            .request_capacity(nonzero(1))
            .startup_timeout(Duration::from_secs(1));
        let result = PolicyClient::spawn(config);
        assert!(result.is_ok(), "cannot spawn bounded-queue test supervisor");
        let Some(client) = result.ok() else {
            return;
        };
        assert!(matches!(
            client.try_submit("one", Value::Nil),
            Ok(PolicySubmit::Queued(_))
        ));
        assert_eq!(
            client.try_submit("two", Value::Nil),
            Ok(PolicySubmit::DroppedFull)
        );
    }

    #[test]
    fn installed_common_lisp_worker_round_trips_when_configured() {
        let Some(program) = env::var_os("RETRO_DECK_TEST_SBCL") else {
            return;
        };
        let lisp_script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lisp/run-worker.lisp");
        exercise_common_lisp_worker(
            WorkerCommand::new(program)
                .arg("--script")
                .arg(lisp_script.into_os_string()),
        );
    }

    #[test]
    fn installed_ecl_worker_round_trips_when_configured() {
        let Some(program) = env::var_os("RETRO_DECK_TEST_ECL") else {
            return;
        };
        let lisp_script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lisp/run-worker.lisp");
        exercise_common_lisp_worker(
            WorkerCommand::new(program)
                .arg("--norc")
                .arg("--shell")
                .arg(lisp_script.into_os_string()),
        );
    }
}
