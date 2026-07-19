//! Fixed-capacity TCP runtime and installed uploader assembly.

use std::{
    fmt, io,
    io::{BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    address::{AddressError, ServiceAddress},
    application::{Application, ApplicationError},
    auth::AuthManager,
    http::{
        BAD_REQUEST, EXPECTATION_FAILED, HEADER_FIELDS_TOO_LARGE, INTERNAL_SERVER_ERROR,
        PAYLOAD_TOO_LARGE, REQUEST_TIMEOUT, RequestReadError, Response, SERVICE_UNAVAILABLE,
        read_request_body, read_request_head,
    },
    palette::{PaletteStore, PaletteStoreError},
    password::{PasswordConfig, PasswordError},
    process::CommandRestarter,
    store::{RomStore, StoreError},
};

const WORKERS: usize = 4;
const QUEUED_CONNECTIONS: usize = 8;
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
const BODY_TIMEOUT: Duration = Duration::from_secs(35);
const WRITE_TIMEOUT: Duration = Duration::from_secs(35);
const OVERLOAD_WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const DASHBOARD_RESTART_TIMEOUT: Duration = Duration::from_secs(20);

const PASSWORD_CONFIG: &str = "/mnt/data/nes-deck/uploader/password.conf";
const ADDRESS_CONFIG: &str = "/mnt/data/nes-deck/uploader/address.conf";
const ROM_ROOT: &str = "/mnt/data/roms";
const BASE_CATALOG: &str = "/mnt/data/nes-deck/menu/games.tsv";
const UPLOAD_CATALOG: &str = "/mnt/data/nes-deck/uploads/games.tsv";
const ACTIVE_PALETTE: &str = "/mnt/data/nes-deck/state/palette.tsv";
const FALLBACK_PALETTE: &str = "/mnt/data/nes-deck/menu/palette.tsv";
const PALETTE_OVERRIDE: &str = "/mnt/data/nes-deck/state/dashboard-palette.sexp";
const DASHBOARD_SERVICE: &str = "/etc/init.d/nes-deck";

/// Load installed configuration and serve on every IPv4 interface.
///
/// # Errors
///
/// Returns [`RuntimeError`] for configuration, storage, process-control,
/// listener, worker, or accept failures.
pub fn serve_installed() -> Result<(), RuntimeError> {
    let password = PasswordConfig::load(Path::new(PASSWORD_CONFIG))?;
    let address = ServiceAddress::load(Path::new(ADDRESS_CONFIG))?;
    let roms = RomStore::new(ROM_ROOT, BASE_CATALOG, UPLOAD_CATALOG)?;
    let palette = PaletteStore::new(ACTIVE_PALETTE, FALLBACK_PALETTE, PALETTE_OVERRIDE)?;
    let restarter = CommandRestarter::new(DASHBOARD_SERVICE, DASHBOARD_RESTART_TIMEOUT)?;
    let application = Application::new(
        AuthManager::new(password),
        roms,
        palette,
        Box::new(restarter),
    );
    serve(application, address).map_err(RuntimeError::Io)
}

/// Run a fixed worker pool around an already assembled application.
///
/// # Errors
///
/// Returns an I/O error if binding, accepting, or all worker delivery fails.
pub fn serve(application: Application, address: ServiceAddress) -> io::Result<()> {
    let listener = TcpListener::bind(address.socket_addr())?;
    let application = Arc::new(application);
    let (sender, receiver) = mpsc::sync_channel(QUEUED_CONNECTIONS);
    let receiver = Arc::new(Mutex::new(receiver));
    let workers = spawn_workers(&application, &receiver)?;
    eprintln!("rom-uploader: listening at {address} on all IPv4 interfaces");
    let result = accept_connections(&listener, &sender);
    drop(sender);
    drop(receiver);
    for worker in workers {
        if worker.join().is_err() {
            eprintln!("rom-uploader: worker panicked during shutdown");
        }
    }
    result
}

fn spawn_workers(
    application: &Arc<Application>,
    receiver: &Arc<Mutex<Receiver<TcpStream>>>,
) -> io::Result<Vec<thread::JoinHandle<()>>> {
    let mut workers = Vec::with_capacity(WORKERS);
    for index in 0..WORKERS {
        let application = Arc::clone(application);
        let receiver = Arc::clone(receiver);
        let worker = thread::Builder::new()
            .name(format!("uploader-{index}"))
            .spawn(move || worker_loop(&application, &receiver))?;
        workers.push(worker);
    }
    Ok(workers)
}

fn worker_loop(application: &Application, receiver: &Mutex<Receiver<TcpStream>>) {
    loop {
        let stream = {
            let Ok(receiver) = receiver.lock() else {
                eprintln!("rom-uploader: connection queue lock was poisoned");
                return;
            };
            match receiver.recv() {
                Ok(stream) => stream,
                Err(_) => return,
            }
        };
        if let Err(error) = serve_connection(application, stream) {
            eprintln!("rom-uploader: connection failed: {error}");
        }
    }
}

fn accept_connections(listener: &TcpListener, sender: &SyncSender<TcpStream>) -> io::Result<()> {
    loop {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = stream.set_nodelay(true) {
            eprintln!("rom-uploader: cannot configure connection: {error}");
            continue;
        }
        match sender.try_send(stream) {
            Ok(()) => {}
            Err(TrySendError::Full(mut stream)) => {
                if stream
                    .set_write_timeout(Some(OVERLOAD_WRITE_TIMEOUT))
                    .is_ok()
                {
                    let mut response = Response::text(
                        SERVICE_UNAVAILABLE,
                        "The uploader is busy. Try again in a moment.",
                    );
                    let _header_result = response.add_header("Retry-After", "1");
                    let _write_result = response.hardened().write_to(&mut stream);
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "all uploader workers stopped",
                ));
            }
        }
    }
}

