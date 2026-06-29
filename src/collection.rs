//! The user's owned MTGO cards, parsed from an MTGO collection export.
//!
//! ## Assumed export format (documented — adjust here if MTGO differs)
//! MTGO's client exports a collection as CSV. We assume a header row followed by
//! one row per card, with at least a card-name column (`Card Name` or `Name`) and
//! a quantity column (`Quantity` or `Qty`); other columns (set, rarity, premium,
//! id) are ignored. Foil and non-foil rows of the same card are summed, since for
//! deck-building any printing counts. The parser is tolerant of column order and
//! extra columns, and isolated here so it is easy to fix if the format changes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cards::lookup_key;
use crate::error::{Error, Result};

/// Owned-card counts, keyed like [`crate::cards`] so deck cards match exactly.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Collection {
    owned: HashMap<String, u32>,
}

impl Collection {
    /// How many copies of `name` the user owns (0 if none).
    pub fn owned(&self, name: &str) -> u32 {
        self.owned.get(&lookup_key(name)).copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.owned.is_empty()
    }

    /// Number of distinct cards owned.
    pub fn distinct_cards(&self) -> usize {
        self.owned.len()
    }

    /// Owned cards as `(lookup key, quantity)` pairs.
    pub fn entries(&self) -> impl Iterator<Item = (&str, u32)> {
        self.owned.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

/// Parse an MTGO collection CSV into a [`Collection`].
pub fn parse_collection_csv(text: &str) -> Result<Collection> {
    let mut rows = text.lines().filter(|l| !l.trim().is_empty());
    let header = rows
        .next()
        .ok_or_else(|| Error::Parse("empty collection file".into()))?;
    let cols = split_csv_line(header);
    let name_idx = find_column(&cols, &["card name", "name"])?;
    let qty_idx = find_column(&cols, &["quantity", "qty"])?;

    let mut owned: HashMap<String, u32> = HashMap::new();
    for row in rows {
        let fields = split_csv_line(row);
        let (Some(name), Some(qty)) = (fields.get(name_idx), fields.get(qty_idx)) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        *owned.entry(lookup_key(name)).or_insert(0) += qty.parse().unwrap_or(0);
    }
    Ok(Collection { owned })
}

fn find_column(cols: &[String], candidates: &[&str]) -> Result<usize> {
    cols.iter()
        .position(|c| candidates.contains(&c.to_lowercase().as_str()))
        .ok_or_else(|| Error::Parse(format!("collection CSV missing a {candidates:?} column")))
}

/// Split one CSV line, honoring `"quoted, fields"` and `""` escaped quotes.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => {
                    fields.push(std::mem::take(&mut cur).trim().to_string());
                }
                _ => cur.push(c),
            }
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = include_str!("../tests/fixtures/collection.csv");

    #[test]
    fn parses_names_quantities_and_sums_printings() {
        let c = parse_collection_csv(CSV).unwrap();
        assert_eq!(c.owned("Lightning Bolt"), 4);
        // foil + non-foil rows of the same card are summed
        assert_eq!(c.owned("Psychic Frog"), 3);
        // quoted name containing a comma
        assert_eq!(c.owned("Minsc, Beloved Ranger"), 1);
        // MTGO split name matches whether queried as A/B or A // B
        assert_eq!(c.owned("Wear/Tear"), 3);
        assert_eq!(c.owned("Wear // Tear"), 3);
        assert_eq!(c.owned("Not Owned"), 0);
    }

    #[test]
    fn errors_on_missing_columns() {
        assert!(parse_collection_csv("Set,Rarity\nMH3,Rare").is_err());
    }
}
