//! Approximate MTGO prices (in **tix**), sourced from **Scryfall** card data.
//!
//! All prices are estimates and labeled as such everywhere they surface. Prices
//! come from Scryfall's `prices.tix` via its batch `/cards/collection` endpoint
//! (≤75 names per request) — a targeted lookup of only the cards in the cached
//! decks, not a bulk download.
//!
//! GoatBots (the other candidate source) is intentionally *not* used: its price
//! files sit under a `robots.txt`-disallowed `/download/` path behind a
//! Cloudflare challenge, so fetching them would violate the project's polite-
//! sourcing constraints.

use std::collections::HashMap;

use serde::Deserialize;

use crate::cards::{card_keys, lookup_key};
use crate::error::Result;
use crate::http::PoliteClient;
use crate::model::Deck;

const COLLECTION_URL: &str = "https://api.scryfall.com/cards/collection";
const BATCH: usize = 75; // Scryfall's max identifiers per request.

/// Name-keyed MTGO price table (tix). Keys match [`crate::cards`] exactly, so a
/// deck card looks up the same way it validates.
pub struct PriceTable {
    tix: HashMap<String, f64>,
}

impl PriceTable {
    /// Build from `(card name, tix)` pairs (names may be split `A // B` forms).
    pub fn from_pairs<I: IntoIterator<Item = (String, f64)>>(pairs: I) -> Self {
        let mut tix = HashMap::new();
        for (name, price) in pairs {
            for k in card_keys(&name) {
                tix.entry(k).or_insert(price);
            }
        }
        Self { tix }
    }

    pub fn is_empty(&self) -> bool {
        self.tix.is_empty()
    }

    /// Estimated tix price of one card, if known.
    pub fn get(&self, name: &str) -> Option<f64> {
        self.tix.get(&lookup_key(name)).copied()
    }

    /// Estimated total tix for a whole deck (maindeck + sideboard). Cards without
    /// a known price contribute 0 — the total is approximate.
    pub fn deck_price(&self, deck: &Deck) -> f64 {
        deck.maindeck
            .iter()
            .chain(&deck.sideboard)
            .map(|c| self.get(&c.name).unwrap_or(0.0) * f64::from(c.quantity))
            .sum()
    }
}

// ---- Scryfall batch fetch ----

#[derive(Deserialize)]
struct CollectionResponse {
    data: Vec<ScryCard>,
}

#[derive(Deserialize)]
struct ScryCard {
    name: String,
    #[serde(default)]
    prices: ScryPrices,
}

#[derive(Deserialize, Default)]
struct ScryPrices {
    #[serde(default)]
    tix: Option<String>,
}

/// Scryfall stores split cards as `A // B`; MTGO writes `A/B`. Convert for the
/// query (DFC/adventure front names match as-is).
fn scryfall_name(mtgo: &str) -> String {
    mtgo.replace('/', " // ")
}

/// Fetch tix prices for the given card names from Scryfall, in batches.
pub fn fetch_prices(client: &PoliteClient, names: &[String]) -> Result<Vec<(String, f64)>> {
    let mut out = Vec::new();
    for chunk in names.chunks(BATCH) {
        let identifiers: Vec<_> = chunk.iter().map(|n| json_name(&scryfall_name(n))).collect();
        let body = format!("{{\"identifiers\":[{}]}}", identifiers.join(","));
        let resp: CollectionResponse = client.post_json(COLLECTION_URL, body)?;
        for card in resp.data {
            if let Some(tix) = card
                .prices
                .tix
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
            {
                out.push((card.name, tix));
            }
        }
    }
    Ok(out)
}

/// Minimal JSON object `{"name":"..."}` with the name string escaped.
fn json_name(name: &str) -> String {
    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    format!("{{\"name\":\"{escaped}\"}}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CardEntry, EventResult, EventType, Format};

    fn deck(main: &[(&str, u32)], side: &[(&str, u32)]) -> Deck {
        let entry = |(n, q): &(&str, u32)| CardEntry {
            name: (*n).into(),
            quantity: *q,
        };
        Deck {
            id: "t".into(),
            format: Format::Modern,
            source: "wotc-mtgo".into(),
            source_url: String::new(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 6, 28).unwrap(),
            event_type: EventType::Challenge,
            result: EventResult::default(),
            archetype: None,
            colors: None,
            player: None,
            maindeck: main.iter().map(entry).collect(),
            sideboard: side.iter().map(entry).collect(),
            est_price: None,
        }
    }

    #[test]
    fn sums_deck_price_over_main_and_sideboard() {
        let prices = PriceTable::from_pairs([
            ("Lightning Bolt".to_string(), 0.02),
            ("Wear // Tear".to_string(), 4.98),
            ("Unpriced Card".to_string(), 1.0), // present but not in the deck
        ]);
        // MTGO writes the split as "Wear/Tear"; it must still match.
        let d = deck(
            &[("Lightning Bolt", 4), ("Wear/Tear", 2)],
            &[("Lightning Bolt", 1)],
        );
        // 5 * 0.02 + 2 * 4.98 = 0.10 + 9.96 = 10.06
        assert!((prices.deck_price(&d) - 10.06).abs() < 1e-9);
    }

    #[test]
    fn unknown_cards_contribute_zero() {
        let prices = PriceTable::from_pairs([("Lightning Bolt".to_string(), 0.02)]);
        let d = deck(&[("Lightning Bolt", 4), ("Brand New Card", 4)], &[]);
        assert!((prices.deck_price(&d) - 0.08).abs() < 1e-9);
        assert_eq!(prices.get("Brand New Card"), None);
    }
}