fn serve_connection(application: &Application, mut stream: TcpStream) -> io::Result<()> {
    let source = stream.peer_addr()?.ip();
    let transport = DeadlineStream::new(&mut stream, HEADER_TIMEOUT)?;
    let mut reader = BufReader::with_capacity(4_096, transport);
    let head = match read_request_head(&mut reader) {
        Ok(head) => head,
        Err(error) => {
            reader.get_mut().set_write_deadline(WRITE_TIMEOUT)?;
            return response_for_read_error(&error).write_to(reader.get_mut());
        }
    };
    reader.get_mut().set_read_deadline(BODY_TIMEOUT)?;
    let maximum = application.maximum_body_bytes(&head);
    let request = match read_request_body(&mut reader, head, maximum) {
        Ok(request) => request,
        Err(error) => {
            reader.get_mut().set_write_deadline(WRITE_TIMEOUT)?;
            return response_for_read_error(&error).write_to(reader.get_mut());
        }
    };
    let response = match application.handle(request, source, Instant::now()) {
        Ok(response) => response,
        Err(error) => {
            log_application_error(source, &error);
            Response::text(INTERNAL_SERVER_ERROR, "Internal server error").hardened()
        }
    };
    reader.get_mut().set_write_deadline(WRITE_TIMEOUT)?;
    response.write_to(reader.get_mut())
}

struct DeadlineStream<'a> {
    stream: &'a mut TcpStream,
    read_deadline: Instant,
    write_deadline: Instant,
}

impl<'a> DeadlineStream<'a> {
    fn new(stream: &'a mut TcpStream, read_timeout: Duration) -> io::Result<Self> {
        let now = Instant::now();
        Ok(Self {
            stream,
            read_deadline: deadline(now, read_timeout)?,
            write_deadline: now,
        })
    }

    fn set_read_deadline(&mut self, timeout: Duration) -> io::Result<()> {
        self.read_deadline = deadline(Instant::now(), timeout)?;
        Ok(())
    }

    fn set_write_deadline(&mut self, timeout: Duration) -> io::Result<()> {
        self.write_deadline = deadline(Instant::now(), timeout)?;
        Ok(())
    }

