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

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EventType::League => "League",
            EventType::Challenge => "Challenge",
            EventType::Preliminary => "Preliminary",
            EventType::Other => "Other",
        })
    }
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

impl Color {
    /// Parse a single WUBRG letter (case-insensitive).
    pub fn from_letter(c: char) -> Option<Color> {
        match c.to_ascii_uppercase() {
            'W' => Some(Color::W),
            'U' => Some(Color::U),
            'B' => Some(Color::B),
            'R' => Some(Color::R),
            'G' => Some(Color::G),
            _ => None,
        }
    }

    pub fn letter(self) -> char {
        match self {
            Color::W => 'W',
            Color::U => 'U',
            Color::B => 'B',
            Color::R => 'R',
            Color::G => 'G',
        }
    }
}

/// Parse a color string like `"UR"` or `"wug"` into a set of colors.
pub fn parse_colors(s: &str) -> std::result::Result<std::collections::BTreeSet<Color>, String> {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            Color::from_letter(c).ok_or_else(|| format!("invalid color '{c}' (use W/U/B/R/G)"))
        })
        .collect()
}

/// Render a color set in WUBRG order, e.g. `"UR"` (or `"C"` when colorless).
pub fn colors_label(colors: &std::collections::BTreeSet<Color>) -> String {
    if colors.is_empty() {
        return "C".to_string();
    }
    // BTreeSet<Color> already iterates in WUBRG order (enum discriminant order).
    colors.iter().map(|c| c.letter()).collect()
}

/// How a deck's colors are compared against the requested colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMatch {
    /// Deck colors fit within the requested colors (deck ⊆ requested). Default.
    #[default]
    Subset,
    /// Deck colors are exactly the requested colors.
    Exact,
    /// Deck colors include all requested colors (deck ⊇ requested).
    Includes,
}

impl std::str::FromStr for ColorMatch {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_lowercase().as_str() {
            "subset" => Ok(ColorMatch::Subset),
            "exact" => Ok(ColorMatch::Exact),
            "includes" => Ok(ColorMatch::Includes),
            other => Err(format!(
                "invalid color-match '{other}' (use subset/exact/includes)"
            )),
        }
    }
}

/// Does `deck` satisfy the `requested` colors under `mode`?
pub fn color_matches(
    deck: &std::collections::BTreeSet<Color>,
    requested: &std::collections::BTreeSet<Color>,
    mode: ColorMatch,
) -> bool {
    match mode {
        ColorMatch::Subset => deck.is_subset(requested),
        ColorMatch::Exact => deck == requested,
        ColorMatch::Includes => deck.is_superset(requested),
    }
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

impl Format {
    /// Canonical lowercase token used in cache filenames and CLI input.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Standard => "standard",
            Format::Modern => "modern",
            Format::Pauper => "pauper",
            Format::Pioneer => "pioneer",
            Format::Vintage => "vintage",
            Format::Legacy => "legacy",
            Format::Limited => "limited",
            Format::DuelCommander => "duel-commander",
            Format::Premodern => "premodern",
            Format::Contraption => "contraption",
        }
    }

    /// Leading token of an mtgo.com decklist slug for this format, e.g.
    /// `modern-` in `/decklist/modern-challenge-32-2026-06-01...`.
    pub fn slug_prefix(self) -> &'static str {
        match self {
            // Slug uses `duel-` (commander events), not the full `duel-commander`.
            Format::DuelCommander => "duel-",
            Format::Standard => "standard-",
            Format::Modern => "modern-",
            Format::Pauper => "pauper-",
            Format::Pioneer => "pioneer-",
            Format::Vintage => "vintage-",
            Format::Legacy => "legacy-",
            Format::Limited => "limited-",
            Format::Premodern => "premodern-",
            Format::Contraption => "contraption-",
        }
    }
}

impl std::str::FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Ok(match s.to_lowercase().as_str() {
            "standard" => Format::Standard,
            "modern" => Format::Modern,
            "pauper" => Format::Pauper,
            "pioneer" => Format::Pioneer,
            "vintage" => Format::Vintage,
            "legacy" => Format::Legacy,
            "limited" => Format::Limited,
            "duel" | "duel-commander" | "duelcommander" => Format::DuelCommander,
            "premodern" => Format::Premodern,
            "contraption" => Format::Contraption,
            other => return Err(format!("unknown format: {other}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn colors(s: &str) -> BTreeSet<Color> {
        parse_colors(s).unwrap()
    }

    #[test]
    fn parses_and_renders_colors() {
        assert_eq!(colors("ur"), BTreeSet::from([Color::U, Color::R]));
        assert_eq!(colors_label(&colors("ru")), "UR"); // canonical WUBRG order
        assert_eq!(colors_label(&BTreeSet::new()), "C");
        assert!(parse_colors("ux").is_err());
    }

    #[test]
    fn color_match_modes() {
        let deck = colors("UR");
        assert!(color_matches(&deck, &colors("UR"), ColorMatch::Subset));
        assert!(color_matches(&deck, &colors("UWR"), ColorMatch::Subset)); // fits within
        assert!(!color_matches(&deck, &colors("U"), ColorMatch::Subset)); // R not allowed
        assert!(color_matches(&deck, &colors("UR"), ColorMatch::Exact));
        assert!(!color_matches(&deck, &colors("UWR"), ColorMatch::Exact));
        assert!(color_matches(&deck, &colors("U"), ColorMatch::Includes)); // contains U
        assert!(!color_matches(&deck, &colors("UG"), ColorMatch::Includes));
        // colorless deck fits within any requested colors
        assert!(color_matches(
            &BTreeSet::new(),
            &colors("UR"),
            ColorMatch::Subset
        ));
    }
}
