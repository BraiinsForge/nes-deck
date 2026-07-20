//! Coalescing dashboard preference persistence off the input thread.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};

use retro_deck_platform::file::{
    AtomicWriteError, DeviceFileError, read_device_bounded, write_device_bounded,
    write_private_atomic,
};

use crate::{
    Brightness, DashboardPreferences, Keymap, MAXIMUM_PREFERENCE_BYTES, PreferenceField,
    PreferencePaths, PreferenceValueError, SettingChange, encode_setting,
};

const DIRTY_VOLUME: u8 = 1 << 0;
const DIRTY_BRIGHTNESS: u8 = 1 << 1;
const DIRTY_KEYMAP: u8 = 1 << 2;
const DIRTY_ALL: u8 = DIRTY_VOLUME | DIRTY_BRIGHTNESS | DIRTY_KEYMAP;
const WAKE_CAPACITY: usize = 1;
const ERROR_CAPACITY: usize = 8;
const DEVICE_INTEGER_BYTES: usize = 32;
const DEVICE_OUTPUT_BYTES: usize = 11;
const WORKER_NAME: &str = "retro-deck-preferences";

/// Absolute sysfs-style brightness value and maximum paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrightnessDevicePaths {
    value: PathBuf,
    maximum: PathBuf,
}

impl BrightnessDevicePaths {
    /// Validate two distinct absolute device paths without opening them.
    ///
    /// # Errors
    ///
    /// Returns [`BrightnessPathError`] for a relative or repeated path.
    pub fn new(
        value: impl Into<PathBuf>,
        maximum: impl Into<PathBuf>,
    ) -> Result<Self, BrightnessPathError> {
        let paths = Self {
            value: value.into(),
            maximum: maximum.into(),
        };
        if !paths.value.is_absolute() {
            return Err(BrightnessPathError::Relative(paths.value));
        }
        if !paths.maximum.is_absolute() {
            return Err(BrightnessPathError::Relative(paths.maximum));
        }
        if paths.value == paths.maximum {
            return Err(BrightnessPathError::Duplicate);
        }
        Ok(paths)
    }

    /// Writable brightness attribute.
    #[must_use]
    pub fn value(&self) -> &Path {
        self.value.as_path()
    }

    /// Read-only raw maximum attribute.
    #[must_use]
    pub fn maximum(&self) -> &Path {
        self.maximum.as_path()
    }
}

/// Brightness paths violate the fixed device contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrightnessPathError {
    /// One path is relative.
    Relative(PathBuf),
    /// The value and maximum paths are identical.
    Duplicate,
}

impl fmt::Display for BrightnessPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative(path) => {
                write!(
                    formatter,
                    "brightness device path is not absolute: {}",
                    path.display()
                )
            }
            Self::Duplicate => {
                formatter.write_str("brightness value and maximum paths must be distinct")
            }
        }
    }
}

impl Error for BrightnessPathError {}

/// Nonblocking outcome of publishing one latest preference value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreferenceSubmit {
    /// The value became the pending value for its field.
    Accepted,
    /// A pending value for the same field was replaced with this newer value.
    Coalesced,
    /// The setting itself was outside its typed schema.
    Invalid,
    /// The worker has stopped and cannot persist the value.
    Disconnected,
}

/// Input-side handle for one coalescing persistence thread.
///
/// Publishing uses only atomics and a bounded `try_send`; no file or device
/// operation can run on the caller's input path.
#[derive(Debug)]
pub struct PreferenceWorker {
    controls: Arc<PreferenceControls>,
    wake: Option<SyncSender<()>>,
    errors: Receiver<PreferenceWorkerError>,
    thread: Option<JoinHandle<PreferenceWorkerReport>>,
}

