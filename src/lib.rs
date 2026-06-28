//! `mtgo_deckfinder` — find, rank, and export recent competitive MTGO decklists.
//!
//! Architecture: pure core, impure edges. Ranking and export logic are pure
//! functions, unit-testable without network or filesystem. Sources, caching,
//! and IO live behind traits at the edges.
//!
//! Phase 0 surface: the data model, a pure MTGO-text exporter, a JSON-backed
//! deck store behind the [`DeckStore`] trait, and a built-in sample deck.
//! Network sources, ranking, archetypes, and pricing arrive in later phases.

pub mod error;
pub mod export;
pub mod model;
pub mod sample;
pub mod store;

pub use error::{Error, Result};
pub use export::export_mtgo_txt;
pub use model::{CardEntry, Color, Deck, EventResult, EventType, Format};
pub use sample::sample_deck;
pub use store::{DeckStore, JsonStore};
