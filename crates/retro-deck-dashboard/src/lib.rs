//! Native Retro Deck dashboard model, renderer, and runtime seams.

mod catalog;
mod credits;
mod render;
mod settings;
mod state;

pub use catalog::{Category, DashboardCatalog, DashboardCatalogError};
pub use credits::{CreditsCrawl, CreditsLayout};
pub use render::{
    ArtworkProvider, CANVAS_HEIGHT, CANVAS_WIDTH, Cover, CoverError, DashboardFrame, EntryButton,
    MenuLayout, NoArtwork, RenderError, RenderedScreen,
};
pub use settings::{NetworkView, SettingsLayout, SettingsView};
pub use state::{
    Action, Brightness, BrightnessError, DashboardModel, Intent, Keymap, MenuCue, Screen,
    SettingChange, SettingsTarget, Status, Transition, VolumeError, VolumeState,
};
