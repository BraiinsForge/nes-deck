//! Native Retro Deck dashboard model, renderer, and runtime seams.

mod assets;
mod catalog;
mod controls;
mod credits;
mod launch;
mod render;
mod settings;
mod state;

pub use assets::{
    AssetKind, AssetPathError, CreditsFallback, DashboardAssetPaths, DashboardAssets,
    DashboardAssetsError, PaletteFallback,
};
pub use catalog::{Category, DashboardCatalog, DashboardCatalogError, MAXIMUM_DASHBOARD_ENTRIES};
pub use controls::{ControllerGuard, TouchCommitter, controller_action};
pub use credits::{CreditsCrawl, CreditsLayout};
pub use launch::{LaunchTarget, LaunchTargetError, TerminalMode};
pub use render::{
    ArtworkProvider, CANVAS_HEIGHT, CANVAS_WIDTH, Cover, CoverError, DashboardFrame, EntryButton,
    MenuLayout, NoArtwork, RenderError, RenderedScreen,
};
pub use settings::{NetworkView, SettingsLayout, SettingsView};
pub use state::{
    Action, Brightness, BrightnessError, DashboardModel, Intent, Keymap, MenuCue, Screen,
    SettingChange, SettingsTarget, Status, Transition, VolumeError, VolumeState,
};
