//! Source-agnostic deck data model. Every source normalizes into these types.
//!
//! Fields populated only in later phases ([`Deck::archetype`], [`Deck::colors`],
//! [`Deck::est_price`]) are `Option` and stay `None` until then.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Competitive formats published on MTGO decklists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Standard,
    Modern,
    Pauper,
    Pioneer,
    Vintage,
    Legacy,
    Limited,
    DuelCommander,
    Premodern,
    Contraption,
}

/// Tournament event type. MTGO publishes only undefeated (5-0) league lists,
/// so [`EventType::League`] always means a 5-0 run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventType {
    League,
    Challenge,
    Preliminary,
    Other,
}

/// A deck's finishing result in its event. Fields are optional; sources vary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventResult {
    pub rank: Option<u32>,
    pub wins: Option<u32>,
    pub losses: Option<u32>,
}

/// A single Magic color (WUBRG).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Color {
    W,
    U,
    B,
    R,
    G,
}

/// One decklist line: a card name and copy count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardEntry {
    /// Canonical MTGO card name (validated against the reference set in Phase 1).
    pub name: String,
    pub quantity: u32,
}

/// A normalized decklist from any source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deck {
    /// Stable id derived from source + event + player + date.
    pub id: String,
    pub format: Format,
    /// Source identifier, e.g. `"wotc-mtgo"`.
    pub source: String,
    pub source_url: String,
    pub date: NaiveDate,
    pub event_type: EventType,
    pub result: EventResult,
    /// Archetype label; `None` until Phase 3.
    pub archetype: Option<String>,
    /// Color identity; `None` until Phase 3.
    pub colors: Option<BTreeSet<Color>>,
    pub player: Option<String>,
    pub maindeck: Vec<CardEntry>,
    pub sideboard: Vec<CardEntry>,
    /// Estimated MTGO price; `None` until Phase 4. Always approximate.
    pub est_price: Option<f64>,
}
