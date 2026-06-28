//! Card reference: name validation, color identity, and land flags, built from
//! the local **MTGJSON** `AtomicCards` bulk file (downloaded once, checksum-
//! verified, cached). The same reference answers three questions:
//!
//! - is this a real MTGO card name? (export must never emit an unknown name)
//! - what is a card's color identity? (to derive each deck's colors)
//! - is it a land? (so archetype labels skip the mana base)
//!
//! MTGO names differ from MTGJSON's full names for multi-faced cards (MTGO uses
//! the front face for DFCs/adventures and `A/B` for splits, MTGJSON always uses
//! `A // B`). We index the full name *and* each ` // ` component, mapping every
//! face to the whole card's info.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::error::Result;
use crate::model::{CardEntry, Color};

/// Normalize a card name for storage/display: NFC-normalize accents, trim, and
/// collapse internal whitespace. Case is preserved.
pub fn normalize_name(name: &str) -> String {
    let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.nfc().collect()
}

/// Lookup key: normalized, lowercased, separator-canonicalized (MTGJSON `A // B`
/// and MTGO `A/B` both map to `a/b`).
fn key(name: &str) -> String {
    normalize_name(name).to_lowercase().replace(" // ", "/")
}

/// The full name plus each ` // ` component (front face, split sides).
fn faces(name: &str) -> impl Iterator<Item = &str> {
    std::iter::once(name).chain(name.split(" // ").filter(move |p| *p != name))
}

/// Per-card facts used by the rest of the tool.
#[derive(Debug, Clone)]
pub struct CardInfo {
    pub colors: BTreeSet<Color>,
    pub is_land: bool,
}

/// One row of the cached reference (a full MTGJSON card name + its facts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardRecord {
    pub name: String,
    pub colors: Vec<Color>,
    pub is_land: bool,
}

/// Card lookup by name, keyed for case/separator-insensitive matching.
pub struct CardReference {
    cards: HashMap<String, CardInfo>,
}

impl CardReference {
    /// Build from cached records (re-keys on load, so key logic can change
    /// without re-downloading).
    pub fn from_records(records: &[CardRecord]) -> Self {
        let mut cards = HashMap::new();
        for r in records {
            let info = CardInfo {
                colors: r.colors.iter().copied().collect(),
                is_land: r.is_land,
            };
            for face in faces(&r.name) {
                cards.entry(key(face)).or_insert_with(|| info.clone());
            }
        }
        Self { cards }
    }

    /// Build from a streamed MTGJSON `AtomicCards` JSON reader.
    pub fn from_atomic_reader<R: Read>(reader: R) -> Result<Self> {
        Ok(Self::from_records(&read_atomic_records(reader)?))
    }

    pub fn is_valid(&self, name: &str) -> bool {
        self.cards.contains_key(&key(name))
    }

    pub fn colors(&self, name: &str) -> Option<&BTreeSet<Color>> {
        self.cards.get(&key(name)).map(|i| &i.colors)
    }

    pub fn is_land(&self, name: &str) -> bool {
        self.cards.get(&key(name)).is_some_and(|i| i.is_land)
    }

    /// A deck's color identity: the union over its maindeck cards' colors.
    pub fn deck_colors(&self, maindeck: &[CardEntry]) -> BTreeSet<Color> {
        let mut out = BTreeSet::new();
        for card in maindeck {
            if let Some(colors) = self.colors(&card.name) {
                out.extend(colors.iter().copied());
            }
        }
        out
    }
}

// ---- MTGJSON AtomicCards: { "data": { "Card Name": [ {colorIdentity, types} ] } } ----

#[derive(Deserialize)]
struct AtomicFile {
    data: HashMap<String, Vec<AtomicCard>>,
}

#[derive(Deserialize)]
struct AtomicCard {
    #[serde(rename = "colorIdentity", default)]
    color_identity: Vec<String>,
    #[serde(default)]
    types: Vec<String>,
}

