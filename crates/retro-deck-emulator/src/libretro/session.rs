//! Ordered ownership of one statically linked libretro core session.

#![allow(
    dead_code,
    reason = "the tested session boundary is wired into the executable in the next host slice"
)]

use std::error::Error;
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::fmt;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::time::Duration;

use retro_deck_audio::SampleRate;
use retro_deck_platform::display::Dimensions;

use super::abi;
use super::callbacks::{CallbackBinding, CallbackBindingError};
use super::{
    Content, ControllerDevice, LibretroCore, MAXIMUM_SAVE_BYTES, MemoryKind, SaveError, SaveStore,
};

const MAXIMUM_FRAMES_PER_SECOND: f64 = 1_000.0;
const MAXIMUM_SAMPLE_RATE: f64 = 192_000.0;

/// Copied identity strings reported by one initialized core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreMetadata {
    name: String,
    version: String,
}

impl CoreMetadata {
    /// Upstream library name or the profile fallback.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String-to-str dereference is not const on the supported Rust toolchain"
    )]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Upstream version string, which may be empty.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "String-to-str dereference is not const on the supported Rust toolchain"
    )]
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// Validated geometry and timing reported after content loading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreAvInfo {
    source_dimensions: Dimensions,
    maximum_dimensions: Dimensions,
    aspect_ratio: f32,
    frames_per_second: f64,
    frame_period: Duration,
    sample_rate: SampleRate,
}

impl CoreAvInfo {
    /// Base dimensions expected from ordinary video callbacks.
    #[must_use]
    pub const fn source_dimensions(self) -> Dimensions {
        self.source_dimensions
    }

    /// Largest dimensions declared by the core.
    #[must_use]
    pub const fn maximum_dimensions(self) -> Dimensions {
        self.maximum_dimensions
    }

    /// Declared display aspect ratio, or zero when geometry determines it.
    #[must_use]
    pub const fn aspect_ratio(self) -> f32 {
        self.aspect_ratio
    }

    /// Native frames per second retained for diagnostics.
    #[must_use]
    pub const fn frames_per_second(self) -> f64 {
        self.frames_per_second
    }

    /// Rounded monotonic period used by the runtime frame clock.
    #[must_use]
    pub const fn frame_period(self) -> Duration {
        self.frame_period
    }

    /// Rounded, bounded source PCM rate.
    #[must_use]
    pub const fn sample_rate(self) -> SampleRate {
        self.sample_rate
    }

    fn validate(raw: abi::SystemAvInfo) -> Result<Self, CoreSessionError> {
        let base_width = usize::try_from(raw.geometry.base_width)
            .map_err(|_| CoreSessionError::InvalidAv("base width is not representable"))?;
        let base_height = usize::try_from(raw.geometry.base_height)
            .map_err(|_| CoreSessionError::InvalidAv("base height is not representable"))?;
        let maximum_width = usize::try_from(raw.geometry.max_width)
            .map_err(|_| CoreSessionError::InvalidAv("maximum width is not representable"))?;
        let maximum_height = usize::try_from(raw.geometry.max_height)
            .map_err(|_| CoreSessionError::InvalidAv("maximum height is not representable"))?;
        let source_dimensions = Dimensions::new(base_width, base_height)
            .ok_or(CoreSessionError::InvalidAv("base dimensions are invalid"))?;
        let maximum_dimensions = Dimensions::new(maximum_width, maximum_height).ok_or(
            CoreSessionError::InvalidAv("maximum dimensions are invalid"),
        )?;
        if maximum_width < base_width || maximum_height < base_height {
            return Err(CoreSessionError::InvalidAv(
                "maximum dimensions are smaller than the base frame",
            ));
        }
        let aspect_ratio = raw.geometry.aspect_ratio;
        if !aspect_ratio.is_finite() || aspect_ratio < 0.0 {
            return Err(CoreSessionError::InvalidAv("aspect ratio is invalid"));
        }
        let frames_per_second = raw.timing.frames_per_second;
        if !frames_per_second.is_finite()
            || frames_per_second <= 0.0
            || frames_per_second > MAXIMUM_FRAMES_PER_SECOND
        {
            return Err(CoreSessionError::InvalidAv("frame rate is invalid"));
        }
        let frame_period = Duration::try_from_secs_f64(frames_per_second.recip())
            .map_err(|_| CoreSessionError::InvalidAv("frame period is invalid"))?;
        if frame_period.is_zero() {
            return Err(CoreSessionError::InvalidAv("frame period is zero"));
        }
        let sample_rate = rounded_sample_rate(raw.timing.sample_rate)
            .ok_or(CoreSessionError::InvalidAv("sample rate is invalid"))?;
        Ok(Self {
            source_dimensions,
            maximum_dimensions,
            aspect_ratio,
            frames_per_second,
            frame_period,
            sample_rate,
        })
    }
}

