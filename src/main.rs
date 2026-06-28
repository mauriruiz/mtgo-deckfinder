//! `mtgo-deckfinder` CLI.
//!
//! `fetch <format>` pulls recent decks from mtgo.com (validating card names,
//! detecting colors) and caches them. `list <format>` shows them ranked, with
//! archetype labels and optional color filtering. `export <format> <n>` writes
//! the nth-ranked deck (or `export --sample` the built-in deck) to MTGO text.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use clap::{Parser, Subcommand};

use mtgo_deckfinder::{
    Cache, CardReference, Clustering, Color, ColorMatch, DEFAULT_WEIGHTS, Deck, DeckSource, Format,
    SIMILARITY_THRESHOLD, WotcMtgoSource, cluster_decks, color_matches, colors_label,
    download_atomic_cards, export_mtgo_txt, http::PoliteClient, model::EventResult, parse_colors,
    rank::score, rank_decks, sample_deck,
};

/// Refetch decks older than this.
const DECKS_TTL_HOURS: i64 = 12;
/// Refetch the card reference older than this (MTGJSON updates daily; card data
/// changes slowly, so a week is plenty).
const CARDS_TTL_DAYS: i64 = 7;

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
        /// Filter to these colors, e.g. `--colors UR`.
        #[arg(long, value_parser = parse_color_set)]
        colors: Option<BTreeSet<Color>>,
        /// How `--colors` is matched: `subset` (default), `exact`, or `includes`.
        #[arg(long, default_value = "subset", value_parser = parse_color_match)]
        color_match: ColorMatch,
        /// Group by archetype and show the most popular ones first.
        #[arg(long)]
        archetypes: bool,
    },
    /// Export a deck to MTGO-importable text: `export <format> <rank>`, or `--sample`.
    Export {
        /// Format whose cached, ranked decks to pick from.
        #[arg(value_parser = parse_format)]
        format: Option<Format>,
        /// 1-based rank position to export (see `list`).
        rank: Option<usize>,
        /// Filter to these colors before ranking, e.g. `--colors UR`.
        #[arg(long, value_parser = parse_color_set)]
        colors: Option<BTreeSet<Color>>,
        /// How `--colors` is matched: `subset` (default), `exact`, or `includes`.
        #[arg(long, default_value = "subset", value_parser = parse_color_match)]
        color_match: ColorMatch,
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

fn parse_color_set(s: &str) -> Result<BTreeSet<Color>, String> {
    parse_colors(s)
}

fn parse_color_match(s: &str) -> Result<ColorMatch, String> {
    s.parse()
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Fetch { format, refresh } => fetch(format, refresh)?,
        Command::List {
            format,
            limit,
            colors,
            color_match,
            archetypes,
        } => list(format, limit, filter(colors, color_match), archetypes)?,
        Command::Export {
            format,
            rank,
            colors,
            color_match,
            sample,
            out,
        } => {
            if sample {
                write_deck(&sample_deck(), &out)?;
            } else {
                let format = format.context("provide a format and rank, or pass --sample")?;
                let rank = rank.context("provide a 1-based rank position, or pass --sample")?;
                export_ranked(format, rank, filter(colors, color_match), &out)?;
            }
        }
    }
    Ok(())
}

/// Pair a color set with its match mode, if a `--colors` filter was given.
fn filter(
    colors: Option<BTreeSet<Color>>,
    mode: ColorMatch,
) -> Option<(BTreeSet<Color>, ColorMatch)> {
    colors.map(|c| (c, mode))
}

/// Cached decks for a format, color-filtered, clustered into archetypes, with
/// per-deck popularity — the shared basis for `list` and `export`.
struct Prepared {
    decks: Vec<Deck>,
    clustering: Clustering,
    popularity: Vec<f64>,
    fetched_at: DateTime<Utc>,
}

