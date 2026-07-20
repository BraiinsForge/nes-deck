//! Shared, side-effect-free configuration schemas for Retro Deck services.

mod catalog;
mod palette;
mod system;

pub use catalog::{
    Catalog, CatalogEntry, CatalogError, CatalogSystem, GameColor, MAXIMUM_CATALOG_BYTES,
    MAXIMUM_GAMES, MAXIMUM_IDENTIFIER_BYTES,
};
pub use palette::{
    MAXIMUM_PALETTE_BYTES, Palette, PaletteError, PaletteField, PaletteRole, PaletteRoleError, Rgb,
};
pub use system::{System, SystemError};
