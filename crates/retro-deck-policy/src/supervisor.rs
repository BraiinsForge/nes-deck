//! One-decision process boundary for the Common Lisp policy worker.

use std::{
    ffi::OsString,
    fmt, io,
    path::PathBuf,
    process::Stdio,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread::{self, JoinHandle},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    process::{Child, ChildStdout, Command},
    runtime::Builder,
    sync::oneshot,
    time,
};

use crate::{
    DEFAULT_MAX_BYTES, MessageError, PolicyRequest, PolicyResponse, RequestId, Value, decode_ready,
};

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

/// Deadlines for one worker process and its single decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerConfig {
    command: WorkerCommand,
    startup_timeout: Duration,
    request_timeout: Duration,
}

impl WorkerConfig {
    /// Construct production-oriented defaults for `command`.
    #[must_use]
    pub const fn new(command: WorkerCommand) -> Self {
        Self {
            command,
            startup_timeout: Duration::from_secs(3),
            request_timeout: Duration::from_millis(250),
        }
    }

    /// Replace the startup readiness deadline.
    #[must_use]
    pub const fn startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    /// Replace the deadline for the request and response exchange.
    #[must_use]
    pub const fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }
}

/// Result of nonblocking policy submission from an application event path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicySubmit {
    /// The only request was handed to the supervisor.
    Queued(RequestId),
    /// The request was already submitted or the worker has stopped.
    Unavailable,
}

/// Terminal event emitted by a one-decision policy worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyEvent {
    /// The worker returned one validated response.
    Response(PolicyResponse),
    /// Startup or request processing failed and built-in behavior must be used.
    Unavailable(WorkerFailure),
}

/// Result of polling the supervisor without waiting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyEventPoll {
    /// One terminal event was waiting.
    Event(PolicyEvent),
    /// The worker remains connected but no event is waiting.
    Empty,
    /// The supervisor ended and no event remains.
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
    /// No response arrived before the request deadline.
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
    /// A response did not match the one request.
    UnexpectedResponse {
        /// ID of the request.
        expected: RequestId,
        /// ID returned by the child.
        received: RequestId,
    },
    /// The child emitted a line before receiving its request.
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

/// Preloaded policy process accepting one nonblocking decision request.
#[derive(Debug)]
pub struct PolicyClient {
    request_sender: Option<oneshot::Sender<WireRequest>>,
    event_receiver: Receiver<PolicyEvent>,
    shutdown_sender: Option<oneshot::Sender<()>>,
    supervisor: Option<JoinHandle<()>>,
}

impl PolicyClient {
    /// Start loading policy in a child and return without waiting for Lisp.
    ///
    /// The client accepts exactly one request. A caller that needs another
    /// decision starts another worker while the next interaction is underway.
    ///
    /// # Errors
    ///
    /// Returns an I/O error only if the Rust supervisor thread itself cannot
    /// be created. Child failures arrive as [`PolicyEvent::Unavailable`].
    pub fn spawn(config: WorkerConfig) -> io::Result<Self> {
        let (request_sender, request_receiver) = oneshot::channel();
        let (event_sender, event_receiver) = mpsc::sync_channel(1);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let supervisor = thread::Builder::new()
            .name("retro-deck-policy".to_owned())
            .spawn(move || {
                supervise(&config, request_receiver, &event_sender, shutdown_receiver);
            })?;
        Ok(Self {
            request_sender: Some(request_sender),
            event_receiver,
            shutdown_sender: Some(shutdown_sender),
            supervisor: Some(supervisor),
        })
    }

    /// Encode and hand off the worker's only policy call without waiting.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] if the hook or arguments cannot be encoded.
    pub fn try_submit(
        &mut self,
        hook: &str,
        arguments: Value,
    ) -> Result<PolicySubmit, MessageError> {
        let request_id = policy_request_id();
        let request = PolicyRequest::new(request_id, hook, arguments)?;
        let line = request.encode()?;
        let Some(sender) = self.request_sender.take() else {
            return Ok(PolicySubmit::Unavailable);
        };
        sender
            .send(WireRequest {
                id: request_id,
                line,
            })
            .map_or(Ok(PolicySubmit::Unavailable), |()| {
                Ok(PolicySubmit::Queued(request_id))
            })
    }

