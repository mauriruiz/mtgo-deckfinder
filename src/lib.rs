//! `mtgo_deckfinder` — find, rank, and export recent competitive MTGO decklists.
//!
//! Architecture: pure core, impure edges. Ranking and export logic are pure
//! functions, unit-testable without network or filesystem. Sources, caching,
//! and IO live behind traits at the edges ([`DeckSource`], [`DeckStore`]).
//!
//! Phase 1 surface: the data model, a pure MTGO-text exporter, a real
//! [`WotcMtgoSource`] over mtgo.com (JSON path), MTGJSON-backed card-name
//! validation ([`NameReference`]), and a timestamped disk [`Cache`].

pub mod cache;
pub mod cards;
pub mod cluster;
pub mod error;
pub mod export;
pub mod http;
pub mod model;
pub mod rank;
pub mod sample;
pub mod source;
pub mod store;

pub use cache::{Cache, Cached};
pub use cards::{CardInfo, CardRecord, CardReference, download_atomic_cards, normalize_name};
pub use cluster::{Clustering, SIMILARITY_THRESHOLD, cluster_decks};
pub use error::{Error, Result};
pub use export::export_mtgo_txt;
pub use model::{
    CardEntry, Color, ColorMatch, Deck, EventResult, EventType, Format, color_matches,
    colors_label, parse_colors,
};
pub use rank::{DEFAULT_WEIGHTS, Scored, Weights, rank_decks, score};
pub use sample::sample_deck;
pub use source::{DeckSource, WotcMtgoSource};
pub use store::{DeckStore, JsonStore};

/// HTTP User-Agent sent on every request: names the app, version, and repo, as
/// required by Scryfall and good manners for the other sources.
pub const USER_AGENT: &str = concat!(
    "mtgo-deckfinder/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/mauriruiz/mtgo-deckfinder)"
);
