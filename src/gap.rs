//! Pure "what does it cost to complete this deck?" computation, against a loaded
//! collection and a price table. No IO — fully unit-testable.

use std::collections::HashMap;

use crate::collection::Collection;
use crate::model::Deck;
use crate::price::PriceTable;

/// How far a deck is from buildable, given what the user owns.
#[derive(Debug, Clone, PartialEq)]
pub struct GapInfo {
    /// Total copies in the deck the user does not own (quantity-aware: needing a
    /// 4th copy when 3 are owned is a gap of 1).
    pub cards_missing: u32,
    /// Estimated tix to buy only the missing copies (approximate).
    pub cost_to_complete: f64,
    /// True when nothing is missing.
    pub buildable_now: bool,
}

/// Compute the gap for one deck. Requirements are aggregated by card across the
/// maindeck and sideboard before comparing to owned copies.
pub fn deck_gap(deck: &Deck, collection: &Collection, prices: &PriceTable) -> GapInfo {
    let mut needed: HashMap<&str, u32> = HashMap::new();
    for card in deck.maindeck.iter().chain(&deck.sideboard) {
        *needed.entry(card.name.as_str()).or_insert(0) += card.quantity;
    }

    let mut cards_missing = 0;
    let mut cost_to_complete = 0.0;
    for (name, total) in needed {
        let missing = total.saturating_sub(collection.owned(name));
        cards_missing += missing;
        cost_to_complete += f64::from(missing) * prices.get(name).unwrap_or(0.0);
    }

    GapInfo {
        cards_missing,
        cost_to_complete,
        buildable_now: cards_missing == 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::parse_collection_csv;
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

    fn prices() -> PriceTable {
        PriceTable::from_pairs([
            ("Lightning Bolt".to_string(), 0.02),
            ("Psychic Frog".to_string(), 4.00),
            ("Force of Negation".to_string(), 3.00),
        ])
    }

    #[test]
    fn counts_missing_copies_quantity_aware() {
        // Collection owns: Lightning Bolt 4, Psychic Frog 3 (see fixture).
        let coll = parse_collection_csv(include_str!("../tests/fixtures/collection.csv")).unwrap();
        // Deck needs 4 Bolt (owned 4 → 0 missing), 4 Frog (owned 3 → 1 missing),
        // 2 Force of Negation (owned 0 → 2 missing).
        let d = deck(
            &[("Lightning Bolt", 4), ("Psychic Frog", 4)],
            &[("Force of Negation", 2)],
        );
        let gap = deck_gap(&d, &coll, &prices());

        assert_eq!(gap.cards_missing, 3); // 1 Frog + 2 Force
        // cost = 1*4.00 + 2*3.00 = 10.00
        assert!((gap.cost_to_complete - 10.00).abs() < 1e-9);
        assert!(!gap.buildable_now);
    }

    #[test]
    fn buildable_when_nothing_missing() {
        let coll = parse_collection_csv(include_str!("../tests/fixtures/collection.csv")).unwrap();
        let d = deck(&[("Lightning Bolt", 4)], &[("Psychic Frog", 2)]);
        let gap = deck_gap(&d, &coll, &prices());
        assert_eq!(gap.cards_missing, 0);
        assert!((gap.cost_to_complete).abs() < 1e-9);
        assert!(gap.buildable_now);
    }
}