    /// Poll the worker's terminal event without waiting.
    #[must_use]
    pub fn try_event(&self) -> PolicyEventPoll {
        match self.event_receiver.try_recv() {
            Ok(event) => PolicyEventPoll::Event(event),
            Err(TryRecvError::Empty) => PolicyEventPoll::Empty,
            Err(TryRecvError::Disconnected) => PolicyEventPoll::Disconnected,
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

enum WaitResult {
    Line(String),
    Shutdown,
    TimedOut,
    Failed(WorkerFailure),
}

fn supervise(
    config: &WorkerConfig,
    request_receiver: oneshot::Receiver<WireRequest>,
    event_sender: &SyncSender<PolicyEvent>,
    shutdown_receiver: oneshot::Receiver<()>,
) {
    let runtime = match Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            publish(
                event_sender,
                PolicyEvent::Unavailable(WorkerFailure::Runtime(error.to_string())),
            );
            return;
        }
    };
    match runtime.block_on(run_worker(config, request_receiver, shutdown_receiver)) {
        Ok(Some(response)) => publish(event_sender, PolicyEvent::Response(response)),
        Ok(None) => {}
        Err(failure) => publish(event_sender, PolicyEvent::Unavailable(failure)),
    }
}

async fn run_worker(
    config: &WorkerConfig,
    request_receiver: oneshot::Receiver<WireRequest>,
    shutdown_receiver: oneshot::Receiver<()>,
) -> Result<Option<PolicyResponse>, WorkerFailure> {
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

    let result = serve_worker(
        config,
        request_receiver,
        shutdown_receiver,
        &mut child,
        stdin,
        stdout,
    )
    .await;
    terminate_child(&mut child).await;
    result
}

async fn serve_worker(
    config: &WorkerConfig,
    mut request_receiver: oneshot::Receiver<WireRequest>,
    mut shutdown_receiver: oneshot::Receiver<()>,
    child: &mut Child,
    mut stdin: tokio::process::ChildStdin,
    stdout: ChildStdout,
) -> Result<Option<PolicyResponse>, WorkerFailure> {
    let mut stdout = BufReader::new(stdout);
    match wait_for_line(
        &mut stdout,
        &mut shutdown_receiver,
        child,
        config.startup_timeout,
    )
    .await
    {
        WaitResult::Line(line) => decode_ready(&line).map_err(WorkerFailure::InvalidReady)?,
        WaitResult::Shutdown => return Ok(None),
        WaitResult::TimedOut => return Err(WorkerFailure::StartupTimeout),
        WaitResult::Failed(failure) => return Err(failure),
    }

    let request = tokio::select! {
        biased;
        _ = &mut shutdown_receiver => return Ok(None),
        exit = child.wait() => return Err(process_failure(exit)),
        output = read_line(&mut stdout) => {
            output?;
            return Err(WorkerFailure::UnsolicitedOutput);
        }
        request = &mut request_receiver => match request {
            Ok(request) => request,
            Err(_) => return Ok(None),
        },
    };

    match exchange(
        &mut stdin,
        &mut stdout,
        &mut shutdown_receiver,
        child,
        &request.line,
        config.request_timeout,
    )
    .await
    {
        WaitResult::Line(line) => {
            let response = PolicyResponse::decode(&line).map_err(WorkerFailure::InvalidResponse)?;
            if response.id() != request.id {
                return Err(WorkerFailure::UnexpectedResponse {
                    expected: request.id,
                    received: response.id(),
                });
            }
            Ok(Some(response))
        }
        WaitResult::Shutdown => Ok(None),
        WaitResult::TimedOut => Err(WorkerFailure::RequestTimeout(request.id)),
        WaitResult::Failed(failure) => Err(failure),
    }
}

async fn wait_for_line(
    stdout: &mut BufReader<ChildStdout>,
    shutdown_receiver: &mut oneshot::Receiver<()>,
    child: &mut Child,
    timeout: Duration,
) -> WaitResult {
    tokio::select! {
        biased;
        _ = shutdown_receiver => WaitResult::Shutdown,
        line = read_line(stdout) => line.map_or_else(WaitResult::Failed, WaitResult::Line),
        exit = child.wait() => WaitResult::Failed(process_failure(exit)),
        () = time::sleep(timeout) => WaitResult::TimedOut,
    }
}

async fn exchange(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    shutdown_receiver: &mut oneshot::Receiver<()>,
    child: &mut Child,
    line: &str,
    timeout: Duration,
) -> WaitResult {
    let operation = async {
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| WorkerFailure::Input(error.to_string()))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| WorkerFailure::Input(error.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|error| WorkerFailure::Input(error.to_string()))?;
        read_line(stdout).await
    };
    tokio::select! {
        biased;
        _ = shutdown_receiver => WaitResult::Shutdown,
        result = time::timeout(timeout, operation) => match result {
            Ok(Ok(line)) => WaitResult::Line(line),
            Ok(Err(failure)) => WaitResult::Failed(failure),
            Err(_) => WaitResult::TimedOut,
        },
        exit = child.wait() => WaitResult::Failed(process_failure(exit)),
    }
}