impl PreferenceWorker {
    /// Start a worker and asynchronously materialize all startup values.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error only when the named thread cannot be
    /// created. File and device failures are nonfatal diagnostics.
    pub fn spawn(
        state_paths: PreferencePaths,
        brightness_paths: BrightnessDevicePaths,
        initial: DashboardPreferences,
    ) -> std::io::Result<Self> {
        let controls = Arc::new(PreferenceControls::new(initial));
        let worker_controls = Arc::clone(&controls);
        let (wake, wake_receiver) = mpsc::sync_channel(WAKE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(ERROR_CAPACITY);
        let thread = thread::Builder::new()
            .name(WORKER_NAME.to_owned())
            .spawn(move || {
                run_worker(
                    &wake_receiver,
                    worker_controls.as_ref(),
                    &error_sender,
                    FilePreferenceBackend::new(state_paths, brightness_paths),
                )
            })?;
        let _ = wake.try_send(());
        Ok(Self {
            controls,
            wake: Some(wake),
            errors,
            thread: Some(thread),
        })
    }

    /// Publish the newest value without waiting for persistence.
    #[must_use]
    pub fn try_submit(&self, setting: SettingChange) -> PreferenceSubmit {
        if self.controls.shutdown.load(Ordering::Acquire) {
            return PreferenceSubmit::Disconnected;
        }
        let Ok(encoded) = encode_setting(setting) else {
            return PreferenceSubmit::Invalid;
        };
        let bit = match encoded.field() {
            PreferenceField::Volume => {
                let SettingChange::Volume(value) = setting else {
                    return PreferenceSubmit::Invalid;
                };
                self.controls.volume.store(value, Ordering::Release);
                DIRTY_VOLUME
            }
            PreferenceField::Brightness => {
                let SettingChange::Brightness(value) = setting else {
                    return PreferenceSubmit::Invalid;
                };
                self.controls.brightness.store(value, Ordering::Release);
                DIRTY_BRIGHTNESS
            }
            PreferenceField::Keymap => {
                let SettingChange::Keymap(value) = setting else {
                    return PreferenceSubmit::Invalid;
                };
                self.controls
                    .keymap
                    .store(keymap_code(value), Ordering::Release);
                DIRTY_KEYMAP
            }
        };
        let previous = self.controls.dirty.fetch_or(bit, Ordering::AcqRel);
        let Some(wake) = &self.wake else {
            return PreferenceSubmit::Disconnected;
        };
        match wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {
                if previous & bit == 0 {
                    PreferenceSubmit::Accepted
                } else {
                    PreferenceSubmit::Coalesced
                }
            }
            Err(TrySendError::Disconnected(())) => PreferenceSubmit::Disconnected,
        }
    }

