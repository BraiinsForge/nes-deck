//! Native Retro Deck dashboard model, renderer, and runtime seams.

mod catalog;
mod state;

pub use catalog::{Category, DashboardCatalog, DashboardCatalogError};
pub use state::{
    Action, Brightness, BrightnessError, DashboardModel, Intent, Keymap, MenuCue, Screen,
    SettingChange, SettingsTarget, Status, Transition, VolumeError, VolumeState,
};
