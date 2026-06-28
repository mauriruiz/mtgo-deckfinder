//! Local deck store: the [`DeckStore`] trait plus a flat-JSON-file impl.
//!
//! JSON over SQLite for the MVP: no native/C dependency, a human-inspectable
//! cache, and the working set (a few hundred decks per format) loads whole in
//! milliseconds. Swap in a SQLite-backed `DeckStore` if query/scale ever demands
//! it — nothing in the core touches this trait's implementation.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::model::Deck;

/// A persistent collection of decks. Lives at the impure edge; core logic never
/// touches it directly.
pub trait DeckStore {
    /// Load all stored decks, or an empty vec if nothing has been saved yet.
    fn load(&self) -> Result<Vec<Deck>>;
    /// Replace the stored decks with `decks`.
    fn save(&self, decks: &[Deck]) -> Result<()>;
}

/// Stores all decks as a single pretty-printed JSON array on disk.
pub struct JsonStore {
    path: PathBuf,
}

impl JsonStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl DeckStore for JsonStore {
    fn load(&self) -> Result<Vec<Deck>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&fs::read_to_string(&self.path)?)?)
    }

    fn save(&self, decks: &[Deck]) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(decks)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_deck;

    #[test]
    fn round_trips_through_disk() {
        let path = std::env::temp_dir()
            .join(format!("mtgo-deckfinder-{}", std::process::id()))
            .join("decks.json");
        let store = JsonStore::new(&path);

        assert!(store.load().unwrap().is_empty(), "no file yet => empty");

        let decks = vec![sample_deck()];
        store.save(&decks).unwrap();
        assert_eq!(store.load().unwrap(), decks);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