/// One initialized core with loaded content and live context-free callbacks.
#[derive(Debug)]
pub struct CoreSession {
    // Keep lifecycle first: Rust drops fields in declaration order, so the
    // core unloads and deinitializes while its callbacks and content are live.
    lifecycle: CoreLifecycle,
    callbacks: CallbackBinding,
    content: Content,
    content_path: CString,
    metadata: CoreMetadata,
    av_info: CoreAvInfo,
    persistence: Persistence,
}

impl CoreSession {
    fn open_with_api(
        core: LibretroCore,
        content: Content,
        api: CoreApi,
    ) -> Result<Self, CoreSessionError> {
        if content.core() != core {
            return Err(CoreSessionError::WrongCore {
                expected: core,
                actual: content.core(),
            });
        }
        let path = content.path();
        let content_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| CoreSessionError::ContentPath(path.to_owned()))?;
        let mut persistence =
            Persistence::new(&content).map_err(CoreSessionError::PersistenceSetup)?;
        let directory = content_directory(path);
        let callbacks =
            CallbackBinding::install(core, directory).map_err(CoreSessionError::Callbacks)?;
        let mut lifecycle = CoreLifecycle::new(api);
        lifecycle.install_callbacks(&callbacks);
        lifecycle.initialize();

        let version = lifecycle.api_version();
        if version != abi::API_VERSION {
            return Err(CoreSessionError::ApiVersion { actual: version });
        }
        let system_info = lifecycle.system_info();
        if system_info.need_fullpath {
            return Err(CoreSessionError::FullPathUnsupported);
        }
        let metadata = metadata(core, system_info);
        lifecycle.configure_controllers(core);

        let game = abi::GameInfo {
            path: content_path.as_ptr(),
            data: content.bytes().as_ptr().cast(),
            size: content.bytes().len(),
            metadata: ptr::null(),
        };
        if !lifecycle.load_game(&game) {
            return Err(CoreSessionError::ContentRejected { core });
        }
        let av_info = CoreAvInfo::validate(lifecycle.system_av_info())?;
        persistence.load(core, &mut lifecycle);

        Ok(Self {
            lifecycle,
            callbacks,
            content,
            content_path,
            metadata,
            av_info,
            persistence,
        })
    }

    /// Selected statically linked core profile.
    #[must_use]
    pub const fn core(&self) -> LibretroCore {
        self.content.core()
    }

    /// Copied core identity strings.
    #[must_use]
    pub const fn metadata(&self) -> &CoreMetadata {
        &self.metadata
    }

    /// Validated loaded-content geometry and timing.
    #[must_use]
    pub const fn av_info(&self) -> CoreAvInfo {
        self.av_info
    }

    /// Startup persistence problems retained for explicit diagnostics.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Vec-to-slice dereference is not const on the supported Rust toolchain"
    )]
    pub fn persistence_issues(&self) -> &[PersistenceIssue] {
        &self.persistence.startup_issues
    }

    /// Atomically persist every writable native core memory region.
    ///
    /// Regions whose file or core-memory validation failed at startup remain
    /// untouched and are returned as [`PersistenceIssue::WriteBlocked`].
    #[must_use]
    pub fn save_persistent_memory(&self) -> Vec<PersistenceIssue> {
        self.persistence.save(self.core(), &self.lifecycle)
    }
}

/// Core initialization, metadata, content, or timing failure.
#[derive(Debug)]
pub enum CoreSessionError {
    /// Content was prepared for a different core profile.
    WrongCore {
        /// Statically selected core.
        expected: LibretroCore,
        /// Core used during content validation.
        actual: LibretroCore,
    },
    /// Content path cannot form a stable C string.
    ContentPath(PathBuf),
    /// Native persistence paths cannot be derived safely.
    PersistenceSetup(SaveError),
    /// Process-global callback ownership or environment setup failed.
    Callbacks(CallbackBindingError),
    /// Core reports a libretro API other than version 1.
    ApiVersion {
        /// Reported API version.
        actual: c_uint,
    },
    /// This in-memory host cannot satisfy a full-path-only core.
    FullPathUnsupported,
    /// The selected core rejected validated content.
    ContentRejected {
        /// Rejecting core.
        core: LibretroCore,
    },
    /// Core geometry or timing violated the bounded runtime contract.
    InvalidAv(&'static str),
}

impl fmt::Display for CoreSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongCore { expected, actual } => write!(
                formatter,
                "{} content cannot be loaded by the {} host",
                actual.system_name(),
                expected.system_name()
            ),
            Self::ContentPath(path) => write!(
                formatter,
                "content path cannot form a C string: {}",
                path.display()
            ),
            Self::PersistenceSetup(source) => source.fmt(formatter),
            Self::Callbacks(source) => source.fmt(formatter),
            Self::ApiVersion { actual } => {
                write!(formatter, "core reports libretro API {actual}; expected 1")
            }
            Self::FullPathUnsupported => {
                formatter.write_str("core requires unsupported full-path content loading")
            }
            Self::ContentRejected { core } => {
                write!(
                    formatter,
                    "{} core rejected the content image",
                    core.core_name()
                )
            }
            Self::InvalidAv(reason) => {
                write!(formatter, "core AV information is invalid: {reason}")
            }
        }
    }
}

