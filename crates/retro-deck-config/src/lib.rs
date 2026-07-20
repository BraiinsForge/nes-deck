//! Shared, side-effect-free configuration schemas for Retro Deck services.

mod catalog;
mod credits;
mod palette;
mod system;

pub use catalog::{
    Catalog, CatalogEntry, CatalogError, CatalogSystem, GameColor, MAXIMUM_CATALOG_BYTES,
    MAXIMUM_GAMES, MAXIMUM_IDENTIFIER_BYTES,
};
pub use credits::{Credits, CreditsError, MAXIMUM_CREDITS, MAXIMUM_CREDITS_BYTES, ProjectCredit};
pub use palette::{
    MAXIMUM_PALETTE_BYTES, Palette, PaletteError, PaletteField, PaletteRole, PaletteRoleError, Rgb,
};
pub use system::{System, SystemError};
