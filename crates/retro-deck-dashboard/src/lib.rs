//! Native Retro Deck dashboard model, renderer, and runtime seams.

mod catalog;
mod render;
mod state;

pub use catalog::{Category, DashboardCatalog, DashboardCatalogError};
pub use render::{
    ArtworkProvider, CANVAS_HEIGHT, CANVAS_WIDTH, Cover, CoverError, DashboardFrame, EntryButton,
    MenuLayout, NoArtwork, RenderError,
};
pub use state::{
    Action, Brightness, BrightnessError, DashboardModel, Intent, Keymap, MenuCue, Screen,
    SettingChange, SettingsTarget, Status, Transition, VolumeError, VolumeState,
};
