//! Pure MTGO decklist exporter. No IO — fully unit-testable.

use std::fmt::Write;

use crate::model::Deck;

/// Render a deck as MTGO-importable text.
///
/// Format: one `"<qty> <name>"` line per maindeck card, then a blank line and
/// the same for the sideboard. The sideboard block (and its leading blank line)
/// is omitted when the sideboard is empty. Card names are emitted verbatim — the
/// caller supplies canonical MTGO names, so split (`Fire // Ice`), DFC, adventure,
/// and accented names all pass through unchanged.
pub fn export_mtgo_txt(deck: &Deck) -> String {
    let mut out = String::new();
    for card in &deck.maindeck {
        // Writing into a String is infallible; the Result is safe to drop.
        let _ = writeln!(out, "{} {}", card.quantity, card.name);
    }
    if !deck.sideboard.is_empty() {
        out.push('\n');
        for card in &deck.sideboard {
            let _ = writeln!(out, "{} {}", card.quantity, card.name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CardEntry;
    use crate::sample::sample_deck;

    fn card(quantity: u32, name: &str) -> CardEntry {
        CardEntry {
            name: name.to_string(),
            quantity,
        }
    }

    fn deck_with(maindeck: Vec<CardEntry>, sideboard: Vec<CardEntry>) -> Deck {
        // Reuse the sample for the fields the exporter ignores.
        let mut d = sample_deck();
        d.maindeck = maindeck;
        d.sideboard = sideboard;
        d
    }

    #[test]
    fn maindeck_then_blank_line_then_sideboard() {
        let d = deck_with(
            vec![card(4, "Lightning Bolt"), card(20, "Mountain")],
            vec![card(2, "Pyroblast")],
        );
        assert_eq!(
            export_mtgo_txt(&d),
            "4 Lightning Bolt\n20 Mountain\n\n2 Pyroblast\n"
        );
    }

    #[test]
    fn empty_sideboard_has_no_trailing_block() {
        let d = deck_with(vec![card(1, "Black Lotus")], vec![]);
        assert_eq!(export_mtgo_txt(&d), "1 Black Lotus\n");
    }

    #[test]
    fn special_card_names_pass_through_verbatim() {
        let d = deck_with(
            vec![
                card(2, "Fire // Ice"),       // split card
                card(4, "Delver of Secrets"), // DFC — front face only
                card(3, "Brazen Borrower"),   // adventure
                card(1, "Lim-Dûl's Vault"),   // accented name
            ],
            vec![],
        );
        assert_eq!(
            export_mtgo_txt(&d),
            "2 Fire // Ice\n4 Delver of Secrets\n3 Brazen Borrower\n1 Lim-Dûl's Vault\n"
        );
    }
}
