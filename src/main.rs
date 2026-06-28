//! `mtgo-deckfinder` CLI.
//!
//! `fetch <format>` pulls recent decks from mtgo.com (validating card names) and
//! caches them. `list <format>` shows them ranked. `export <format> <n>` writes
//! the nth-ranked deck (or `export --sample` the built-in deck) to MTGO text.

use std::path::PathBuf;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration, Utc};
use clap::{Parser, Subcommand};

use mtgo_deckfinder::{
    Cache, DEFAULT_WEIGHTS, Deck, DeckSource, Format, NameReference, WotcMtgoSource,
    download_atomic_names, export_mtgo_txt, http::PoliteClient, model::EventResult, rank_decks,
    sample_deck,
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
    /// List cached decks for a format, best-ranked first.
    List {
        #[arg(value_parser = parse_format)]
        format: Format,
        /// Max rows to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Export a deck to MTGO-importable text: `export <format> <rank>`, or `--sample`.
    Export {
        /// Format whose cached, ranked decks to pick from.
        #[arg(value_parser = parse_format)]
        format: Option<Format>,
        /// 1-based rank position to export (see `list`).
        rank: Option<usize>,
        /// Export the built-in sample deck instead.
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
        Command::List { format, limit } => list(format, limit)?,
        Command::Export {
            format,
            rank,
            sample,
            out,
        } => {
            if sample {
                write_deck(&sample_deck(), &out)?;
            } else {
                let format = format.context("provide a format and rank, or pass --sample")?;
                let rank = rank.context("provide a 1-based rank position, or pass --sample")?;
                export_ranked(format, rank, &out)?;
            }
        }
    }
    Ok(())
}

/// Load cached, ranked decks for a format, erroring if nothing is cached yet.
fn ranked_cached(format: Format) -> Result<(Vec<Deck>, DateTime<Utc>)> {
    let cached = Cache::default_location()?
        .read_decks(format)?
        .ok_or_else(|| {
            anyhow!(
                "no cached {0} decks — run `fetch {0}` first",
                format.as_str()
            )
        })?;
    Ok((cached.data, cached.fetched_at))
}

fn list(format: Format, limit: usize) -> Result<()> {
    let (decks, fetched_at) = ranked_cached(format)?;
    let ranked = rank_decks(&decks, Utc::now().date_naive(), &DEFAULT_WEIGHTS);

    println!(
        "{:>3}  {:>5}  {:<10}  {:<11}  {:<7}  {:<18}  source",
        "#", "score", "date", "event", "result", "player"
    );
    for (i, s) in ranked.iter().take(limit).enumerate() {
        let d = s.deck;
        println!(
            "{:>3}  {:>5.3}  {:<10}  {:<11}  {:<7}  {:<18}  {}",
            i + 1,
            s.score,
            d.date,
            d.event_type,
            result_label(&d.result),
            truncate(d.player.as_deref().unwrap_or("-"), 18),
            d.source,
        );
    }
    println!(
        "\nShowing {} of {} cached {} decks (fetched {}).",
        ranked.len().min(limit),
        ranked.len(),
        format.as_str(),
        fetched_at.format("%Y-%m-%d %H:%M UTC"),
    );
    Ok(())
}

fn export_ranked(format: Format, rank: usize, out: &PathBuf) -> Result<()> {
    let (decks, _) = ranked_cached(format)?;
    let ranked = rank_decks(&decks, Utc::now().date_naive(), &DEFAULT_WEIGHTS);
    let idx = rank
        .checked_sub(1)
        .filter(|&i| i < ranked.len())
        .ok_or_else(|| anyhow!("rank {rank} out of range (1..={})", ranked.len()))?;
    let deck = ranked[idx].deck;
    write_deck(deck, out)?;
    println!(
        "  #{rank}  {}  {}  {}",
        deck.date,
        deck.event_type,
        deck.player.as_deref().unwrap_or("-"),
    );
    Ok(())
}

fn write_deck(deck: &Deck, out: &PathBuf) -> Result<()> {
    let text = export_mtgo_txt(deck);
    std::fs::write(out, &text)?;
    println!("Wrote {} ({} bytes)", out.display(), text.len());
    Ok(())
}

/// Compact result column: tournament rank if known, else win-loss record.
fn result_label(r: &EventResult) -> String {
    match (r.rank, r.wins, r.losses) {
        (Some(rank), _, _) => format!("#{rank}"),
        (None, Some(w), Some(l)) => format!("{w}-{l}"),
        _ => "-".to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    } else {
        s.to_string()
    }
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
