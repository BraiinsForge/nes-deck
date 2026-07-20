//! Native Retro Deck dashboard model, renderer, and runtime seams.

#[cfg(feature = "application-wire")]
mod application;
#[cfg(feature = "legacy-runtime")]
mod artwork;
#[cfg(feature = "legacy-runtime")]
mod assets;
#[cfg(feature = "legacy-runtime")]
mod audio;
#[cfg(feature = "bmc-native")]
mod bmc_ui;
mod catalog;
#[cfg(feature = "legacy-runtime")]
mod controls;
#[cfg(feature = "legacy-runtime")]
mod credits;
mod launch;
#[cfg(feature = "bmc-native")]
mod native_catalog;
#[cfg(feature = "legacy-runtime")]
mod network_worker;
#[cfg(feature = "legacy-runtime")]
mod preference_io;
#[cfg(feature = "legacy-runtime")]
mod preference_worker;
#[cfg(feature = "legacy-runtime")]
mod preferences;
#[cfg(feature = "legacy-runtime")]
mod render;
#[cfg(feature = "legacy-runtime")]
mod settings;
mod state;
#[cfg(feature = "legacy-runtime")]
mod wifi;
#[cfg(feature = "legacy-runtime")]
mod wifi_render;
#[cfg(feature = "legacy-runtime")]
mod wifi_session;
#[cfg(feature = "legacy-runtime")]
mod wifi_writer;

#[cfg(feature = "application-wire")]
pub use application::{
    ApplicationRequest, ApplicationRequestError, BMC_APPLICATION_ID,
    MAXIMUM_APPLICATION_INPUT_BYTES,
};
#[cfg(feature = "legacy-runtime")]
pub use artwork::{ArtworkError, ArtworkIssue, ArtworkReport, ArtworkStore, ArtworkStoreError};
#[cfg(feature = "legacy-runtime")]
pub use assets::{
    AssetKind, AssetPathError, CreditsFallback, DashboardAssetPaths, DashboardAssets,
    DashboardAssetsError, PaletteFallback,
};
#[cfg(feature = "legacy-runtime")]
pub use audio::menu_notes;
#[cfg(feature = "bmc-native")]
pub use bmc_ui::{BmcScreen, BmcUiAction, bmc_action_for_touch, build_bmc_tree};
pub use catalog::{Category, DashboardCatalog, DashboardCatalogError, MAXIMUM_DASHBOARD_ENTRIES};
#[cfg(feature = "legacy-runtime")]
pub use controls::{
    ControllerGuard, ExitHold, ExitHoldEvent, TouchCommitter, controller_action, keyboard_action,
    wifi_keyboard_action,
};
#[cfg(feature = "legacy-runtime")]
pub use credits::{CreditsCrawl, CreditsLayout};
pub use launch::{
    ExitPolicy, LaunchPlan, LaunchPlanError, LaunchTarget, LaunchTargetError, TerminalMode,
};
#[cfg(feature = "bmc-native")]
pub use native_catalog::{NativeCatalogError, load_native_catalog};
#[cfg(feature = "legacy-runtime")]
pub use network_worker::{
    NetworkStatus, NetworkStatusConfig, NetworkStatusConfigError, NetworkStatusError,
    NetworkStatusPoll, NetworkStatusWorker, NetworkStatusWorkerReport,
};
#[cfg(feature = "legacy-runtime")]
pub use preference_io::{
    PreferenceLoad, PreferenceLoadError, PreferenceLoadIssue, PreferencePathError, PreferencePaths,
};
#[cfg(feature = "legacy-runtime")]
pub use preference_worker::{
    BrightnessDevicePaths, BrightnessPathError, PreferenceSubmit, PreferenceWorker,
    PreferenceWorkerError, PreferenceWorkerReport, PreferenceWriteError, brightness_raw_value,
};
#[cfg(feature = "legacy-runtime")]
pub use preferences::{
    DashboardPreferences, EncodedPreference, MAXIMUM_PREFERENCE_BYTES, PreferenceField,
    PreferenceValueError, encode_setting, parse_brightness, parse_keymap, parse_volume,
};
#[cfg(feature = "legacy-runtime")]
pub use render::{
    ArtworkProvider, CANVAS_HEIGHT, CANVAS_WIDTH, Cover, CoverError, DashboardFrame, EntryButton,
    MenuLayout, NoArtwork, RenderError, RenderedScreen,
};
#[cfg(feature = "legacy-runtime")]
pub use settings::{NetworkView, SettingsLayout, SettingsView};
pub use state::{
    Action, Brightness, BrightnessError, DEFAULT_BRIGHTNESS_PERCENT, DEFAULT_VOLUME_PERCENT,
    DashboardModel, Intent, Keymap, MenuCue, Screen, SettingChange, SettingsTarget, Status,
    Transition, VolumeError, VolumeState,
};
#[cfg(feature = "legacy-runtime")]
pub use wifi::{
    MAXIMUM_PASSPHRASE_BYTES, MAXIMUM_SSID_BYTES, MINIMUM_PASSPHRASE_BYTES, WifiAction,
    WifiCredentials, WifiEditor, WifiEffect, WifiField, WifiStatus, WifiTransition,
};
#[cfg(feature = "legacy-runtime")]
pub use wifi_render::WifiLayout;
#[cfg(feature = "legacy-runtime")]
pub use wifi_session::WifiSession;
#[cfg(feature = "legacy-runtime")]
pub use wifi_writer::{
    WifiProfileWriter, WifiWriteError, WifiWriterPoll, WifiWriterReport, WifiWriterRequestId,
    WifiWriterResult, WifiWriterStartError, WifiWriterSubmit,
};
