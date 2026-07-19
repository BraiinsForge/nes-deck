//! Linux platform adapters kept separate from application state and policy.

pub mod audio;
pub mod display;
pub mod input;
pub mod shutdown;
pub mod time;
pub mod wayland;
pub mod wayland_protocol;

/// Logical width exposed by the Deck compositor and touchscreen.
pub const DECK_LOGICAL_WIDTH: u16 = 1_280;
/// Logical height exposed by the Deck compositor and touchscreen.
pub const DECK_LOGICAL_HEIGHT: u16 = 480;
