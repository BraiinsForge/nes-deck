//! Nonblocking client and process supervisor for the Common Lisp worker.

use std::{
    ffi::OsString,
    fmt, io,
    num::NonZeroUsize,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU8, Ordering},
        mpsc::{self as std_mpsc, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    process::{Child, ChildStdout, Command},
    runtime::Builder,
    sync::{
        mpsc::{self as tokio_mpsc, error::TrySendError},
        oneshot,
    },
    time,
};

use crate::{
    DEFAULT_MAX_BYTES, MessageError, PolicyRequest, PolicyResponse, RequestId, Value, decode_ready,
};

const FIRST_REQUEST_ID: i64 = 1;

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
    /// The asynchronous supervisor runtime could not be created.
    Runtime(String),
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
            Self::Runtime(error) => write!(formatter, "cannot start policy runtime: {error}"),
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
    request_sender: Option<tokio_mpsc::Sender<WireRequest>>,
    event_receiver: Receiver<PolicyEvent>,
    shutdown_sender: Option<oneshot::Sender<()>>,
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
        let (request_sender, request_receiver) = tokio_mpsc::channel(config.request_capacity.get());
        let (event_sender, event_receiver) = std_mpsc::sync_channel(config.event_capacity.get());
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let status = Arc::new(AtomicU8::new(STATUS_STARTING));
        let supervisor_status = Arc::clone(&status);
        let supervisor = thread::Builder::new()
            .name("retro-deck-policy".to_owned())
            .spawn(move || {
                supervise(
                    &config,
                    request_receiver,
                    &event_sender,
                    shutdown_receiver,
                    &supervisor_status,
                );
            })?;
        Ok(Self {
            request_sender: Some(request_sender),
            event_receiver,
            shutdown_sender: Some(shutdown_sender),
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
            Err(TrySendError::Closed(_)) => Ok(PolicySubmit::Unavailable),
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
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
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
    request_receiver: tokio_mpsc::Receiver<WireRequest>,
    event_sender: &SyncSender<PolicyEvent>,
    shutdown_receiver: oneshot::Receiver<()>,
    status: &AtomicU8,
) {
    let runtime = match Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            fail(
                status,
                event_sender,
                WorkerFailure::Runtime(error.to_string()),
            );
            return;
        }
    };
    let outcome = runtime.block_on(run_worker(
        config,
        request_receiver,
        event_sender,
        shutdown_receiver,
        status,
    ));

    match outcome {
        Ok(()) => status.store(WorkerStatus::Stopped.to_wire(), Ordering::Release),
        Err(failure) => fail(status, event_sender, failure),
    }
}

async fn run_worker(
    config: &WorkerConfig,
    request_receiver: tokio_mpsc::Receiver<WireRequest>,
    event_sender: &SyncSender<PolicyEvent>,
    shutdown_receiver: oneshot::Receiver<()>,
    status: &AtomicU8,
) -> Result<(), WorkerFailure> {
    let mut child = config
        .command
        .build()
        .spawn()
        .map_err(|error| WorkerFailure::Spawn(error.to_string()))?;
    let Some(stdin) = child.stdin.take() else {
        terminate_child(&mut child).await;
        return Err(WorkerFailure::MissingPipe("stdin"));
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child).await;
        return Err(WorkerFailure::MissingPipe("stdout"));
    };

    let (line_sender, line_receiver) = tokio_mpsc::channel(2);
    let reader = tokio::spawn(read_lines(stdout, line_sender));
    let result = serve_worker(
        config,
        request_receiver,
        event_sender,
        shutdown_receiver,
        status,
        &mut child,
        stdin,
        line_receiver,
    )
    .await;
    terminate_child(&mut child).await;
    reader.abort();
    let _ = reader.await;
    result
}

#[expect(
    clippy::too_many_arguments,
    reason = "the loop explicitly owns the child and each bounded channel"
)]
async fn serve_worker(
    config: &WorkerConfig,
    mut request_receiver: tokio_mpsc::Receiver<WireRequest>,
    event_sender: &SyncSender<PolicyEvent>,
    mut shutdown_receiver: oneshot::Receiver<()>,
    status: &AtomicU8,
    child: &mut Child,
    mut stdin: tokio::process::ChildStdin,
    mut line_receiver: tokio_mpsc::Receiver<ReaderEvent>,
) -> Result<(), WorkerFailure> {
    match wait_for_line(
        &mut line_receiver,
        &mut shutdown_receiver,
        child,
        config.startup_timeout,
    )
    .await
    {
        WaitResult::Line(line) => decode_ready(&line).map_err(WorkerFailure::InvalidReady)?,
        WaitResult::Shutdown => return Ok(()),
        WaitResult::TimedOut => return Err(WorkerFailure::StartupTimeout),
        WaitResult::Failed(failure) => return Err(failure),
    }

    status.store(WorkerStatus::Ready.to_wire(), Ordering::Release);
    publish(event_sender, PolicyEvent::Ready);

    loop {
        let request = tokio::select! {
            biased;
            _ = &mut shutdown_receiver => return Ok(()),
            event = line_receiver.recv() => return Err(idle_failure(event)),
            exit = child.wait() => return Err(process_failure(exit)),
            request = request_receiver.recv() => match request {
                Some(request) => request,
                None => return Ok(()),
            },
        };
        match exchange(
            &mut stdin,
            &mut line_receiver,
            &mut shutdown_receiver,
            child,
            &request.line,
            config.request_timeout,
        )
        .await
        {
            WaitResult::Line(line) => {
                let response =
                    PolicyResponse::decode(&line).map_err(WorkerFailure::InvalidResponse)?;
                if response.id() != request.id {
                    return Err(WorkerFailure::UnexpectedResponse {
                        expected: request.id,
                        received: response.id(),
                    });
                }
                publish(event_sender, PolicyEvent::Response(response));
            }
            WaitResult::Shutdown => return Ok(()),
            WaitResult::TimedOut => return Err(WorkerFailure::RequestTimeout(request.id)),
            WaitResult::Failed(failure) => return Err(failure),
        }
    }
}