async fn read_line(reader: &mut BufReader<ChildStdout>) -> Result<String, WorkerFailure> {
    let mut bytes = Vec::with_capacity(256);
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| WorkerFailure::Output(error.to_string()))?;
        if available.is_empty() {
            return Err(if bytes.is_empty() {
                WorkerFailure::OutputEnded
            } else {
                WorkerFailure::TruncatedOutput
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let amount = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(amount) > DEFAULT_MAX_BYTES {
            return Err(WorkerFailure::OversizedOutput);
        }
        let Some(chunk) = available.get(..amount) else {
            return Err(WorkerFailure::Output(
                "policy reader produced invalid bounds".to_owned(),
            ));
        };
        bytes.extend_from_slice(chunk);
        reader.consume(amount.saturating_add(usize::from(newline.is_some())));
        if newline.is_some() {
            return String::from_utf8(bytes)
                .map_err(|error| WorkerFailure::Output(error.to_string()));
        }
    }
}

fn process_failure(result: io::Result<std::process::ExitStatus>) -> WorkerFailure {
    match result {
        Ok(exit) => WorkerFailure::ProcessExited(exit.code()),
        Err(error) => WorkerFailure::Process(error.to_string()),
    }
}

fn publish(sender: &SyncSender<PolicyEvent>, event: PolicyEvent) {
    let _ = sender.try_send(event);
}

fn policy_request_id() -> RequestId {
    RequestId::new(1).unwrap_or(RequestId::ZERO)
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

#[cfg(test)]
mod tests {
    use super::{
        PolicyClient, PolicyEvent, PolicyEventPoll, PolicySubmit, WorkerCommand, WorkerConfig,
        WorkerFailure,
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

    fn exercise_common_lisp_worker(command: WorkerCommand) {
        let config = WorkerConfig::new(command)
            .startup_timeout(Duration::from_secs(5))
            .request_timeout(Duration::from_secs(1));
        let result = PolicyClient::spawn(config);
        assert!(result.is_ok(), "cannot spawn Common Lisp test worker");
        let Some(mut client) = result.ok() else {
            return;
        };
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
            wait_event(&client, Duration::from_secs(6)),
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
                      printf '(:response :version 1 :id 1 :status :ok :value (:answer 42))\\n'";
        let Some(mut client) = spawn_test_client(shell_worker(script)) else {
            return;
        };
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
        let Some(mut client) = spawn_test_client(shell_worker(
            "printf '(:ready :version 2)\\n'; IFS= read -r rest",
        )) else {
            return;
        };
        assert!(matches!(
            wait_event(&client, Duration::from_secs(1)),
            Some(PolicyEvent::Unavailable(WorkerFailure::InvalidReady(_)))
        ));
        assert_eq!(
            client.try_submit("test", Value::Nil),
            Ok(PolicySubmit::Unavailable)
        );
    }

    #[test]
    fn unresponsive_request_is_killed_at_its_deadline() {
        let Some(mut client) = spawn_test_client(shell_worker(
            "printf '(:ready :version 1)\\n'; IFS= read -r request; IFS= read -r rest",
        )) else {
            return;
        };
        assert!(matches!(
            client.try_submit("test", Value::Nil),
            Ok(PolicySubmit::Queued(_))
        ));
        assert!(matches!(
            wait_event(&client, Duration::from_secs(1)),
            Some(PolicyEvent::Unavailable(WorkerFailure::RequestTimeout(_)))
        ));
    }

    #[test]
    fn one_worker_accepts_exactly_one_request() {
        let Some(mut client) = spawn_test_client(shell_worker(
            "printf '(:ready :version 1)\\n'; IFS= read -r request; IFS= read -r rest",
        )) else {
            return;
        };
        assert!(matches!(
            client.try_submit("one", Value::Nil),
            Ok(PolicySubmit::Queued(_))
        ));
        assert_eq!(
            client.try_submit("two", Value::Nil),
            Ok(PolicySubmit::Unavailable)
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
