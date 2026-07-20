//! Native Retro Deck dashboard model, renderer, and runtime seams.

#[cfg(feature = "application-wire")]
mod application;
#[cfg(feature = "bmc-native")]
mod bmc_ui;
mod catalog;
mod launch;
#[cfg(feature = "bmc-native")]
mod native_catalog;
mod state;

#[cfg(feature = "application-wire")]
pub use application::{
    ApplicationRequest, ApplicationRequestError, BMC_APPLICATION_ID,
    MAXIMUM_APPLICATION_INPUT_BYTES,
};
#[cfg(feature = "bmc-native")]
pub use bmc_ui::{BmcScreen, BmcUiAction, bmc_action_for_touch, build_bmc_tree};
pub use catalog::{Category, DashboardCatalog, DashboardCatalogError, MAXIMUM_DASHBOARD_ENTRIES};
#[cfg(feature = "bmc-native")]
pub use catalog::{DashboardApplicationPolicyError, applications_from_policy};
pub use launch::{
    ExitPolicy, LaunchPlan, LaunchPlanError, LaunchTarget, LaunchTargetError, TerminalMode,
};
#[cfg(feature = "bmc-native")]
pub use native_catalog::{NativeCatalogError, load_native_catalog};
pub use state::{
    Action, Brightness, BrightnessError, DEFAULT_BRIGHTNESS_PERCENT, DEFAULT_VOLUME_PERCENT,
    DashboardModel, Intent, Keymap, MenuCue, Screen, SettingChange, SettingsTarget, Status,
    Transition, VolumeError, VolumeState,
};