    /// Drain currently reported persistence failures without waiting.
    #[must_use]
    pub fn take_errors(&self) -> Vec<PreferenceWorkerError> {
        let mut errors = Vec::new();
        loop {
            match self.errors.try_recv() {
                Ok(error) => errors.push(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return errors,
            }
        }
    }

    /// Flush the latest values, stop the worker, and return final counts.
    #[must_use]
    pub fn shutdown(mut self) -> PreferenceWorkerReport {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> PreferenceWorkerReport {
        self.controls.shutdown.store(true, Ordering::Release);
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
        let _ = self.wake.take();
        let Some(thread) = self.thread.take() else {
            return PreferenceWorkerReport::default();
        };
        match thread.join() {
            Ok(report) => report,
            Err(_) => PreferenceWorkerReport {
                panicked: true,
                ..PreferenceWorkerReport::default()
            },
        }
    }
}

impl Drop for PreferenceWorker {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

#[derive(Debug)]
struct PreferenceControls {
    volume: AtomicU8,
    brightness: AtomicU8,
    keymap: AtomicU8,
    dirty: AtomicU8,
    shutdown: AtomicBool,
}

impl PreferenceControls {
    const fn new(initial: DashboardPreferences) -> Self {
        Self {
            volume: AtomicU8::new(initial.volume().percent()),
            brightness: AtomicU8::new(initial.brightness().percent()),
            keymap: AtomicU8::new(keymap_code(initial.keymap())),
            dirty: AtomicU8::new(DIRTY_ALL),
            shutdown: AtomicBool::new(false),
        }
    }

    fn setting(&self, field: PreferenceField) -> SettingChange {
        match field {
            PreferenceField::Volume => SettingChange::Volume(self.volume.load(Ordering::Acquire)),
            PreferenceField::Brightness => {
                SettingChange::Brightness(self.brightness.load(Ordering::Acquire))
            }
            PreferenceField::Keymap => {
                SettingChange::Keymap(keymap_from_code(self.keymap.load(Ordering::Acquire)))
            }
        }
    }
}

const fn keymap_code(keymap: Keymap) -> u8 {
    match keymap {
        Keymap::Us => 0,
        Keymap::Czech => 1,
    }
}

const fn keymap_from_code(code: u8) -> Keymap {
    if code == 1 { Keymap::Czech } else { Keymap::Us }
}

trait PreferenceBackend {
    fn persist(&mut self, setting: SettingChange) -> Result<(), PreferenceWriteError>;
}

#[derive(Debug)]
struct FilePreferenceBackend {
    state_paths: PreferencePaths,
    brightness_paths: BrightnessDevicePaths,
    brightness_maximum: Option<u32>,
}

impl FilePreferenceBackend {
    const fn new(state_paths: PreferencePaths, brightness_paths: BrightnessDevicePaths) -> Self {
        Self {
            state_paths,
            brightness_paths,
            brightness_maximum: None,
        }
    }

    fn persist_brightness(&mut self, percent: u8) -> Result<(), PreferenceWriteError> {
        let brightness = Brightness::new(percent)
            .map_err(|_| PreferenceWriteError::Value(PreferenceValueError::Brightness))?;
        let maximum = if let Some(maximum) = self.brightness_maximum {
            maximum
        } else {
            let bytes = read_device_bounded(self.brightness_paths.maximum(), DEVICE_INTEGER_BYTES)
                .map_err(PreferenceWriteError::Device)?;
            let maximum = parse_device_integer(&bytes)
                .filter(|maximum| *maximum != 0)
                .ok_or(PreferenceWriteError::InvalidBrightnessMaximum)?;
            self.brightness_maximum = Some(maximum);
            maximum
        };
        let raw = brightness_raw_value(brightness, maximum);
        let value = format!("{raw}\n");
        write_device_bounded(
            self.brightness_paths.value(),
            value.as_bytes(),
            DEVICE_OUTPUT_BYTES,
        )
        .map_err(PreferenceWriteError::Device)?;
        self.persist_state(SettingChange::Brightness(percent))
    }

    fn persist_state(&self, setting: SettingChange) -> Result<(), PreferenceWriteError> {
        let encoded = encode_setting(setting).map_err(PreferenceWriteError::Value)?;
        let path = match encoded.field() {
            PreferenceField::Volume => self.state_paths.volume(),
            PreferenceField::Brightness => self.state_paths.brightness(),
            PreferenceField::Keymap => self.state_paths.keymap(),
        };
        write_private_atomic(path, encoded.as_bytes(), MAXIMUM_PREFERENCE_BYTES)
            .map_err(PreferenceWriteError::State)
    }
}

impl PreferenceBackend for FilePreferenceBackend {
    fn persist(&mut self, setting: SettingChange) -> Result<(), PreferenceWriteError> {
        if let SettingChange::Brightness(percent) = setting {
            self.persist_brightness(percent)
        } else {
            self.persist_state(setting)
        }
    }
}

fn run_worker<B: PreferenceBackend>(
    wake: &Receiver<()>,
    controls: &PreferenceControls,
    errors: &SyncSender<PreferenceWorkerError>,
    mut backend: B,
) -> PreferenceWorkerReport {
    let mut report = PreferenceWorkerReport::default();
    loop {
        let disconnected = wake.recv().is_err();
        flush_pending(controls, errors, &mut backend, &mut report);
        if disconnected || controls.shutdown.load(Ordering::Acquire) {
            return report;
        }
    }
}

fn flush_pending<B: PreferenceBackend>(
    controls: &PreferenceControls,
    errors: &SyncSender<PreferenceWorkerError>,
    backend: &mut B,
    report: &mut PreferenceWorkerReport,
) {
    loop {
        let dirty = controls.dirty.swap(0, Ordering::AcqRel);
        if dirty == 0 {
            return;
        }
        for (bit, field) in [
            (DIRTY_VOLUME, PreferenceField::Volume),
            (DIRTY_BRIGHTNESS, PreferenceField::Brightness),
            (DIRTY_KEYMAP, PreferenceField::Keymap),
        ] {
            if dirty & bit == 0 {
                continue;
            }
            let setting = controls.setting(field);
            match backend.persist(setting) {
                Ok(()) => report.writes = report.writes.saturating_add(1),
                Err(source) => {
                    record_error(PreferenceWorkerError { field, source }, errors, report);
                }
            }
        }
    }
}

fn record_error(
    error: PreferenceWorkerError,
    errors: &SyncSender<PreferenceWorkerError>,
    report: &mut PreferenceWorkerReport,
) {
    report.errors = report.errors.saturating_add(1);
    match errors.try_send(error) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(_)) => {
            report.dropped_errors = report.dropped_errors.saturating_add(1);
        }
    }
}

fn parse_device_integer(bytes: &[u8]) -> Option<u32> {
    let start = bytes.iter().position(|byte| !byte.is_ascii_whitespace())?;
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())?
        .checked_add(1)?;
    let digits = bytes.get(start..end)?;
    let mut value = 0_u32;
    for digit in digits {
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(digit.saturating_sub(b'0')))?;
    }
    Some(value)
}