async fn wait_for_line(
    line_receiver: &mut tokio_mpsc::Receiver<ReaderEvent>,
    shutdown_receiver: &mut oneshot::Receiver<()>,
    child: &mut Child,
    timeout: Duration,
) -> WaitResult {
    tokio::select! {
        biased;
        _ = shutdown_receiver => WaitResult::Shutdown,
        event = line_receiver.recv() => reader_wait_result(event),
        exit = child.wait() => WaitResult::Failed(process_failure(exit)),
        () = time::sleep(timeout) => WaitResult::TimedOut,
    }
}

async fn exchange(
    stdin: &mut tokio::process::ChildStdin,
    line_receiver: &mut tokio_mpsc::Receiver<ReaderEvent>,
    shutdown_receiver: &mut oneshot::Receiver<()>,
    child: &mut Child,
    line: &str,
    timeout: Duration,
) -> WaitResult {
    let operation = async {
        if let Err(error) = stdin.write_all(line.as_bytes()).await {
            return WaitResult::Failed(WorkerFailure::Input(error.to_string()));
        }
        if let Err(error) = stdin.write_all(b"\n").await {
            return WaitResult::Failed(WorkerFailure::Input(error.to_string()));
        }
        if let Err(error) = stdin.flush().await {
            return WaitResult::Failed(WorkerFailure::Input(error.to_string()));
        }
        reader_wait_result(line_receiver.recv().await)
    };
    tokio::select! {
        biased;
        _ = shutdown_receiver => WaitResult::Shutdown,
        result = time::timeout(timeout, operation) => match result {
            Ok(result) => result,
            Err(_) => WaitResult::TimedOut,
        },
        exit = child.wait() => WaitResult::Failed(process_failure(exit)),
    }
}

fn reader_wait_result(event: Option<ReaderEvent>) -> WaitResult {
    match event {
        Some(ReaderEvent::Line(line)) => WaitResult::Line(line),
        Some(ReaderEvent::Ended) | None => WaitResult::Failed(WorkerFailure::OutputEnded),
        Some(ReaderEvent::Truncated) => WaitResult::Failed(WorkerFailure::TruncatedOutput),
        Some(ReaderEvent::Oversized) => WaitResult::Failed(WorkerFailure::OversizedOutput),
        Some(ReaderEvent::Failed(error)) => WaitResult::Failed(WorkerFailure::Output(error)),
    }
}

fn idle_failure(event: Option<ReaderEvent>) -> WorkerFailure {
    match event {
        Some(ReaderEvent::Line(_)) => WorkerFailure::UnsolicitedOutput,
        Some(ReaderEvent::Ended) | None => WorkerFailure::OutputEnded,
        Some(ReaderEvent::Truncated) => WorkerFailure::TruncatedOutput,
        Some(ReaderEvent::Oversized) => WorkerFailure::OversizedOutput,
        Some(ReaderEvent::Failed(error)) => WorkerFailure::Output(error),
    }
}

fn process_failure(result: io::Result<std::process::ExitStatus>) -> WorkerFailure {
    match result {
        Ok(exit) => WorkerFailure::ProcessExited(exit.code()),
        Err(error) => WorkerFailure::Process(error.to_string()),
    }
}

async fn read_lines(stdout: ChildStdout, sender: tokio_mpsc::Sender<ReaderEvent>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let event = read_line(&mut reader).await;
        let terminal = !matches!(event, ReaderEvent::Line(_));
        if sender.send(event).await.is_err() || terminal {
            return;
        }
    }
}

async fn read_line(reader: &mut BufReader<ChildStdout>) -> ReaderEvent {
    let mut bytes = Vec::with_capacity(256);
    loop {
        let available = match reader.fill_buf().await {
            Ok(available) => available,
            Err(error) => return ReaderEvent::Failed(error.to_string()),
        };
        if available.is_empty() {
            return if bytes.is_empty() {
                ReaderEvent::Ended
            } else {
                ReaderEvent::Truncated
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let amount = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(amount) > DEFAULT_MAX_BYTES {
            return ReaderEvent::Oversized;
        }
        let Some(chunk) = available.get(..amount) else {
            return ReaderEvent::Failed("policy reader produced invalid bounds".to_owned());
        };
        bytes.extend_from_slice(chunk);
        reader.consume(amount.saturating_add(usize::from(newline.is_some())));
        if newline.is_some() {
            return match String::from_utf8(bytes) {
                Ok(line) => ReaderEvent::Line(line),
                Err(error) => ReaderEvent::Failed(error.to_string()),
            };
        }
    }
}

fn publish(sender: &SyncSender<PolicyEvent>, event: PolicyEvent) {
    let _ = sender.try_send(event);
}

fn fail(status: &AtomicU8, sender: &SyncSender<PolicyEvent>, failure: WorkerFailure) {
    status.store(WorkerStatus::Unavailable.to_wire(), Ordering::Release);
    publish(sender, PolicyEvent::Unavailable(failure));
}

async fn terminate_child(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            let _ = child.kill().await;
        }
    }
    let _ = child.wait().await;
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
