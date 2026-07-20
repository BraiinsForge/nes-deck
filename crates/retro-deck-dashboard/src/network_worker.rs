//! Periodic read-only network snapshots kept off the dashboard input thread.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use retro_deck_platform::file::read_regular_bounded;
use retro_deck_platform::network::{NetworkInterfaces, NetworkReadError};

use crate::NetworkView;

const UPDATE_CAPACITY: usize = 1;
const STOP_CAPACITY: usize = 1;
const MAXIMUM_SELECTOR_BYTES: usize = 128;
const MAXIMUM_SELECTOR_CHARACTERS: usize = 64;
const MINIMUM_REFRESH: Duration = Duration::from_millis(250);
const MAXIMUM_REFRESH: Duration = Duration::from_secs(60);
const WORKER_NAME: &str = "retro-deck-network-status";

/// Fixed read-only sources and cadence for network status collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkStatusConfig {
    wireless: String,
    wireguard: String,
    selector_status: PathBuf,
    refresh: Duration,
}

impl NetworkStatusConfig {
    /// Validate an absolute selector path and bounded refresh interval.
    ///
    /// # Errors
    ///
    /// Returns [`NetworkStatusConfigError`] for a relative path or interval
    /// outside 250 milliseconds through 60 seconds.
    pub fn new(
        wireless: impl Into<String>,
        wireguard: impl Into<String>,
        selector_status: impl Into<PathBuf>,
        refresh: Duration,
    ) -> Result<Self, NetworkStatusConfigError> {
        let config = Self {
            wireless: wireless.into(),
            wireguard: wireguard.into(),
            selector_status: selector_status.into(),
            refresh,
        };
        if !config.selector_status.is_absolute() {
            return Err(NetworkStatusConfigError::RelativeSelector(
                config.selector_status,
            ));
        }
        if !(MINIMUM_REFRESH..=MAXIMUM_REFRESH).contains(&config.refresh) {
            return Err(NetworkStatusConfigError::Refresh(config.refresh));
        }
        Ok(config)
    }
}

/// Invalid read-only network status configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkStatusConfigError {
    /// Selector status path is relative.
    RelativeSelector(PathBuf),
    /// Refresh cadence is outside the bounded contract.
    Refresh(Duration),
}

impl fmt::Display for NetworkStatusConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativeSelector(path) => write!(
                formatter,
                "network selector status path is not absolute: {}",
                path.display()
            ),
            Self::Refresh(refresh) => write!(
                formatter,
                "network status refresh {refresh:?} is outside 250ms through 60s"
            ),
        }
    }
}

impl Error for NetworkStatusConfigError {}

/// Owned, bounded status displayed by settings and the Wi-Fi editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkStatus {
    ssid: String,
    wireless_ipv4: String,
    wireguard_ipv4: String,
    selector: String,
}

impl NetworkStatus {
    /// Safe startup value before the first worker snapshot arrives.
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            ssid: String::new(),
            wireless_ipv4: String::new(),
            wireguard_ipv4: String::new(),
            selector: "STATUS UNAVAILABLE".to_owned(),
        }
    }

    /// Borrow the renderer's immutable network view.
    #[must_use]
    pub fn view(&self) -> NetworkView<'_> {
        NetworkView::new(
            self.ssid.as_str(),
            self.wireless_ipv4.as_str(),
            self.wireguard_ipv4.as_str(),
            self.selector.as_str(),
        )
    }

    fn collect(config: &NetworkStatusConfig) -> Result<Self, NetworkStatusError> {
        let interfaces = NetworkInterfaces::read(&config.wireless, &config.wireguard)
            .map_err(NetworkStatusError::Interfaces)?;
        Ok(Self {
            ssid: interfaces.ssid().to_owned(),
            wireless_ipv4: interfaces
                .wireless_ipv4()
                .map(|address| address.to_string())
                .unwrap_or_default(),
            wireguard_ipv4: interfaces
                .wireguard_ipv4()
                .map(|address| address.to_string())
                .unwrap_or_default(),
            selector: selector_status(&config.selector_status),
        })
    }
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self::unavailable()
    }
}