/// Convert a validated percentage to the nearest nonzero raw backlight value.
#[must_use]
pub fn brightness_raw_value(brightness: Brightness, maximum: u32) -> u32 {
    if maximum == 0 {
        return 0;
    }
    let scaled = (u64::from(brightness.percent()) * u64::from(maximum) + 50) / 100;
    u32::try_from(scaled).unwrap_or(maximum).clamp(1, maximum)
}

/// Nonfatal persistence failure reported by the worker.
#[derive(Debug)]
pub struct PreferenceWorkerError {
    field: PreferenceField,
    source: PreferenceWriteError,
}

impl PreferenceWorkerError {
    /// Field that could not be persisted.
    #[must_use]
    pub const fn field(&self) -> PreferenceField {
        self.field
    }
}

impl fmt::Display for PreferenceWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot persist {} preference: {}",
            self.field, self.source
        )
    }
}

impl Error for PreferenceWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Filesystem, device, or schema failure behind a worker diagnostic.
#[derive(Debug)]
pub enum PreferenceWriteError {
    /// A manually assembled setting violated its schema.
    Value(PreferenceValueError),
    /// Atomic private state replacement failed.
    State(AtomicWriteError),
    /// A bounded brightness attribute operation failed.
    Device(DeviceFileError),
    /// The maximum brightness attribute was not a positive integer.
    InvalidBrightnessMaximum,
}

impl fmt::Display for PreferenceWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => error.fmt(formatter),
            Self::State(error) => error.fmt(formatter),
            Self::Device(error) => error.fmt(formatter),
            Self::InvalidBrightnessMaximum => {
                formatter.write_str("maximum brightness is not a positive unsigned integer")
            }
        }
    }
}

impl Error for PreferenceWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Value(error) => Some(error),
            Self::State(error) => Some(error),
            Self::Device(error) => Some(error),
            Self::InvalidBrightnessMaximum => None,
        }
    }
}

