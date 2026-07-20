//! Shared, side-effect-free configuration schemas for Retro Deck services.

mod catalog;
mod system;

pub use catalog::{
    Catalog, CatalogEntry, CatalogError, CatalogSystem, GameColor, MAXIMUM_CATALOG_BYTES,
    MAXIMUM_GAMES, MAXIMUM_IDENTIFIER_BYTES,
};
pub use system::{System, SystemError};