impl Error for CoreSessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Callbacks(source) => Some(source),
            Self::PersistenceSetup(source) => Some(source),
            Self::WrongCore { .. }
            | Self::ContentPath(_)
            | Self::ApiVersion { .. }
            | Self::FullPathUnsupported
            | Self::ContentRejected { .. }
            | Self::InvalidAv(_) => None,
        }
    }
}

/// Invalid memory region reported by an initialized libretro core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreMemoryError {
    /// A nonempty region has no address.
    NullPointer {
        /// Profile memory kind being queried.
        kind: MemoryKind,
        /// Nonzero byte count reported by the core.
        bytes: usize,
    },
    /// A region exceeds the persistence allocation bound.
    TooLarge {
        /// Profile memory kind being queried.
        kind: MemoryKind,
        /// Byte count reported by the core.
        bytes: usize,
    },
}

impl fmt::Display for CoreMemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullPointer { kind, bytes } => {
                write!(
                    formatter,
                    "core reports {bytes} bytes of {kind:?} memory at a null address"
                )
            }
            Self::TooLarge { kind, bytes } => write!(
                formatter,
                "core reports {bytes} bytes of {kind:?} memory; maximum is {MAXIMUM_SAVE_BYTES}"
            ),
        }
    }
}

impl CoreMemoryError {
    /// Profile memory kind rejected by validation.
    #[must_use]
    pub const fn kind(self) -> MemoryKind {
        match self {
            Self::NullPointer { kind, .. } | Self::TooLarge { kind, .. } => kind,
        }
    }
}

impl Error for CoreMemoryError {}

/// Nonfatal native persistence problem for one loaded memory region.
#[derive(Debug)]
pub enum PersistenceIssue {
    /// The core exposed an invalid memory region.
    CoreMemory(CoreMemoryError),
    /// An existing native save could not be loaded exactly.
    Read {
        /// Profile memory kind left unchanged in the core.
        kind: MemoryKind,
        /// Filesystem validation or I/O failure.
        source: SaveError,
    },
    /// A native save could not be written atomically.
    Write {
        /// Profile memory kind that remains only in core memory.
        kind: MemoryKind,
        /// Filesystem validation or I/O failure.
        source: SaveError,
    },
    /// A startup validation failure protects existing data from replacement.
    WriteBlocked {
        /// Profile memory kind deliberately left on disk unchanged.
        kind: MemoryKind,
    },
}

impl PersistenceIssue {
    /// Memory kind affected by this problem.
    #[must_use]
    pub const fn kind(&self) -> MemoryKind {
        match self {
            Self::CoreMemory(source) => source.kind(),
            Self::Read { kind, .. } | Self::Write { kind, .. } | Self::WriteBlocked { kind } => {
                *kind
            }
        }
    }
}

impl fmt::Display for PersistenceIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreMemory(source) => source.fmt(formatter),
            Self::Read { kind, source } => {
                write!(formatter, "cannot load native {kind:?} memory: {source}")
            }
            Self::Write { kind, source } => {
                write!(formatter, "cannot save native {kind:?} memory: {source}")
            }
            Self::WriteBlocked { kind } => write!(
                formatter,
                "native {kind:?} memory was not saved because startup validation did not establish a safe replacement"
            ),
        }
    }
}

impl Error for PersistenceIssue {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CoreMemory(source) => Some(source),
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::WriteBlocked { .. } => None,
        }
    }
}

#[derive(Debug)]
struct Persistence {
    store: SaveStore,
    blocked: Vec<MemoryKind>,
    startup_issues: Vec<PersistenceIssue>,
}

impl Persistence {
    fn new(content: &Content) -> Result<Self, SaveError> {
        Ok(Self {
            store: SaveStore::for_content(content)?,
            blocked: Vec::new(),
            startup_issues: Vec::new(),
        })
    }

    fn load(&mut self, core: LibretroCore, lifecycle: &mut CoreLifecycle) {
        for memory in core.memory_files() {
            let result = lifecycle.memory_mut(memory.kind());
            match result {
                Ok(None) => {}
                Ok(Some(destination)) => {
                    if let Err(source) = self.store.load(*memory, destination) {
                        self.block(memory.kind());
                        self.startup_issues.push(PersistenceIssue::Read {
                            kind: memory.kind(),
                            source,
                        });
                    }
                }
                Err(source) => {
                    self.block(source.kind());
                    self.startup_issues
                        .push(PersistenceIssue::CoreMemory(source));
                }
            }
        }
    }

