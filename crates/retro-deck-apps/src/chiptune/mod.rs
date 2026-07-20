//! Device-independent chiptune catalog, playback model, and renderer.

mod catalog;
mod model;
mod ogg;
mod render;

pub use catalog::{CatalogError, ChiptuneCatalog};
pub use model::{
    PlaybackMode, PlayerControl, PlayerEffect, PlayerModel, controller_control, touch_control,
};
pub use ogg::{OggBlock, OggDecoder, OggDecoderError};
pub use render::{
    CANVAS_HEIGHT, CANVAS_WIDTH, ChiptuneFrame, ChiptuneView, PlayerContent, RenderError, TrackView,
};
