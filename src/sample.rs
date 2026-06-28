//! Built-in sample deck for `export --sample` and as a test fixture. No network.

use chrono::NaiveDate;

use crate::model::{CardEntry, Deck, EventResult, EventType, Format};

fn card(quantity: u32, name: &str) -> CardEntry {
    CardEntry {
        name: name.to_string(),
        quantity,
    }
}

/// A small, recognizable Pauper "Kuldotha Red" list (60 main / 15 side), used as
/// the built-in fixture so the pipeline is demonstrable without any data source.
pub fn sample_deck() -> Deck {
    Deck {
        id: "sample-kuldotha-red-pauper".to_string(),
        format: Format::Pauper,
        source: "sample".to_string(),
        source_url: "https://example.invalid/sample".to_string(),
        date: NaiveDate::from_ymd_opt(2026, 6, 1).expect("valid date literal"),
        event_type: EventType::League,
        result: EventResult {
            rank: Some(1),
            wins: Some(5),
            losses: Some(0),
        },
        archetype: None,
        colors: None,
        player: Some("sample-player".to_string()),
        maindeck: vec![
            card(4, "Kuldotha Rebirth"),
            card(4, "Goblin Bushwhacker"),
            card(4, "Voldaren Epicure"),
            card(4, "Experimental Synthesizer"),
            card(4, "Galvanic Blast"),
            card(4, "Lightning Bolt"),
            card(2, "Fireblast"),
            card(4, "Goblin Tomb Raider"),
            card(4, "Kessig Flamebreather"),
            card(2, "Reckless Abandon"),
            card(4, "Implement of Combustion"),
            card(4, "Great Furnace"),
            card(16, "Mountain"),
        ],
        sideboard: vec![
            card(3, "Pyroblast"),
            card(2, "Red Elemental Blast"),
            card(3, "Relic of Progenitus"),
            card(2, "Smash to Smithereens"),
            card(2, "Faithless Looting"),
            card(3, "Electrickery"),
        ],
        est_price: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_is_a_legal_60_15_deck() {
        let main: u32 = sample_deck().maindeck.iter().map(|c| c.quantity).sum();
        let side: u32 = sample_deck().sideboard.iter().map(|c| c.quantity).sum();
        assert_eq!(main, 60);
        assert_eq!(side, 15);
    }
}
