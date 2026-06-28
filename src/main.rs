//! `mtgo-deckfinder` CLI.
//!
//! Phase 1: `fetch <format>` pulls recent decks from mtgo.com, validates every
//! card name against the MTGJSON reference, and caches the result. `export
//! --sample` writes the built-in deck. `list` is a Phase 2 stub.

use std::path::PathBuf;
use std::time::Duration as StdDuration;

use anyhow::{Result, bail};
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};

use mtgo_deckfinder::{
    Cache, DeckSource, Format, NameReference, WotcMtgoSource, download_atomic_names,
    export_mtgo_txt, http::PoliteClient, sample_deck,
};

/// Refetch decks older than this.
const DECKS_TTL_HOURS: i64 = 12;
/// Refetch the card-name reference older than this (MTGJSON updates daily; names
/// change slowly, so a week is plenty).
const NAMES_TTL_DAYS: i64 = 7;

#[derive(Parser)]
#[command(
    name = "mtgo-deckfinder",
    version,
    about = "Find and export recent competitive MTGO decklists"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch recent decklists for a format and cache them.
    Fetch {
        /// e.g. `modern`, `pauper`, `legacy`.
        #[arg(value_parser = parse_format)]
        format: Format,
        /// Ignore the cache and refetch from the network.
        #[arg(long)]
        refresh: bool,
    },
    /// List ranked cached decks (Phase 2 — not yet implemented).
    List {
        #[arg(value_parser = parse_format)]
        format: Format,
    },
    /// Export a deck to MTGO-importable text. Currently supports `--sample`.
    Export {
        /// Export the built-in sample deck.
        #[arg(long)]
        sample: bool,
        /// Output file path.
        #[arg(long, short, default_value = "deck.txt")]
        out: PathBuf,
    },
}

fn parse_format(s: &str) -> Result<Format, String> {
    s.parse()
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Fetch { format, refresh } => fetch(format, refresh)?,
        Command::List { format } => {
            println!("list {}: not yet implemented (Phase 2)", format.as_str());
        }
        Command::Export { sample, out } => {
            if !sample {
                bail!("Phase 1 supports only `export --sample`");
            }
            let text = export_mtgo_txt(&sample_deck());
            std::fs::write(&out, &text)?;
            println!("Wrote {} ({} bytes)", out.display(), text.len());
        }
    }
    Ok(())
}

fn fetch(format: Format, refresh: bool) -> Result<()> {
    let cache = Cache::default_location()?;
    let now = Utc::now();

    let names = load_names(&cache, refresh, now)?;

    if !refresh
        && let Some(cached) = cache.read_decks(format)?
        && !cached.is_stale(Duration::hours(DECKS_TTL_HOURS), now)
    {
        println!(
            "Using {} cached {} decks (fetched {}). Pass --refresh to refetch.",
            cached.data.len(),
            format.as_str(),
            cached.fetched_at.format("%Y-%m-%d %H:%M UTC"),
        );
        return Ok(());
    }

    println!("Fetching recent {} decks from mtgo.com…", format.as_str());
    let decks = WotcMtgoSource::new()?.fetch_recent(format)?;

    let mut warnings = 0;
    for deck in &decks {
        for card in deck.maindeck.iter().chain(&deck.sideboard) {
            if !names.is_valid(&card.name) {
                warnings += 1;
                eprintln!("warning: unknown card name in {}: {}", deck.id, card.name);
            }
        }
    }

    cache.write_decks(format, &decks, now)?;
    println!(
        "Fetched and cached {} {} decks ({warnings} card-name warning(s)).",
        decks.len(),
        format.as_str(),
    );
    Ok(())
}

/// Load the card-name reference from cache, downloading from MTGJSON if missing
/// or stale.
fn load_names(cache: &Cache, refresh: bool, now: chrono::DateTime<Utc>) -> Result<NameReference> {
    if !refresh
        && let Some(cached) = cache.read_names()?
        && !cached.is_stale(Duration::days(NAMES_TTL_DAYS), now)
    {
        return Ok(NameReference::from_names(&cached.data));
    }
    println!("Downloading MTGJSON card-name reference (~50 MB, cached afterwards)…");
    let client = PoliteClient::new(StdDuration::from_secs(2))?;
    let names = download_atomic_names(&client)?;
    cache.write_names(&names, now)?;
    Ok(NameReference::from_names(&names))
}
