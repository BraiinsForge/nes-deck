//! Native Retro Deck dashboard model, renderer, and runtime seams.

mod artwork;
mod assets;
mod audio;
mod catalog;
mod controls;
mod credits;
mod launch;
mod preferences;
mod render;
mod settings;
mod state;

pub use artwork::{ArtworkError, ArtworkIssue, ArtworkReport, ArtworkStore, ArtworkStoreError};
pub use assets::{
    AssetKind, AssetPathError, CreditsFallback, DashboardAssetPaths, DashboardAssets,
    DashboardAssetsError, PaletteFallback,
};
pub use audio::menu_notes;
pub use catalog::{Category, DashboardCatalog, DashboardCatalogError, MAXIMUM_DASHBOARD_ENTRIES};
pub use controls::{ControllerGuard, TouchCommitter, controller_action};
pub use credits::{CreditsCrawl, CreditsLayout};
pub use launch::{LaunchTarget, LaunchTargetError, TerminalMode};
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