    fn remaining_read(&self) -> io::Result<Duration> {
        remaining(self.read_deadline)
    }

    fn remaining_write(&self) -> io::Result<Duration> {
        remaining(self.write_deadline)
    }
}

impl Read for DeadlineStream<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining_read()?))?;
        self.stream.read(buffer)
    }
}

impl Write for DeadlineStream<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream
            .set_write_timeout(Some(self.remaining_write()?))?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream
            .set_write_timeout(Some(self.remaining_write()?))?;
        self.stream.flush()
    }
}

fn deadline(now: Instant, timeout: Duration) -> io::Result<Instant> {
    now.checked_add(timeout)
        .ok_or_else(|| io::Error::other("socket deadline overflow"))
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    match deadline.checked_duration_since(Instant::now()) {
        Some(remaining) if !remaining.is_zero() => Ok(remaining),
        Some(_) | None => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "absolute socket deadline expired",
        )),
    }
}

fn response_for_read_error(error: &RequestReadError) -> Response {
    let (status, message) = match error {
        RequestReadError::TimedOut => (REQUEST_TIMEOUT, "Request timed out"),
        RequestReadError::HeaderTooLarge | RequestReadError::TooManyHeaders => {
            (HEADER_FIELDS_TOO_LARGE, "Request headers are too large")
        }
        RequestReadError::UnsupportedExpectation => {
            (EXPECTATION_FAILED, "Request expectation is unsupported")
        }
        RequestReadError::BodyTooLarge { .. } => (PAYLOAD_TOO_LARGE, "Request body is too large"),
        RequestReadError::Allocation | RequestReadError::Io(_) => {
            (INTERNAL_SERVER_ERROR, "Internal server error")
        }
        RequestReadError::UnexpectedEof
        | RequestReadError::Malformed
        | RequestReadError::InvalidTarget
        | RequestReadError::DuplicateHeader
        | RequestReadError::UnsupportedTransferCoding
        | RequestReadError::UnsupportedContentCoding
        | RequestReadError::InvalidContentLength => (BAD_REQUEST, "Malformed request"),
    };
    Response::text(status, message).hardened()
}

fn log_application_error(source: std::net::IpAddr, error: &ApplicationError) {
    eprintln!("rom-uploader: request from {source} failed internally: {error}");
}