fn prepare(format: Format, filter: Option<(BTreeSet<Color>, ColorMatch)>) -> Result<Prepared> {
    let cache = Cache::default_location()?;
    let cached = cache.read_decks(format)?.ok_or_else(|| {
        anyhow!(
            "no cached {0} decks — run `fetch {0}` first",
            format.as_str()
        )
    })?;
    let mut decks = cached.data;

    if let Some((want, mode)) = &filter {
        decks.retain(|d| {
            d.colors
                .as_ref()
                .is_some_and(|c| color_matches(c, want, *mode))
        });
    }

    let cards = load_cached_reference(&cache)?;
    let clustering = cluster_decks(&decks, &cards, SIMILARITY_THRESHOLD);
    for (i, d) in decks.iter_mut().enumerate() {
        d.archetype = Some(clustering.label_of(i).to_string());
    }
    let max_size = clustering.sizes.iter().copied().max().unwrap_or(1).max(1) as f64;
    let popularity = (0..decks.len())
        .map(|i| clustering.size_of(i) as f64 / max_size)
        .collect();

    Ok(Prepared {
        decks,
        clustering,
        popularity,
        fetched_at: cached.fetched_at,
    })
}

fn list(
    format: Format,
    limit: usize,
    filter: Option<(BTreeSet<Color>, ColorMatch)>,
    archetypes: bool,
) -> Result<()> {
    let prep = prepare(format, filter)?;
    let today = Utc::now().date_naive();
    if archetypes {
        list_archetypes(&prep, today, limit);
    } else {
        list_decks(&prep, today, limit);
    }
    println!(
        "\nShowing up to {} of {} cached {} decks (fetched {}).",
        limit,
        prep.decks.len(),
        format.as_str(),
        prep.fetched_at.format("%Y-%m-%d %H:%M UTC"),
    );
    Ok(())
}

fn list_decks(prep: &Prepared, today: NaiveDate, limit: usize) {
    let ranked = rank_decks(&prep.decks, today, &DEFAULT_WEIGHTS, &prep.popularity);
    println!(
        "{:>3}  {:>5}  {:<10}  {:<11}  {:<5}  {:<6}  {:<26}  player",
        "#", "score", "date", "event", "color", "result", "archetype"
    );
    for (i, s) in ranked.iter().take(limit).enumerate() {
        let d = s.deck;
        println!(
            "{:>3}  {:>5.3}  {:<10}  {:<11}  {:<5}  {:<6}  {:<26}  {}",
            i + 1,
            s.score,
            d.date,
            d.event_type,
            deck_colors_label(d),
            result_label(&d.result),
            truncate(d.archetype.as_deref().unwrap_or("-"), 26),
            truncate(d.player.as_deref().unwrap_or("-"), 16),
        );
    }
}

