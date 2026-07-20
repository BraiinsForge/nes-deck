//! Native Retro Deck dashboard model, renderer, and runtime seams.

mod artwork;
mod assets;
mod audio;
#[cfg(feature = "bmc-native")]
mod bmc_ui;
mod catalog;
mod controls;
mod credits;
mod launch;
mod network_worker;
mod preference_io;
mod preference_worker;
mod preferences;
mod render;
mod settings;
mod state;
mod wifi;
mod wifi_render;
mod wifi_session;
mod wifi_writer;

pub use artwork::{ArtworkError, ArtworkIssue, ArtworkReport, ArtworkStore, ArtworkStoreError};
pub use assets::{
    AssetKind, AssetPathError, CreditsFallback, DashboardAssetPaths, DashboardAssets,
    DashboardAssetsError, PaletteFallback,
};
pub use audio::menu_notes;
#[cfg(feature = "bmc-native")]
pub use bmc_ui::{BmcScreen, BmcUiAction, bmc_action_for_touch, build_bmc_tree};
pub use catalog::{Category, DashboardCatalog, DashboardCatalogError, MAXIMUM_DASHBOARD_ENTRIES};
pub use controls::{
    ControllerGuard, ExitHold, ExitHoldEvent, TouchCommitter, controller_action, keyboard_action,
    wifi_keyboard_action,
};
pub use credits::{CreditsCrawl, CreditsLayout};
pub use launch::{
    ExitPolicy, LaunchPlan, LaunchPlanError, LaunchTarget, LaunchTargetError, TerminalMode,
};
pub use network_worker::{
    NetworkStatus, NetworkStatusConfig, NetworkStatusConfigError, NetworkStatusError,
    NetworkStatusPoll, NetworkStatusWorker, NetworkStatusWorkerReport,
};
pub use preference_io::{
    PreferenceLoad, PreferenceLoadError, PreferenceLoadIssue, PreferencePathError, PreferencePaths,
};
pub use preference_worker::{
    BrightnessDevicePaths, BrightnessPathError, PreferenceSubmit, PreferenceWorker,
    PreferenceWorkerError, PreferenceWorkerReport, PreferenceWriteError, brightness_raw_value,
};
pub use preferences::{
    DashboardPreferences, EncodedPreference, MAXIMUM_PREFERENCE_BYTES, PreferenceField,
    PreferenceValueError, encode_setting, parse_brightness, parse_keymap, parse_volume,
};
pub use render::{
    ArtworkProvider, CANVAS_HEIGHT, CANVAS_WIDTH, Cover, CoverError, DashboardFrame, EntryButton,
    MenuLayout, NoArtwork, RenderError, RenderedScreen,
};
pub use settings::{NetworkView, SettingsLayout, SettingsView};
pub use state::{
    Action, Brightness, BrightnessError, DEFAULT_BRIGHTNESS_PERCENT, DEFAULT_VOLUME_PERCENT,
    DashboardModel, Intent, Keymap, MenuCue, Screen, SettingChange, SettingsTarget, Status,
    Transition, VolumeError, VolumeState,
};
pub use wifi::{
    MAXIMUM_PASSPHRASE_BYTES, MAXIMUM_SSID_BYTES, MINIMUM_PASSPHRASE_BYTES, WifiAction,
    WifiCredentials, WifiEditor, WifiEffect, WifiField, WifiStatus, WifiTransition,
};
pub use wifi_render::WifiLayout;
pub use wifi_session::WifiSession;
pub use wifi_writer::{
    WifiProfileWriter, WifiWriteError, WifiWriterPoll, WifiWriterReport, WifiWriterRequestId,
    WifiWriterResult, WifiWriterStartError, WifiWriterSubmit,
};
