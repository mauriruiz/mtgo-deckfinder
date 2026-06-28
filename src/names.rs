//! Card-name normalization and validation against a local reference set.
//!
//! Card-name correctness is the #1 integration risk: export must never emit a
//! name MTGO rejects. The reference set is built from the **MTGJSON**
//! `AtomicCards` bulk file (downloaded once, checksum-verified, cached) — we do
//! not iterate live Scryfall calls over many names.
//!
//! MTGO names differ from MTGJSON's full names for multi-faced cards: MTGO uses
//! the front face only for DFCs/adventures (`Brazen Borrower`) and `A // B` for
//! split cards (`Fire // Ice`). MTGJSON's name is always the full `A // B` form,
//! so we index the full name *and* each ` // ` component — that single split
//! covers DFC front faces, split sides, and the joined split name at once.

use std::collections::HashSet;
use std::io::Read;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::error::Result;

/// Normalize a card name for storage/display: NFC-normalize accents, trim, and
/// collapse internal runs of whitespace. Case is preserved (MTGO names are
/// properly cased at the source).
pub fn normalize_name(name: &str) -> String {
    let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.nfc().collect()
}

/// Lookup key: normalized, lowercased, separator-canonicalized. Validation is
/// case-insensitive, and the split-card separator is unified — MTGJSON writes
/// `A // B`, MTGO writes `A/B`; both map to `a/b`.
fn key(name: &str) -> String {
    normalize_name(name).to_lowercase().replace(" // ", "/")
}

/// The full name plus each ` // ` component (front face, split sides).
fn faces(name: &str) -> impl Iterator<Item = &str> {
    std::iter::once(name).chain(name.split(" // ").filter(move |p| *p != name))
}

/// A set of legal MTGO card names to validate decklists against.
pub struct NameReference {
    keys: HashSet<String>,
}

/// Minimal view of MTGJSON `AtomicCards`: the `data` map's keys are the card
/// names; values are skipped without allocation via `IgnoredAny`.
#[derive(Deserialize)]
struct AtomicFile {
    data: std::collections::HashMap<String, serde::de::IgnoredAny>,
}

impl NameReference {
    /// Build from any iterator of card names (full names may contain ` // `).
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut keys = HashSet::new();
        for name in names {
            for face in faces(name.as_ref()) {
                keys.insert(key(face));
            }
        }
        Self { keys }
    }

    /// Build from a streamed MTGJSON `AtomicCards` JSON reader.
    pub fn from_atomic_reader<R: Read>(reader: R) -> Result<Self> {
        Ok(Self::from_names(read_atomic_names(reader)?))
    }

    /// Is `name` a known legal card name?
    pub fn is_valid(&self, name: &str) -> bool {
        self.keys.contains(&key(name))
    }
}

/// Extract the raw card names (the `data` map keys) from an MTGJSON
/// `AtomicCards` JSON reader. Cached verbatim so key-derivation can change
/// without re-downloading.
fn read_atomic_names<R: Read>(reader: R) -> Result<Vec<String>> {
    let file: AtomicFile = serde_json::from_reader(reader)?;
    Ok(file.data.into_keys().collect())
}

/// MTGJSON `AtomicCards` (gzip) — the local card-name source of truth.
const ATOMIC_URL: &str = "https://mtgjson.com/api/v5/AtomicCards.json.gz";

/// Download MTGJSON `AtomicCards`, verify it against its published `.sha256`,
/// decompress, and return the raw card names. ~50 MB; callers cache the result.
pub fn download_atomic_names(client: &crate::http::PoliteClient) -> Result<Vec<String>> {
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
    read_atomic_names(reader)
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

    const ATOMIC: &str = include_str!("../tests/fixtures/atomic_cards_sample.json");

    #[test]
    fn normalizes_whitespace_and_accents() {
        assert_eq!(normalize_name("  Lightning   Bolt "), "Lightning Bolt");
        // precomposed output regardless of input form
        assert_eq!(normalize_name("Lim-Du\u{0302}l's Vault"), "Lim-Dûl's Vault");
    }

    #[test]
    fn builds_reference_and_validates() {
        let r = NameReference::from_atomic_reader(ATOMIC.as_bytes()).unwrap();
        assert!(r.is_valid("Fire // Ice")); // split, MTGJSON form
        assert!(r.is_valid("Fire/Ice")); // split, MTGO slash form
        assert!(r.is_valid("Fire")); // split side
        assert!(r.is_valid("Ice")); // split side
        assert!(r.is_valid("Delver of Secrets")); // DFC front face
        assert!(r.is_valid("Brazen Borrower")); // adventure front
        assert!(r.is_valid("  lim-dûl's   vault  ")); // case + whitespace insensitive
        assert!(!r.is_valid("Not A Real Card"));
    }

    #[test]
    fn rebuilds_from_cached_raw_names() {
        let raw = read_atomic_names(ATOMIC.as_bytes()).unwrap();
        let r = NameReference::from_names(&raw);
        assert!(r.is_valid("Fire/Ice")); // MTGO slash form
        assert!(r.is_valid("Delver of Secrets"));
    }
}