    fn save(&self, core: LibretroCore, lifecycle: &CoreLifecycle) -> Vec<PersistenceIssue> {
        let mut issues = Vec::new();
        for memory in core.memory_files() {
            let kind = memory.kind();
            if self.blocked.contains(&kind) {
                issues.push(PersistenceIssue::WriteBlocked { kind });
                continue;
            }
            match lifecycle.memory(kind) {
                Ok(None) => {}
                Ok(Some(source)) => {
                    if let Err(source) = self.store.save(*memory, source) {
                        issues.push(PersistenceIssue::Write { kind, source });
                    }
                }
                Err(source) => issues.push(PersistenceIssue::CoreMemory(source)),
            }
        }
        issues
    }

    fn block(&mut self, kind: MemoryKind) {
        if !self.blocked.contains(&kind) {
            self.blocked.push(kind);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CoreApi {
    set_environment: unsafe extern "C" fn(abi::EnvironmentCallback),
    set_video_refresh: unsafe extern "C" fn(abi::VideoRefreshCallback),
    set_audio_sample: unsafe extern "C" fn(abi::AudioSampleCallback),
    set_audio_sample_batch: unsafe extern "C" fn(abi::AudioSampleBatchCallback),
    set_input_poll: unsafe extern "C" fn(abi::InputPollCallback),
    set_input_state: unsafe extern "C" fn(abi::InputStateCallback),
    init: unsafe extern "C" fn(),
    deinit: unsafe extern "C" fn(),
    api_version: unsafe extern "C" fn() -> c_uint,
    get_system_info: unsafe extern "C" fn(*mut abi::SystemInfo),
    get_system_av_info: unsafe extern "C" fn(*mut abi::SystemAvInfo),
    set_controller_port_device: unsafe extern "C" fn(c_uint, c_uint),
    load_game: unsafe extern "C" fn(*const abi::GameInfo) -> bool,
    unload_game: unsafe extern "C" fn(),
    run: unsafe extern "C" fn(),
    get_memory_data: unsafe extern "C" fn(c_uint) -> *mut c_void,
    get_memory_size: unsafe extern "C" fn(c_uint) -> usize,
}

#[derive(Debug)]
struct CoreLifecycle {
    api: CoreApi,
    initialized: bool,
    loaded: bool,
}

impl CoreLifecycle {
    const fn new(api: CoreApi) -> Self {
        Self {
            api,
            initialized: false,
            loaded: false,
        }
    }

    fn install_callbacks(&self, callbacks: &CallbackBinding) {
        // SAFETY: Every callback has the exact private API-v1 signature and
        // remains bound until after this lifecycle unloads and deinitializes.
        unsafe {
            (self.api.set_environment)(callbacks.environment_callback());
            (self.api.set_video_refresh)(callbacks.video_refresh_callback());
            (self.api.set_audio_sample)(callbacks.audio_sample_callback());
            (self.api.set_audio_sample_batch)(callbacks.audio_sample_batch_callback());
            (self.api.set_input_poll)(callbacks.input_poll_callback());
            (self.api.set_input_state)(callbacks.input_state_callback());
        }
    }

    fn initialize(&mut self) {
        // SAFETY: Callback registration is complete and this lifecycle owns
        // the process-wide session.
        unsafe { (self.api.init)() };
        self.initialized = true;
    }

    fn api_version(&self) -> c_uint {
        // SAFETY: The initialized pinned core exposes this scalar query.
        unsafe { (self.api.api_version)() }
    }

    fn system_info(&self) -> abi::SystemInfo {
        let mut info = abi::SystemInfo::default();
        // SAFETY: `info` is writable for the complete C structure.
        unsafe { (self.api.get_system_info)(ptr::from_mut(&mut info)) };
        info
    }

    fn configure_controllers(&self, core: LibretroCore) {
        for (port, controller) in core.controller_ports().iter().copied().enumerate() {
            let Ok(port) = c_uint::try_from(port) else {
                continue;
            };
            let device = match controller {
                ControllerDevice::Joypad => abi::DEVICE_JOYPAD,
                ControllerDevice::JoypadSubclass(identifier) => {
                    abi::device_subclass(abi::DEVICE_JOYPAD, c_uint::from(identifier))
                }
            };
            // SAFETY: The profile contains only bounded API-v1 port devices.
            unsafe { (self.api.set_controller_port_device)(port, device) };
        }
    }

    fn load_game(&mut self, game: &abi::GameInfo) -> bool {
        // SAFETY: The content path and byte allocation outlive this lifecycle.
        let loaded = unsafe { (self.api.load_game)(ptr::from_ref(game)) };
        self.loaded = loaded;
        loaded
    }

    fn system_av_info(&self) -> abi::SystemAvInfo {
        let mut info = abi::SystemAvInfo::default();
        // SAFETY: `info` is writable and content is loaded.
        unsafe { (self.api.get_system_av_info)(ptr::from_mut(&mut info)) };
        info
    }

    fn memory_mut(&mut self, kind: MemoryKind) -> Result<Option<&mut [u8]>, CoreMemoryError> {
        let Some((pointer, bytes)) = self.memory_region(kind)? else {
            return Ok(None);
        };
        // SAFETY: The pinned loaded core reports `bytes` writable bytes for
        // this region. The bound is far below `isize::MAX`, the pointer is
        // non-null, and the mutable lifecycle borrow prevents `retro_run` or
        // another region query while the slice is live.
        Ok(Some(unsafe { slice::from_raw_parts_mut(pointer, bytes) }))
    }

    fn memory(&self, kind: MemoryKind) -> Result<Option<&[u8]>, CoreMemoryError> {
        let Some((pointer, bytes)) = self.memory_region(kind)? else {
            return Ok(None);
        };
        // SAFETY: The pinned loaded core reports `bytes` readable bytes for
        // this region. The bound is far below `isize::MAX`, the pointer is
        // non-null, and callers cannot mutably drive the lifecycle while this
        // shared slice is live.
        Ok(Some(unsafe { slice::from_raw_parts(pointer, bytes) }))
    }

    fn memory_region(&self, kind: MemoryKind) -> Result<Option<(*mut u8, usize)>, CoreMemoryError> {
        let identifier = memory_identifier(kind);
        // SAFETY: Content is loaded and the identifier is defined by API v1.
        let bytes = unsafe { (self.api.get_memory_size)(identifier) };
        if bytes == 0 {
            return Ok(None);
        }
        if bytes > MAXIMUM_SAVE_BYTES {
            return Err(CoreMemoryError::TooLarge { kind, bytes });
        }
        // SAFETY: Content is loaded and the identifier is defined by API v1.
        let pointer = unsafe { (self.api.get_memory_data)(identifier) }.cast::<u8>();
        if pointer.is_null() {
            Err(CoreMemoryError::NullPointer { kind, bytes })
        } else {
            Ok(Some((pointer, bytes)))
        }
    }
}

impl Drop for CoreLifecycle {
    fn drop(&mut self) {
        if self.loaded {
            // SAFETY: This lifecycle owns the one successfully loaded game.
            unsafe { (self.api.unload_game)() };
            self.loaded = false;
        }
        if self.initialized {
            // SAFETY: This lifecycle owns the one initialized core.
            unsafe { (self.api.deinit)() };
            self.initialized = false;
        }
    }
}

fn content_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

const fn memory_identifier(kind: MemoryKind) -> c_uint {
    match kind {
        MemoryKind::SaveRam => abi::MEMORY_SAVE_RAM,
        MemoryKind::Rtc => abi::MEMORY_RTC,
    }
}

fn metadata(core: LibretroCore, info: abi::SystemInfo) -> CoreMetadata {
    CoreMetadata {
        // SAFETY: The pinned core owns valid static NUL-terminated metadata
        // pointers for its complete initialized lifetime.
        name: unsafe { copy_core_string(info.library_name, core.core_name()) },
        // SAFETY: The same core metadata contract applies to the version.
        version: unsafe { copy_core_string(info.library_version, "") },
    }
}

unsafe fn copy_core_string(pointer: *const c_char, fallback: &str) -> String {
    if pointer.is_null() {
        fallback.to_owned()
    } else {
        // SAFETY: The caller guarantees a valid NUL-terminated core string.
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

fn rounded_sample_rate(rate: f64) -> Option<SampleRate> {
    if !rate.is_finite() || rate <= 0.0 || rate > MAXIMUM_SAMPLE_RATE {
        return None;
    }
    let rounded = rate.round();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the finite positive value is bounded to u32 before rounding and conversion"
    )]
    SampleRate::new(rounded as u32)
}

#[cfg(test)]
mod tests {
    use super::super::callbacks::serialize_test_sessions;
    use super::*;
    use std::fs;
    use std::os::unix::ffi::OsStringExt as _;
    use std::slice;
    use std::sync::{LazyLock, Mutex, MutexGuard};

    static FAKE_CORE: LazyLock<Mutex<FakeCore>> = LazyLock::new(|| Mutex::new(FakeCore::default()));
    static LIBRARY_NAME: &[u8] = b"Test Core\0";
    static LIBRARY_VERSION: &[u8] = b"1.2.3\0";

    #[derive(Debug)]
    struct FakeCore {
        events: Vec<&'static str>,
        api_version: c_uint,
        need_fullpath: bool,
        accept_game: bool,
        av_info: abi::SystemAvInfo,
        content: Vec<u8>,
        controllers: Vec<(c_uint, c_uint)>,
        save_ram: Vec<u8>,
        rtc: Vec<u8>,
        null_memory: Option<MemoryKind>,
        reported_memory_size: Option<(MemoryKind, usize)>,
    }

    impl Default for FakeCore {
        fn default() -> Self {
            Self {
                events: Vec::new(),
                api_version: abi::API_VERSION,
                need_fullpath: false,
                accept_game: true,
                av_info: valid_av_info(),
                content: Vec::new(),
                controllers: Vec::new(),
                save_ram: Vec::new(),
                rtc: Vec::new(),
                null_memory: None,
                reported_memory_size: None,
            }
        }
    }

    fn fake() -> MutexGuard<'static, FakeCore> {
        FAKE_CORE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn reset_fake() {
        *fake() = FakeCore::default();
    }

    fn valid_av_info() -> abi::SystemAvInfo {
        abi::SystemAvInfo {
            geometry: abi::GameGeometry {
                base_width: 256,
                base_height: 240,
                max_width: 512,
                max_height: 480,
                aspect_ratio: 4.0 / 3.0,
            },
            timing: abi::SystemTiming {
                frames_per_second: 60.098_8,
                sample_rate: 47_999.6,
            },
        }
    }

    fn test_api() -> CoreApi {
        CoreApi {
            set_environment: fake_set_environment,
            set_video_refresh: fake_set_video_refresh,
            set_audio_sample: fake_set_audio_sample,
            set_audio_sample_batch: fake_set_audio_sample_batch,
            set_input_poll: fake_set_input_poll,
            set_input_state: fake_set_input_state,
            init: fake_init,
            deinit: fake_deinit,
            api_version: fake_api_version,
            get_system_info: fake_get_system_info,
            get_system_av_info: fake_get_system_av_info,
            set_controller_port_device: fake_set_controller_port_device,
            load_game: fake_load_game,
            unload_game: fake_unload_game,
            run: fake_run,
            get_memory_data: fake_get_memory_data,
            get_memory_size: fake_get_memory_size,
        }
    }

    fn content_fixture(core: LibretroCore) -> (tempfile::TempDir, Content) {
        let directory = tempfile::tempdir().expect("temporary content directory");
        let extension = core.extensions().first().copied().expect("core extension");
        let path = directory.path().join(format!("game.{extension}"));
        let bytes = vec![0x5a; core.minimum_rom_bytes()];
        fs::write(&path, bytes).expect("content fixture");
        let content = Content::load(core, &path).expect("validated content");
        (directory, content)
    }

    #[test]
    fn session_orders_callbacks_initialization_content_and_cleanup() {
        let _session = serialize_test_sessions();
        reset_fake();
        let (_directory, content) = content_fixture(LibretroCore::Fceumm);
        {
            let session = CoreSession::open_with_api(LibretroCore::Fceumm, content, test_api())
                .expect("valid fake core session");
            assert_eq!(session.core(), LibretroCore::Fceumm);
            assert_eq!(session.metadata().name(), "Test Core");
            assert_eq!(session.metadata().version(), "1.2.3");
            assert_eq!(
                session.av_info().source_dimensions(),
                Dimensions::new(256, 240).expect("dimensions")
            );
            assert_eq!(session.av_info().sample_rate().get(), 48_000);
            let state = fake();
            assert_eq!(state.content, vec![0x5a; 16]);
            assert_eq!(
                state.controllers,
                vec![(0, abi::DEVICE_JOYPAD), (1, abi::DEVICE_JOYPAD)]
            );
            assert_eq!(
                state.events,
                [
                    "set environment",
                    "set video",
                    "set audio sample",
                    "set audio batch",
                    "set input poll",
                    "set input state",
                    "init",
                    "api version",
                    "system info",
                    "controller",
                    "controller",
                    "load",
                    "AV info",
                ]
            );
        }
        assert!(fake().events.ends_with(&["unload", "deinit"]));
    }

    #[test]
    fn every_post_init_failure_cleans_up_exactly_once() {
        let _session = serialize_test_sessions();
        reset_fake();
        fake().api_version = 99;
        let (_directory, content) = content_fixture(LibretroCore::Gambatte);
        assert!(CoreSession::open_with_api(LibretroCore::Gambatte, content, test_api()).is_err());
        assert!(fake().events.ends_with(&["api version", "deinit"]));
        assert!(!fake().events.contains(&"unload"));

        reset_fake();
        fake().av_info.geometry.max_width = 1;
        let (_directory, content) = content_fixture(LibretroCore::Fceumm);
        assert!(CoreSession::open_with_api(LibretroCore::Fceumm, content, test_api()).is_err());
        assert!(fake().events.ends_with(&["AV info", "unload", "deinit"]));
    }

    #[test]
    fn session_preserves_non_utf8_unix_content_paths() {
        let _session = serialize_test_sessions();
        reset_fake();
        let directory = tempfile::tempdir().expect("temporary content directory");
        let filename = std::ffi::OsString::from_vec(b"game-\xff.nes".to_vec());
        let path = directory.path().join(filename);
        fs::write(&path, vec![0x5a; 16]).expect("content fixture");
        let content = Content::load(LibretroCore::Fceumm, &path).expect("validated content");
        let session = CoreSession::open_with_api(LibretroCore::Fceumm, content, test_api());
        assert!(session.is_ok());
    }

    #[test]
    fn av_validation_rejects_nonfinite_and_unbounded_timing() {
        let mut raw = valid_av_info();
        assert!(CoreAvInfo::validate(raw).is_ok());
        raw.timing.frames_per_second = f64::NAN;
        assert!(CoreAvInfo::validate(raw).is_err());
        raw = valid_av_info();
        raw.timing.sample_rate = MAXIMUM_SAMPLE_RATE + 1.0;
        assert!(CoreAvInfo::validate(raw).is_err());
        raw = valid_av_info();
        raw.geometry.aspect_ratio = -1.0;
        assert!(CoreAvInfo::validate(raw).is_err());
    }

    #[test]
    fn memory_regions_are_empty_mutable_bounded_and_nonnull() {
        let _session = serialize_test_sessions();
        reset_fake();
        let (_directory, content) = content_fixture(LibretroCore::Gambatte);
        let mut session = CoreSession::open_with_api(LibretroCore::Gambatte, content, test_api())
            .expect("valid fake core session");
        assert!(matches!(
            session.lifecycle.memory_mut(MemoryKind::SaveRam),
            Ok(None)
        ));

        fake().save_ram = vec![0x11; 8];
        let memory = session
            .lifecycle
            .memory_mut(MemoryKind::SaveRam)
            .expect("valid memory")
            .expect("nonempty memory");
        memory.copy_from_slice(b"save ram");
        assert_eq!(fake().save_ram, b"save ram");
        assert!(matches!(
            session.lifecycle.memory(MemoryKind::SaveRam),
            Ok(Some(memory)) if memory == b"save ram"
        ));

        fake().reported_memory_size = Some((MemoryKind::Rtc, MAXIMUM_SAVE_BYTES + 1));
        assert!(matches!(
            session.lifecycle.memory(MemoryKind::Rtc),
            Err(CoreMemoryError::TooLarge {
                kind: MemoryKind::Rtc,
                bytes
            }) if bytes == MAXIMUM_SAVE_BYTES + 1
        ));
        fake().reported_memory_size = None;
        fake().rtc = vec![0; 4];
        fake().null_memory = Some(MemoryKind::Rtc);
        assert!(matches!(
            session.lifecycle.memory(MemoryKind::Rtc),
            Err(CoreMemoryError::NullPointer {
                kind: MemoryKind::Rtc,
                bytes: 4
            })
        ));
    }

    #[test]
    fn startup_loads_exact_native_saves_and_preserves_malformed_files() {
        let _session = serialize_test_sessions();
        reset_fake();
        fake().save_ram = vec![0; 8];
        fake().rtc = vec![0x77; 4];
        let (directory, content) = content_fixture(LibretroCore::Gambatte);
        let save_path = directory.path().join("game.sav");
        let rtc_path = directory.path().join("game.rtc");
        fs::write(&save_path, b"battery!").expect("exact native save");
        fs::write(&rtc_path, b"bad").expect("malformed native RTC");

        let session = CoreSession::open_with_api(LibretroCore::Gambatte, content, test_api())
            .expect("save errors do not block gameplay");
        assert_eq!(fake().save_ram, b"battery!");
        assert_eq!(fake().rtc, [0x77; 4]);
        assert!(matches!(
            session.persistence_issues(),
            [PersistenceIssue::Read {
                kind: MemoryKind::Rtc,
                ..
            }]
        ));
        assert_eq!(session.persistence.blocked, [MemoryKind::Rtc]);
        assert!(matches!(fs::read(&rtc_path), Ok(bytes) if bytes == b"bad"));

        fake().save_ram = b"updated!".to_vec();
        fake().rtc = b"nope".to_vec();
        assert!(matches!(
            session.save_persistent_memory().as_slice(),
            [PersistenceIssue::WriteBlocked {
                kind: MemoryKind::Rtc
            }]
        ));
        assert!(matches!(fs::read(&save_path), Ok(bytes) if bytes == b"updated!"));
        assert!(matches!(fs::read(&rtc_path), Ok(bytes) if bytes == b"bad"));
    }

    #[test]
    fn missing_saves_are_normal_but_invalid_core_memory_is_blocked() {
        let _session = serialize_test_sessions();
        reset_fake();
        fake().save_ram = vec![0x44; 4];
        let (_directory, content) = content_fixture(LibretroCore::Fceumm);
        {
            let session = CoreSession::open_with_api(LibretroCore::Fceumm, content, test_api())
                .expect("missing save is normal");
            assert!(session.persistence_issues().is_empty());
            assert!(session.persistence.blocked.is_empty());
            assert_eq!(fake().save_ram, [0x44; 4]);
        }

        reset_fake();
        fake().reported_memory_size = Some((MemoryKind::SaveRam, MAXIMUM_SAVE_BYTES + 1));
        let (_directory, content) = content_fixture(LibretroCore::Fceumm);
        let session = CoreSession::open_with_api(LibretroCore::Fceumm, content, test_api())
            .expect("invalid optional persistence does not block gameplay");
        assert!(matches!(
            session.persistence_issues(),
            [PersistenceIssue::CoreMemory(CoreMemoryError::TooLarge {
                kind: MemoryKind::SaveRam,
                ..
            })]
        ));
        assert_eq!(session.persistence.blocked, [MemoryKind::SaveRam]);
    }

    #[test]
    fn explicit_save_reports_atomic_write_failures() {
        let _session = serialize_test_sessions();
        reset_fake();
        fake().save_ram = vec![0x44; 4];
        let (directory, content) = content_fixture(LibretroCore::Fceumm);
        let session = CoreSession::open_with_api(LibretroCore::Fceumm, content, test_api())
            .expect("valid session");
        fs::create_dir(directory.path().join("game.srm")).expect("blocking directory");
        assert!(matches!(
            session.save_persistent_memory().as_slice(),
            [PersistenceIssue::Write {
                kind: MemoryKind::SaveRam,
                ..
            }]
        ));
        assert!(!fake().events.contains(&"unload"));
    }

    unsafe extern "C" fn fake_set_environment(_: abi::EnvironmentCallback) {
        fake().events.push("set environment");
    }

    unsafe extern "C" fn fake_set_video_refresh(_: abi::VideoRefreshCallback) {
        fake().events.push("set video");
    }

    unsafe extern "C" fn fake_set_audio_sample(_: abi::AudioSampleCallback) {
        fake().events.push("set audio sample");
    }

    unsafe extern "C" fn fake_set_audio_sample_batch(_: abi::AudioSampleBatchCallback) {
        fake().events.push("set audio batch");
    }

    unsafe extern "C" fn fake_set_input_poll(_: abi::InputPollCallback) {
        fake().events.push("set input poll");
    }

    unsafe extern "C" fn fake_set_input_state(_: abi::InputStateCallback) {
        fake().events.push("set input state");
    }

    unsafe extern "C" fn fake_init() {
        fake().events.push("init");
    }

    unsafe extern "C" fn fake_deinit() {
        fake().events.push("deinit");
    }

    unsafe extern "C" fn fake_api_version() -> c_uint {
        let mut state = fake();
        state.events.push("api version");
        state.api_version
    }

    unsafe extern "C" fn fake_get_system_info(info: *mut abi::SystemInfo) {
        let mut state = fake();
        state.events.push("system info");
        let need_fullpath = state.need_fullpath;
        drop(state);
        // SAFETY: The session supplies a writable `SystemInfo`.
        if let Some(info) = unsafe { info.as_mut() } {
            *info = abi::SystemInfo {
                library_name: LIBRARY_NAME.as_ptr().cast(),
                library_version: LIBRARY_VERSION.as_ptr().cast(),
                valid_extensions: ptr::null(),
                need_fullpath,
                block_extract: false,
            };
        }
    }

    unsafe extern "C" fn fake_get_system_av_info(info: *mut abi::SystemAvInfo) {
        let mut state = fake();
        state.events.push("AV info");
        let av_info = state.av_info;
        drop(state);
        // SAFETY: The session supplies a writable `SystemAvInfo`.
        if let Some(info) = unsafe { info.as_mut() } {
            *info = av_info;
        }
    }

    unsafe extern "C" fn fake_set_controller_port_device(port: c_uint, device: c_uint) {
        let mut state = fake();
        state.events.push("controller");
        state.controllers.push((port, device));
    }

    unsafe extern "C" fn fake_load_game(game: *const abi::GameInfo) -> bool {
        let mut state = fake();
        state.events.push("load");
        let accept = state.accept_game;
        // SAFETY: The session supplies a readable game descriptor and keeps
        // its nonempty content allocation live.
        if let Some(game) = unsafe { game.as_ref() } {
            // SAFETY: The game descriptor owns `size` readable content bytes.
            state.content =
                unsafe { slice::from_raw_parts(game.data.cast::<u8>(), game.size) }.to_vec();
        }
        accept
    }

    unsafe extern "C" fn fake_unload_game() {
        fake().events.push("unload");
    }

    const unsafe extern "C" fn fake_run() {}

    unsafe extern "C" fn fake_get_memory_data(identifier: c_uint) -> *mut c_void {
        let mut state = fake();
        let kind = memory_kind(identifier);
        if state.null_memory == kind {
            return ptr::null_mut();
        }
        match kind {
            Some(MemoryKind::SaveRam) => state.save_ram.as_mut_ptr().cast(),
            Some(MemoryKind::Rtc) => state.rtc.as_mut_ptr().cast(),
            None => ptr::null_mut(),
        }
    }

    unsafe extern "C" fn fake_get_memory_size(identifier: c_uint) -> usize {
        let state = fake();
        let kind = memory_kind(identifier);
        if let Some((reported_kind, bytes)) = state.reported_memory_size {
            if Some(reported_kind) == kind {
                return bytes;
            }
        }
        match kind {
            Some(MemoryKind::SaveRam) => state.save_ram.len(),
            Some(MemoryKind::Rtc) => state.rtc.len(),
            None => 0,
        }
    }

    const fn memory_kind(identifier: c_uint) -> Option<MemoryKind> {
        match identifier {
            abi::MEMORY_SAVE_RAM => Some(MemoryKind::SaveRam),
            abi::MEMORY_RTC => Some(MemoryKind::Rtc),
            _ => None,
        }
    }
}
