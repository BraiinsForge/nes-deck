//! Native Retro Deck dashboard model, renderer, and runtime seams.

#[cfg(feature = "application-wire")]
mod application;
#[cfg(feature = "bmc-native")]
mod bmc_ui;
mod catalog;
#[cfg(feature = "bmc-native")]
mod gamepad;
mod launch;
mod launch_options;
#[cfg(feature = "bmc-native")]
mod native_catalog;
#[cfg(feature = "bmc-native")]
mod native_cover;
#[cfg(feature = "bmc-native")]
mod native_theme;
mod state;

#[cfg(feature = "application-wire")]
pub use application::{
    ApplicationRequest, ApplicationRequestError, BMC_APPLICATION_ID,
    MAXIMUM_APPLICATION_INPUT_BYTES,
};
#[cfg(feature = "bmc-native")]
pub use bmc_ui::{
    BmcNavigation, BmcScreen, BmcUiAction, bmc_action_for_navigation, bmc_action_for_touch,
    build_bmc_tree,
};
pub use catalog::{Category, DashboardCatalog, DashboardCatalogError, MAXIMUM_DASHBOARD_ENTRIES};
#[cfg(feature = "bmc-native")]
pub use catalog::{
    DashboardApplicationPolicyError, DashboardStartupPolicyError, dashboard_startup_from_policy,
};
#[cfg(feature = "bmc-native")]
pub use gamepad::{GamepadInput, GamepadProfile, GamepadProfileError};
pub use launch::{
    ExitPolicy, LaunchPlan, LaunchPlanError, LaunchTarget, LaunchTargetError, TerminalMode,
};
pub use launch_options::{DEFAULT_VOLUME_PERCENT, Keymap, VolumeError, VolumeState};
#[cfg(feature = "bmc-native")]
pub use native_catalog::{NativeCatalogError, load_native_catalog};
#[cfg(feature = "bmc-native")]
pub use native_cover::{NATIVE_COVER_SIZE, NativeCover, load_native_cover};
#[cfg(feature = "bmc-native")]
pub use native_theme::load_native_palette;
pub use state::{Action, DashboardModel, Intent, MenuCue, Transition};