fn read_atomic_records<R: Read>(reader: R) -> Result<Vec<CardRecord>> {
    let file: AtomicFile = serde_json::from_reader(reader)?;
    let mut out = Vec::with_capacity(file.data.len());
    for (name, printings) in file.data {
        let card = printings.first();
        let colors = card
            .map(|c| {
                c.color_identity
                    .iter()
                    .filter_map(|s| Color::from_letter(s.chars().next()?))
                    .collect()
            })
            .unwrap_or_default();
        let is_land = card.is_some_and(|c| c.types.iter().any(|t| t == "Land"));
        out.push(CardRecord {
            name,
            colors,
            is_land,
        });
    }
    Ok(out)
}

/// MTGJSON `AtomicCards` (gzip) — the local card source of truth.
const ATOMIC_URL: &str = "https://mtgjson.com/api/v5/AtomicCards.json.gz";

/// Download MTGJSON `AtomicCards`, verify it against its published `.sha256`,
/// decompress, and return the card records. ~50 MB; callers cache the result.
pub fn download_atomic_cards(client: &crate::http::PoliteClient) -> Result<Vec<CardRecord>> {
    use sha2::{Digest, Sha256};

    let expected = client
        .get_text(&format!("{ATOMIC_URL}.sha256"))?
        .trim()
        .to_string();
    let gz = client.get_bytes(ATOMIC_URL)?;
    let actual = to_hex(Sha256::digest(&gz).as_slice());
    if actual != expected {
        return Err(crate::error::Error::Checksum(ATOMIC_URL.to_string()));
    }
    let reader = flate2::read::GzDecoder::new(std::io::Cursor::new(gz));
    read_atomic_records(reader)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATOMIC: &str = r#"{"data":{
        "Fire // Ice":[{"colorIdentity":["U","R"],"types":["Instant"]}],
        "Steam Vents":[{"colorIdentity":["U","R"],"types":["Land"]}],
        "Lightning Bolt":[{"colorIdentity":["R"],"types":["Instant"]}],
        "Delver of Secrets // Insectile Aberration":[{"colorIdentity":["U"],"types":["Creature"]}],
        "Ornithopter":[{"colorIdentity":[],"types":["Artifact","Creature"]}]
    }}"#;

    #[test]
    fn normalizes_whitespace_and_accents() {
        assert_eq!(normalize_name("  Lightning   Bolt "), "Lightning Bolt");
        assert_eq!(normalize_name("Lim-Du\u{0302}l's Vault"), "Lim-Dûl's Vault");
    }

    #[test]
    fn validates_names_including_split_and_dfc_faces() {
        let r = CardReference::from_atomic_reader(ATOMIC.as_bytes()).unwrap();
        assert!(r.is_valid("Fire // Ice"));
        assert!(r.is_valid("Fire/Ice")); // MTGO slash form
        assert!(r.is_valid("Fire")); // split side
        assert!(r.is_valid("Delver of Secrets")); // DFC front face
        assert!(!r.is_valid("Not A Real Card"));
    }

    #[test]
    fn reads_colors_and_land_flags() {
        let r = CardReference::from_atomic_reader(ATOMIC.as_bytes()).unwrap();
        assert_eq!(
            r.colors("Lightning Bolt").unwrap(),
            &BTreeSet::from([Color::R])
        );
        assert_eq!(
            r.colors("Fire/Ice").unwrap(),
            &BTreeSet::from([Color::U, Color::R])
        );
        assert!(r.colors("Ornithopter").unwrap().is_empty());
        assert!(r.is_land("Steam Vents"));
        assert!(!r.is_land("Lightning Bolt"));
    }

    #[test]
    fn deck_colors_is_union_over_maindeck() {
        let r = CardReference::from_atomic_reader(ATOMIC.as_bytes()).unwrap();
        let deck = vec![
            CardEntry {
                name: "Lightning Bolt".into(),
                quantity: 4,
            },
            CardEntry {
                name: "Steam Vents".into(),
                quantity: 4,
            },
            CardEntry {
                name: "Ornithopter".into(),
                quantity: 2,
            },
        ];
        assert_eq!(r.deck_colors(&deck), BTreeSet::from([Color::U, Color::R]));
    }
}