/// Final persistence-thread diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreferenceWorkerReport {
    /// Successful field writes, including initialization and coalesced updates.
    pub writes: u64,
    /// Failed field writes.
    pub errors: u64,
    /// Failures omitted from the bounded diagnostic channel.
    pub dropped_errors: u64,
    /// Whether the worker thread panicked.
    pub panicked: bool,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        BrightnessDevicePaths, BrightnessPathError, PreferenceSubmit, PreferenceWorker,
        brightness_raw_value, parse_device_integer,
    };
    use crate::{Brightness, DashboardPreferences, Keymap, PreferencePaths, SettingChange};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct Fixture {
        root: std::path::PathBuf,
        state: PreferencePaths,
        device: BrightnessDevicePaths,
    }

    impl Fixture {
        fn new(with_brightness: bool) -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "retro-deck-preference-worker-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("preference worker fixture is created");
            let state = PreferencePaths::new(
                root.join("volume"),
                root.join("brightness-state"),
                root.join("keymap"),
            )
            .expect("fixture state paths are valid");
            let device = BrightnessDevicePaths::new(
                root.join("brightness-device"),
                root.join("max-brightness"),
            )
            .expect("fixture device paths are valid");
            if with_brightness {
                fs::write(device.value(), b"1\n").expect("brightness device is created");
                fs::write(device.maximum(), b"255\n").expect("brightness maximum is created");
            }
            Self {
                root,
                state,
                device,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn latest_values_survive_arbitrary_worker_coalescing() {
        let fixture = Fixture::new(true);
        let worker = PreferenceWorker::spawn(
            fixture.state.clone(),
            fixture.device.clone(),
            DashboardPreferences::default(),
        )
        .expect("preference worker starts");
        for percent in (0..=100).step_by(5) {
            assert!(matches!(
                worker.try_submit(SettingChange::Volume(percent)),
                PreferenceSubmit::Accepted | PreferenceSubmit::Coalesced
            ));
        }
        assert!(matches!(
            worker.try_submit(SettingChange::Brightness(80)),
            PreferenceSubmit::Accepted | PreferenceSubmit::Coalesced
        ));
        assert!(matches!(
            worker.try_submit(SettingChange::Keymap(Keymap::Czech)),
            PreferenceSubmit::Accepted | PreferenceSubmit::Coalesced
        ));
        let report = worker.shutdown();

        assert_eq!(report.errors, 0);
        assert!(!report.panicked);
        assert_eq!(
            fs::read(fixture.state.volume()).ok().as_deref(),
            Some(b"100\n".as_slice())
        );
        assert_eq!(
            fs::read(fixture.state.brightness()).ok().as_deref(),
            Some(b"80\n".as_slice())
        );
        assert_eq!(
            fs::read(fixture.state.keymap()).ok().as_deref(),
            Some(b"cz\n".as_slice())
        );
        assert_eq!(
            fs::read(fixture.device.value()).ok().as_deref(),
            Some(b"204\n".as_slice())
        );
    }

    #[test]
    fn broken_brightness_does_not_block_other_fields() {
        let fixture = Fixture::new(false);
        let worker = PreferenceWorker::spawn(
            fixture.state.clone(),
            fixture.device.clone(),
            DashboardPreferences::default(),
        )
        .expect("preference worker starts");
        let _ = worker.try_submit(SettingChange::Volume(25));
        let _ = worker.try_submit(SettingChange::Keymap(Keymap::Czech));
        let report = worker.shutdown();

        assert!(report.errors >= 1);
        assert_eq!(
            fs::read(fixture.state.volume()).ok().as_deref(),
            Some(b"25\n".as_slice())
        );
        assert_eq!(
            fs::read(fixture.state.keymap()).ok().as_deref(),
            Some(b"cz\n".as_slice())
        );
        assert!(!fixture.state.brightness().exists());
    }

    #[test]
    fn brightness_math_and_device_integer_schema_are_bounded() {
        let Some(low) = Brightness::new(10).ok() else {
            return;
        };
        let Some(high) = Brightness::new(100).ok() else {
            return;
        };
        assert_eq!(brightness_raw_value(low, 255), 26);
        assert_eq!(brightness_raw_value(high, 255), 255);
        assert_eq!(brightness_raw_value(low, 0), 0);
        assert_eq!(parse_device_integer(b" 255\n"), Some(255));
        assert_eq!(parse_device_integer(b"0\n"), Some(0));
        assert_eq!(parse_device_integer(b"-1\n"), None);
        assert_eq!(parse_device_integer(b"4294967296\n"), None);
    }

    #[test]
    fn brightness_paths_are_absolute_and_distinct() {
        assert!(matches!(
            BrightnessDevicePaths::new("relative", "/sys/max"),
            Err(BrightnessPathError::Relative(_))
        ));
        assert_eq!(
            BrightnessDevicePaths::new("/sys/value", "/sys/value"),
            Err(BrightnessPathError::Duplicate)
        );
    }
}
