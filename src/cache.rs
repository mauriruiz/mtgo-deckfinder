//! Timestamped disk cache. Cache-first by default: fetched data is reused until
//! it goes stale or `--refresh` is passed. Lives under the per-OS cache dir.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::model::{Deck, Format};

/// A cached payload tagged with when it was fetched.
#[derive(Serialize, serde::Deserialize)]
pub struct Cached<T> {
    pub fetched_at: DateTime<Utc>,
    pub data: T,
}

impl<T> Cached<T> {
    /// True if older than `ttl` relative to `now`.
    pub fn is_stale(&self, ttl: Duration, now: DateTime<Utc>) -> bool {
        now.signed_duration_since(self.fetched_at) > ttl
    }
}

/// A directory-backed JSON cache.
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    /// `<os-cache-dir>/mtgo-deckfinder`.
    pub fn default_location() -> Result<Self> {
        let base = dirs::cache_dir()
            .ok_or_else(|| Error::Parse("could not determine OS cache directory".into()))?;
        Ok(Self::new(base.join("mtgo-deckfinder")))
    }

    fn read<T: DeserializeOwned>(&self, file: &str) -> Result<Option<Cached<T>>> {
        let path = self.dir.join(file);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
    }

    fn write<T: Serialize>(&self, file: &str, data: T, now: DateTime<Utc>) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let wrapped = Cached {
            fetched_at: now,
            data,
        };
        fs::write(self.dir.join(file), serde_json::to_string_pretty(&wrapped)?)?;
        Ok(())
    }

    pub fn read_decks(&self, format: Format) -> Result<Option<Cached<Vec<Deck>>>> {
        self.read(&decks_file(format))
    }

    pub fn write_decks(&self, format: Format, decks: &[Deck], now: DateTime<Utc>) -> Result<()> {
        self.write(&decks_file(format), decks, now)
    }

    pub fn read_names(&self) -> Result<Option<Cached<Vec<String>>>> {
        self.read("card-names.json")
    }

    pub fn write_names(&self, keys: &[String], now: DateTime<Utc>) -> Result<()> {
        self.write("card-names.json", keys, now)
    }
}

fn decks_file(format: Format) -> String {
    format!("decks-{}.json", format.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_deck;

    #[test]
    fn round_trips_and_reports_staleness() {
        let dir = std::env::temp_dir().join(format!("mtgo-df-cache-{}", std::process::id()));
        let cache = Cache::new(&dir);
        let now = Utc::now();

        assert!(cache.read_decks(Format::Modern).unwrap().is_none());

        let decks = vec![sample_deck()];
        cache.write_decks(Format::Modern, &decks, now).unwrap();

        let cached = cache.read_decks(Format::Modern).unwrap().unwrap();
        assert_eq!(cached.data, decks);
        assert!(!cached.is_stale(Duration::hours(12), now));
        assert!(cached.is_stale(Duration::hours(12), now + Duration::hours(13)));

        let _ = fs::remove_dir_all(&dir);
    }
}