/// One read-only snapshot collection failure.
#[derive(Debug)]
pub enum NetworkStatusError {
    /// Linux interface enumeration failed or fixed names were invalid.
    Interfaces(NetworkReadError),
}

impl fmt::Display for NetworkStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interfaces(error) => write!(formatter, "cannot refresh network status: {error}"),
        }
    }
}

impl Error for NetworkStatusError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Interfaces(error) => Some(error),
        }
    }
}

/// One nonblocking worker poll.
#[derive(Debug)]
pub enum NetworkStatusPoll {
    /// A complete new snapshot is available.
    Updated(NetworkStatus),
    /// The latest collection attempt failed; the caller should keep its prior
    /// snapshot.
    Failed(NetworkStatusError),
    /// No update is currently available.
    Empty,
    /// The worker ended and no further update can arrive.
    Disconnected,
}

/// Input-side handle for the periodic read-only status worker.
#[derive(Debug)]
pub struct NetworkStatusWorker {
    stop: Option<SyncSender<()>>,
    updates: Receiver<Result<NetworkStatus, NetworkStatusError>>,
    thread: Option<JoinHandle<NetworkStatusWorkerReport>>,
}

impl NetworkStatusWorker {
    /// Start collection and return without waiting for the first query.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error only when the named thread cannot be
    /// created.
    pub fn spawn(config: NetworkStatusConfig) -> std::io::Result<Self> {
        let (stop, stop_receiver) = mpsc::sync_channel(STOP_CAPACITY);
        let (update_sender, updates) = mpsc::sync_channel(UPDATE_CAPACITY);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || run_worker(&config, &stop_receiver, &update_sender))?;
        Ok(Self {
            stop: Some(stop),
            updates,
            thread: Some(thread),
        })
    }

    /// Poll one snapshot or error without waiting.
    #[must_use]
    pub fn try_update(&self) -> NetworkStatusPoll {
        match self.updates.try_recv() {
            Ok(Ok(status)) => NetworkStatusPoll::Updated(status),
            Ok(Err(error)) => NetworkStatusPoll::Failed(error),
            Err(TryRecvError::Empty) => NetworkStatusPoll::Empty,
            Err(TryRecvError::Disconnected) => NetworkStatusPoll::Disconnected,
        }
    }

    /// Stop collection and return final counts.
    #[must_use]
    pub fn shutdown(mut self) -> NetworkStatusWorkerReport {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> NetworkStatusWorkerReport {
        if let Some(stop) = self.stop.take() {
            let _ = stop.try_send(());
        }
        let Some(thread) = self.thread.take() else {
            return NetworkStatusWorkerReport::default();
        };
        match thread.join() {
            Ok(report) => report,
            Err(_) => NetworkStatusWorkerReport {
                panicked: true,
                ..NetworkStatusWorkerReport::default()
            },
        }
    }
}

impl Drop for NetworkStatusWorker {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

/// Final read-only network worker diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetworkStatusWorkerReport {
    /// Successful kernel snapshots.
    pub snapshots: u64,
    /// Failed kernel snapshots.
    pub failures: u64,
    /// Updates omitted because the caller had not consumed the previous one.
    pub dropped_updates: u64,
    /// Whether the worker thread panicked.
    pub panicked: bool,
}

fn run_worker(
    config: &NetworkStatusConfig,
    stop: &Receiver<()>,
    updates: &SyncSender<Result<NetworkStatus, NetworkStatusError>>,
) -> NetworkStatusWorkerReport {
    let mut report = NetworkStatusWorkerReport::default();
    loop {
        let update = NetworkStatus::collect(config);
        match &update {
            Ok(_) => report.snapshots = report.snapshots.saturating_add(1),
            Err(_) => report.failures = report.failures.saturating_add(1),
        }
        match updates.try_send(update) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                report.dropped_updates = report.dropped_updates.saturating_add(1);
            }
        }
        match stop.recv_timeout(config.refresh) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return report,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn selector_status(path: &Path) -> String {
    match read_regular_bounded(path, MAXIMUM_SELECTOR_BYTES) {
        Ok(bytes) => parse_selector_status(&bytes).unwrap_or_else(|| "STATUS INVALID".to_owned()),
        Err(_) => "STATUS UNAVAILABLE".to_owned(),
    }
}

