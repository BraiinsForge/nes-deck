//! Device-independent chiptune catalog, playback model, and renderer.

mod catalog;
#[cfg(feature = "chiptune-gme")]
mod gme;
mod model;
mod ogg;
mod render;

pub use catalog::{CatalogError, ChiptuneCatalog};
#[cfg(feature = "chiptune-gme")]
pub use gme::{GmeBlock, GmeDecoder, GmeDecoderError};
pub use model::{
    PlaybackMode, PlayerControl, PlayerEffect, PlayerModel, controller_control, touch_control,
};
pub use ogg::{OggBlock, OggDecoder, OggDecoderError};
pub use render::{
    CANVAS_HEIGHT, CANVAS_WIDTH, ChiptuneFrame, ChiptuneView, PlayerContent, RenderError, TrackView,
};
