//! Axum runtime and installed uploader assembly.

use std::{fmt, io, net::SocketAddr, path::Path, sync::Arc};

use tokio::net::TcpListener;

use crate::{
    address::{AddressError, ServiceAddress},
    application::Application,
    auth::AuthManager,
    palette::{PaletteStore, PaletteStoreError},
    password::{PasswordConfig, PasswordError},
    process::CommandRestarter,
    store::{RomStore, StoreError},
};

const SERVER_THREADS: usize = 2;
const DASHBOARD_RESTART_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

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
/// runtime, listener, or server failures.
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(SERVER_THREADS)
        .enable_all()
        .build()?;
    runtime.block_on(serve(application, address))?;
    Ok(())
}

/// Serve an assembled application with Axum and Hyper.
///
/// # Errors
///
/// Returns an I/O error when the listener cannot bind or the server fails.
pub async fn serve(application: Application, address: ServiceAddress) -> io::Result<()> {
    let socket = SocketAddr::V4(address.socket_addr());
    let listener = TcpListener::bind(socket).await?;
    eprintln!("rom-uploader: listening at {address} on all IPv4 interfaces");
    axum::serve(
        listener,
        Arc::new(application)
            .router()
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
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
    /// Process control, runtime creation, binding, or serving failed.
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