/// Installed configuration or server-runtime failure.
#[derive(Debug)]
pub enum RuntimeError {
    /// Password record could not be loaded or validated.
    Password(PasswordError),
    /// Listener address could not be loaded or validated.
    Address(AddressError),
    /// ROM store paths are invalid.
    Store(StoreError),
    /// Palette store paths are invalid.
    Palette(PaletteStoreError),
    /// Process control, binding, worker creation, or accepting failed.
    Io(io::Error),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password(error) => write!(formatter, "load uploader password: {error}"),
            Self::Address(error) => write!(formatter, "load uploader address: {error}"),
            Self::Store(error) => write!(formatter, "configure ROM store: {error}"),
            Self::Palette(error) => write!(formatter, "configure palette store: {error}"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Password(error) => Some(error),
            Self::Address(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Palette(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

impl From<PasswordError> for RuntimeError {
    fn from(error: PasswordError) -> Self {
        Self::Password(error)
    }
}

impl From<AddressError> for RuntimeError {
    fn from(error: AddressError) -> Self {
        Self::Address(error)
    }
}

impl From<StoreError> for RuntimeError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<PaletteStoreError> for RuntimeError {
    fn from(error: PaletteStoreError) -> Self {
        Self::Palette(error)
    }
}

impl From<io::Error> for RuntimeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read as _, Write as _},
        net::{Shutdown, TcpStream},
    };

    use crate::application::DashboardRestarter;

    use super::*;

    struct NoRestart;

    impl DashboardRestarter for NoRestart {
        fn restart(&self) -> io::Result<()> {
            Ok(())
        }
    }

    fn application(directory: &tempfile::TempDir) -> Option<Application> {
        let base_catalog = directory.path().join("base.tsv");
        let fallback_palette = directory.path().join("fallback.tsv");
        let active_palette = directory.path().join("active.tsv");
        fs::write(&base_catalog, b"").ok()?;
        fs::write(
            &fallback_palette,
            include_bytes!("../../../deploy/menu/palette.tsv"),
        )
        .ok()?;
        fs::write(
            &active_palette,
            include_bytes!("../../../deploy/menu/palette.tsv"),
        )
        .ok()?;
        Some(Application::new(
            AuthManager::new(PasswordConfig::new("configured-password").ok()?),
            RomStore::new(
                directory.path().join("roms"),
                base_catalog,
                directory.path().join("uploads.tsv"),
            )
            .ok()?,
            PaletteStore::new(
                active_palette,
                fallback_palette,
                directory.path().join("override.sexp"),
            )
            .ok()?,
            Box::new(NoRestart),
        ))
    }

    #[test]
    fn protocol_failures_receive_specific_hardened_statuses() {
        for (error, expected) in [
            (RequestReadError::TimedOut, REQUEST_TIMEOUT),
            (RequestReadError::HeaderTooLarge, HEADER_FIELDS_TOO_LARGE),
            (RequestReadError::UnsupportedExpectation, EXPECTATION_FAILED),
            (
                RequestReadError::BodyTooLarge { maximum: 10 },
                PAYLOAD_TOO_LARGE,
            ),
            (RequestReadError::Malformed, BAD_REQUEST),
            (RequestReadError::Allocation, INTERNAL_SERVER_ERROR),
        ] {
            let response = response_for_read_error(&error);
            assert_eq!(response.status(), expected);
            assert_eq!(response.header("Cache-Control"), Some("no-store"));
            assert_eq!(response.header("X-Frame-Options"), Some("DENY"));
        }
    }

    #[test]
    fn serves_a_real_tcp_request_through_the_application() {
        let directory = tempfile::tempdir();
        assert!(directory.is_ok());
        let Some(directory) = directory.ok() else {
            return;
        };
        let Some(application) = application(&directory) else {
            return;
        };
        let listener = TcpListener::bind("127.0.0.1:0");
        assert!(listener.is_ok());
        let Some(listener) = listener.ok() else {
            return;
        };
        let address = listener.local_addr();
        assert!(address.is_ok());
        let Some(address) = address.ok() else {
            return;
        };
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept()?;
            serve_connection(&application, stream)
        });
        let client = TcpStream::connect(address);
        assert!(client.is_ok());
        let Some(mut client) = client.ok() else {
            return;
        };
        assert!(
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .is_ok()
        );
        assert!(
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nContent-Length: 0\r\n\r\n")
                .is_ok()
        );
        assert!(client.shutdown(Shutdown::Write).is_ok());
        let mut response = String::new();
        assert!(client.read_to_string(&mut response).is_ok());
        assert!(matches!(server.join(), Ok(Ok(()))));
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("X-Frame-Options: DENY\r\n"));
        assert!(response.contains("name=\"password\""));
    }

    #[test]
    fn absolute_deadline_expires_even_without_a_socket_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0");
        assert!(listener.is_ok());
        let Some(listener) = listener.ok() else {
            return;
        };
        let address = listener.local_addr();
        assert!(address.is_ok());
        let Some(address) = address.ok() else {
            return;
        };
        let peer = TcpStream::connect(address);
        assert!(peer.is_ok());
        let Some(_peer) = peer.ok() else {
            return;
        };
        let accepted = listener.accept();
        assert!(accepted.is_ok());
        let Some((mut stream, _)) = accepted.ok() else {
            return;
        };
        let deadline = DeadlineStream::new(&mut stream, Duration::from_millis(5));
        assert!(deadline.is_ok());
        let Some(mut deadline) = deadline.ok() else {
            return;
        };
        thread::sleep(Duration::from_millis(10));
        let mut byte = [0_u8; 1];
        assert!(matches!(
            deadline.read(&mut byte),
            Err(error) if error.kind() == io::ErrorKind::TimedOut
        ));
    }
}