fn parse_selector_status(bytes: &[u8]) -> Option<String> {
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty() || line.contains(&b'\n') || line.contains(&b'\r') {
        return None;
    }
    let text = std::str::from_utf8(line).ok()?;
    if text.chars().count() > MAXIMUM_SELECTOR_CHARACTERS
        || text.chars().any(char::is_control)
        || text.trim_matches(char::is_whitespace) != text
    {
        return None;
    }
    let mut output = String::with_capacity(line.len().min(MAXIMUM_SELECTOR_CHARACTERS));
    for character in text.chars() {
        output.push(if character.is_ascii() { character } else { '?' });
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use super::{
        NetworkStatusConfig, NetworkStatusConfigError, NetworkStatusPoll, NetworkStatusWorker,
        parse_selector_status, selector_status,
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "retro-deck-network-status-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("network status fixture is created");
            Self(path)
        }

        fn status(&self) -> std::path::PathBuf {
            self.0.join("status")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn selector_status_is_one_bounded_case_preserving_line() {
        assert_eq!(
            parse_selector_status(b"CONNECTED\n").as_deref(),
            Some("CONNECTED")
        );
        assert_eq!(
            parse_selector_status(b"Trying MixedCase\r\n").as_deref(),
            Some("Trying MixedCase")
        );
        assert_eq!(
            parse_selector_status("Síť ready".as_bytes()).as_deref(),
            Some("S?? ready")
        );
        for invalid in [b"".as_slice(), b" padded ", b"two\nlines\n", b"bad\0text"] {
            assert!(parse_selector_status(invalid).is_none());
        }
    }

    #[test]
    fn missing_and_malformed_selector_files_fail_visibly() {
        let fixture = Fixture::new();
        assert_eq!(selector_status(&fixture.status()), "STATUS UNAVAILABLE");
        fs::write(fixture.status(), b" bad \n").expect("invalid selector fixture is written");
        assert_eq!(selector_status(&fixture.status()), "STATUS INVALID");
    }

    #[test]
    fn worker_publishes_complete_snapshots_without_blocking_pollers() {
        let fixture = Fixture::new();
        fs::write(fixture.status(), b"CONNECTED\n").expect("selector fixture is written");
        let config = NetworkStatusConfig::new("lo", "lo", fixture.status(), Duration::from_secs(1))
            .expect("network status config is valid");
        let worker = NetworkStatusWorker::spawn(config).expect("network status worker starts");
        let deadline = Instant::now() + Duration::from_secs(1);
        let status = loop {
            match worker.try_update() {
                NetworkStatusPoll::Updated(status) => break Some(status),
                NetworkStatusPoll::Empty if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                NetworkStatusPoll::Empty
                | NetworkStatusPoll::Failed(_)
                | NetworkStatusPoll::Disconnected => break None,
            }
        };
        let Some(status) = status else {
            return;
        };
        assert_eq!(status.view().wlan_ipv4(), "127.0.0.1");
        assert_eq!(status.view().wireguard_ipv4(), "127.0.0.1");
        assert_eq!(status.view().selector(), "CONNECTED");
        let report = worker.shutdown();
        assert!(report.snapshots >= 1);
        assert_eq!(report.failures, 0);
        assert!(!report.panicked);
    }

    #[test]
    fn configuration_rejects_relative_paths_and_runaway_polling() {
        assert!(matches!(
            NetworkStatusConfig::new("wlan0", "wg0", "relative", Duration::from_secs(2)),
            Err(NetworkStatusConfigError::RelativeSelector(_))
        ));
        assert!(matches!(
            NetworkStatusConfig::new("wlan0", "wg0", "/status", Duration::from_millis(1)),
            Err(NetworkStatusConfigError::Refresh(_))
        ));
    }
}