fn list_archetypes(prep: &Prepared, today: NaiveDate, limit: usize) {
    let scores: Vec<f64> = (0..prep.decks.len())
        .map(|i| score(&prep.decks[i], today, &DEFAULT_WEIGHTS, prep.popularity[i]))
        .collect();

    // Best (highest-scored) representative deck per cluster.
    let n = prep.clustering.sizes.len();
    let mut best: Vec<Option<usize>> = vec![None; n];
    for i in 0..prep.decks.len() {
        let c = prep.clustering.cluster_of[i];
        if best[c].is_none_or(|j| scores[i] > scores[j]) {
            best[c] = Some(i);
        }
    }

    // Clusters by popularity (size) first, then by their best deck's score.
    let mut clusters: Vec<usize> = (0..n).collect();
    clusters.sort_by(|&a, &b| {
        prep.clustering.sizes[b]
            .cmp(&prep.clustering.sizes[a])
            .then_with(|| {
                let sb = best[b].map_or(0.0, |i| scores[i]);
                let sa = best[a].map_or(0.0, |i| scores[i]);
                sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    println!(
        "{:>3}  {:>5}  {:<5}  {:<28}  best representative",
        "#", "decks", "color", "archetype"
    );
    for (rank, &c) in clusters.iter().take(limit).enumerate() {
        let Some(i) = best[c] else { continue };
        let d = &prep.decks[i];
        println!(
            "{:>3}  {:>5}  {:<5}  {:<28}  {} {} {} {}",
            rank + 1,
            prep.clustering.sizes[c],
            deck_colors_label(d),
            truncate(&prep.clustering.labels[c], 28),
            d.date,
            d.event_type,
            result_label(&d.result),
            truncate(d.player.as_deref().unwrap_or("-"), 16),
        );
    }
}

fn export_ranked(
    format: Format,
    rank: usize,
    filter: Option<(BTreeSet<Color>, ColorMatch)>,
    out: &PathBuf,
) -> Result<()> {
    let prep = prepare(format, filter)?;
    let ranked = rank_decks(
        &prep.decks,
        Utc::now().date_naive(),
        &DEFAULT_WEIGHTS,
        &prep.popularity,
    );
    let idx = rank
        .checked_sub(1)
        .filter(|&i| i < ranked.len())
        .ok_or_else(|| anyhow!("rank {rank} out of range (1..={})", ranked.len()))?;
    let deck = ranked[idx].deck;
    write_deck(deck, out)?;
    println!(
        "  #{rank}  {}  {}  {}  [{}] {}",
        deck.date,
        deck.event_type,
        result_label(&deck.result),
        deck_colors_label(deck),
        deck.archetype.as_deref().unwrap_or("-"),
    );
    Ok(())
}

fn write_deck(deck: &Deck, out: &PathBuf) -> Result<()> {
    let text = export_mtgo_txt(deck);
    std::fs::write(out, &text)?;
    println!("Wrote {} ({} bytes)", out.display(), text.len());
    Ok(())
}

/// WUBRG label for a deck's detected colors (`?` if colors weren't detected).
fn deck_colors_label(deck: &Deck) -> String {
    deck.colors
        .as_ref()
        .map_or_else(|| "?".to_string(), colors_label)
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

    let cards = load_cards(&cache, refresh, now)?;

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
    let mut decks = WotcMtgoSource::new()?.fetch_recent(format)?;

    let mut warnings = 0;
    for deck in &mut decks {
        for card in deck.maindeck.iter().chain(&deck.sideboard) {
            if !cards.is_valid(&card.name) {
                warnings += 1;
                eprintln!("warning: unknown card name in {}: {}", deck.id, card.name);
            }
        }
        deck.colors = Some(cards.deck_colors(&deck.maindeck));
    }

    cache.write_decks(format, &decks, now)?;
    println!(
        "Fetched and cached {} {} decks ({warnings} card-name warning(s)).",
        decks.len(),
        format.as_str(),
    );
    Ok(())
}

/// Load the card reference from cache, downloading from MTGJSON if missing/stale.
fn load_cards(cache: &Cache, refresh: bool, now: DateTime<Utc>) -> Result<CardReference> {
    if !refresh
        && let Some(cached) = cache.read_cards()?
        && !cached.is_stale(Duration::days(CARDS_TTL_DAYS), now)
    {
        return Ok(CardReference::from_records(&cached.data));
    }
    println!("Downloading MTGJSON card reference (~50 MB, cached afterwards)…");
    let client = PoliteClient::new(StdDuration::from_secs(2))?;
    let records = download_atomic_cards(&client)?;
    cache.write_cards(&records, now)?;
    Ok(CardReference::from_records(&records))
}

/// Card reference from cache only (no download); errors if `fetch` hasn't run.
fn load_cached_reference(cache: &Cache) -> Result<CardReference> {
    let cached = cache
        .read_cards()?
        .ok_or_else(|| anyhow!("card reference missing — run a `fetch` first"))?;
    Ok(CardReference::from_records(&cached.data))
}
