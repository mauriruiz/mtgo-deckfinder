//! `mtgo-deckfinder` CLI.
//!
//! `fetch <format>` pulls recent decks from mtgo.com (validating names, detecting
//! colors, pricing via Scryfall) and caches them. `list <format>` shows them
//! ranked, with optional `--colors` filter and `--view` (archetypes / buildable /
//! cheapest / balance). `import-collection <file>` loads your owned cards so the
//! tool can show what's cheapest to *complete*. `export` writes a deck to MTGO text.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use clap::{Parser, Subcommand, ValueEnum};

use mtgo_deckfinder::cards::{card_keys, lookup_key};
use mtgo_deckfinder::{
    Cache, CardReference, Clustering, Color, ColorMatch, DEFAULT_WEIGHTS, Deck, DeckSource, Format,
    GapInfo, PriceTable, SIMILARITY_THRESHOLD, WotcMtgoSource, cluster_decks, color_matches,
    colors_label, deck_gap, download_atomic_cards, export_mtgo_txt, fetch_prices,
    http::PoliteClient, model::EventResult, parse_collection_csv, parse_colors, rank::score,
    sample_deck,
};

/// Refetch decks older than this.
const DECKS_TTL_HOURS: i64 = 12;
/// Refetch the card reference older than this.
const CARDS_TTL_DAYS: i64 = 7;
/// Refetch prices older than this (MTGO prices move daily).
const PRICE_TTL_HOURS: i64 = 24;
/// Base score at/above which a deck counts as "competitive" for price views.
const STRENGTH_THRESHOLD: f64 = 0.6;
/// Best-balance view: weight on normalized cost (strength − this·cost).
const PRICE_PENALTY: f64 = 0.4;

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

/// Deck-selection view.
#[derive(Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
enum View {
    /// Best by strength (default).
    #[default]
    Ranked,
    /// Most popular archetypes, biggest first.
    Archetypes,
    /// Only decks you can build now (needs an imported collection).
    Buildable,
    /// Lowest cost among competitive decks (cost-to-complete with a collection,
    /// else total price).
    Cheapest,
    /// Best strength-for-cost trade-off among competitive decks.
    Balance,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch recent decklists for a format and cache them.
    Fetch {
        #[arg(value_parser = parse_format)]
        format: Format,
        /// Ignore the cache and refetch from the network.
        #[arg(long)]
        refresh: bool,
    },
    /// Import an MTGO collection export (CSV) to enable cost-to-complete views.
    ImportCollection {
        /// Path to the MTGO collection CSV.
        path: PathBuf,
    },
    /// List cached decks for a format.
    List {
        #[arg(value_parser = parse_format)]
        format: Format,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Filter to these colors, e.g. `--colors UR`.
        #[arg(long, value_parser = parse_color_set)]
        colors: Option<HashSet8>,
        /// How `--colors` matches: `subset` (default), `exact`, `includes`.
        #[arg(long, default_value = "subset", value_parser = parse_color_match)]
        color_match: ColorMatch,
        /// Selection view.
        #[arg(long, value_enum, default_value_t = View::Ranked)]
        view: View,
    },
    /// Export a deck to MTGO-importable text: `export <format> <rank>`, or `--sample`.
    Export {
        #[arg(value_parser = parse_format)]
        format: Option<Format>,
        /// 1-based rank position within the chosen view (see `list`).
        rank: Option<usize>,
        #[arg(long, value_parser = parse_color_set)]
        colors: Option<HashSet8>,
        #[arg(long, default_value = "subset", value_parser = parse_color_match)]
        color_match: ColorMatch,
        /// Which view's ordering to pick from.
        #[arg(long, value_enum, default_value_t = View::Ranked)]
        view: View,
        /// Export the built-in sample deck instead.
        #[arg(long)]
        sample: bool,
        #[arg(long, short, default_value = "deck.txt")]
        out: PathBuf,
    },
}

/// Alias to keep the clap arg type readable.
type HashSet8 = std::collections::BTreeSet<Color>;

fn parse_format(s: &str) -> Result<Format, String> {
    s.parse()
}
fn parse_color_set(s: &str) -> Result<HashSet8, String> {
    parse_colors(s)
}
fn parse_color_match(s: &str) -> Result<ColorMatch, String> {
    s.parse()
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Fetch { format, refresh } => fetch(format, refresh)?,
        Command::ImportCollection { path } => import_collection(&path)?,
        Command::List {
            format,
            limit,
            colors,
            color_match,
            view,
        } => list(format, limit, filter(colors, color_match), view)?,
        Command::Export {
            format,
            rank,
            colors,
            color_match,
            view,
            sample,
            out,
        } => {
            if sample {
                write_deck(&sample_deck(), &out)?;
            } else {
                let format = format.context("provide a format and rank, or pass --sample")?;
                let rank = rank.context("provide a 1-based rank position, or pass --sample")?;
                export_ranked(format, rank, filter(colors, color_match), view, &out)?;
            }
        }
    }
    Ok(())
}

fn filter(colors: Option<HashSet8>, mode: ColorMatch) -> Option<(HashSet8, ColorMatch)> {
    colors.map(|c| (c, mode))
}

// ---- shared preparation ----

/// Cached decks for a format: color-filtered, clustered, priced, and (if a
/// collection is loaded) gap-analyzed. Shared by `list` and `export`.
struct Prepared {
    decks: Vec<Deck>, // est_price + archetype populated in place
    clustering: Clustering,
    popularity: Vec<f64>,
    gaps: Vec<Option<GapInfo>>, // parallel to decks; None without a collection
    has_collection: bool,
    fetched_at: DateTime<Utc>,
}

fn prepare(format: Format, filter: Option<(HashSet8, ColorMatch)>) -> Result<Prepared> {
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

    let prices = match cache.read_prices()? {
        Some(c) => PriceTable::from_pairs(c.data),
        None => PriceTable::from_pairs([]),
    };
    for d in &mut decks {
        d.est_price = (!prices.is_empty()).then(|| prices.deck_price(d));
    }

    let collection = cache.read_collection()?.map(|c| c.data);
    let gaps: Vec<Option<GapInfo>> = match &collection {
        Some(coll) => decks
            .iter()
            .map(|d| Some(deck_gap(d, coll, &prices)))
            .collect(),
        None => vec![None; decks.len()],
    };

    Ok(Prepared {
        decks,
        clustering,
        popularity,
        gaps,
        has_collection: collection.is_some(),
        fetched_at: cached.fetched_at,
    })
}

// ---- list ----

fn list(
    format: Format,
    limit: usize,
    filter: Option<(HashSet8, ColorMatch)>,
    view: View,
) -> Result<()> {
    let prep = prepare(format, filter)?;
    let today = Utc::now().date_naive();
    if view == View::Archetypes {
        list_archetypes(&prep, today, limit);
    } else {
        let order = order_decks(&prep, view, today)?;
        print_deck_table(&prep, &order, limit, today);
    }
    println!(
        "\nShowing up to {} of {} cached {} decks (fetched {}). Prices are approximate (Scryfall tix).",
        limit,
        prep.decks.len(),
        format.as_str(),
        prep.fetched_at.format("%Y-%m-%d %H:%M UTC"),
    );
    Ok(())
}

/// Indices of `prep.decks` in the order a view displays them.
fn order_decks(prep: &Prepared, view: View, today: NaiveDate) -> Result<Vec<usize>> {
    let n = prep.decks.len();
    let scores: Vec<f64> = (0..n)
        .map(|i| score(&prep.decks[i], today, &DEFAULT_WEIGHTS, prep.popularity[i]))
        .collect();
    let cost = |i: usize| -> f64 {
        if prep.has_collection {
            prep.gaps[i].as_ref().map_or(0.0, |g| g.cost_to_complete)
        } else {
            prep.decks[i].est_price.unwrap_or(0.0)
        }
    };
    let by_strength = |a: &usize, b: &usize| {
        scores[*b]
            .partial_cmp(&scores[*a])
            .unwrap_or(Ordering::Equal)
            .then_with(|| prep.decks[*b].date.cmp(&prep.decks[*a].date))
            .then_with(|| prep.decks[*a].id.cmp(&prep.decks[*b].id))
    };

    let mut idx: Vec<usize> = (0..n).collect();
    match view {
        View::Ranked => idx.sort_by(by_strength),
        View::Buildable => {
            if !prep.has_collection {
                bail!("no collection loaded — run `import-collection <file>` first");
            }
            idx.retain(|&i| prep.gaps[i].as_ref().is_some_and(|g| g.buildable_now));
            idx.sort_by(by_strength);
        }
        View::Cheapest => {
            idx.retain(|&i| scores[i] >= STRENGTH_THRESHOLD);
            idx.sort_by(|a, b| {
                cost(*a)
                    .partial_cmp(&cost(*b))
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| by_strength(a, b))
            });
        }
        View::Balance => {
            idx.retain(|&i| scores[i] >= STRENGTH_THRESHOLD);
            let max_cost = idx
                .iter()
                .map(|&i| cost(i))
                .fold(0.0_f64, f64::max)
                .max(1e-9);
            let balance = |i: usize| scores[i] - PRICE_PENALTY * (cost(i) / max_cost);
            idx.sort_by(|a, b| {
                balance(*b)
                    .partial_cmp(&balance(*a))
                    .unwrap_or(Ordering::Equal)
            });
        }
        View::Archetypes => unreachable!("handled in list()"),
    }
    Ok(idx)
}

fn print_deck_table(prep: &Prepared, order: &[usize], limit: usize, today: NaiveDate) {
    if prep.has_collection {
        println!(
            "{:>3}  {:>5}  {:<10}  {:<11}  {:<5}  {:>6}  {:>4}  {:>6}  {:<6}  {:<22}  player",
            "#", "score", "date", "event", "color", "~tix", "miss", "+tix", "result", "archetype"
        );
    } else {
        println!(
            "{:>3}  {:>5}  {:<10}  {:<11}  {:<5}  {:>6}  {:<6}  {:<22}  player",
            "#", "score", "date", "event", "color", "~tix", "result", "archetype"
        );
    }
    for (rank, &i) in order.iter().take(limit).enumerate() {
        let d = &prep.decks[i];
        let s = score(d, today, &DEFAULT_WEIGHTS, prep.popularity[i]);
        let price = d
            .est_price
            .map_or_else(|| "—".to_string(), |p| format!("{p:.1}"));
        let arch = truncate(d.archetype.as_deref().unwrap_or("-"), 22);
        let player = truncate(d.player.as_deref().unwrap_or("-"), 14);
        if let Some(g) = prep.gaps[i].as_ref() {
            println!(
                "{:>3}  {:>5.3}  {:<10}  {:<11}  {:<5}  {:>6}  {:>4}  {:>6.1}  {:<6}  {:<22}  {}",
                rank + 1,
                s,
                d.date,
                d.event_type,
                deck_colors_label(d),
                price,
                g.cards_missing,
                g.cost_to_complete,
                result_label(&d.result),
                arch,
                player,
            );
        } else {
            println!(
                "{:>3}  {:>5.3}  {:<10}  {:<11}  {:<5}  {:>6}  {:<6}  {:<22}  {}",
                rank + 1,
                s,
                d.date,
                d.event_type,
                deck_colors_label(d),
                price,
                result_label(&d.result),
                arch,
                player,
            );
        }
    }
}

fn list_archetypes(prep: &Prepared, today: NaiveDate, limit: usize) {
    let scores: Vec<f64> = (0..prep.decks.len())
        .map(|i| score(&prep.decks[i], today, &DEFAULT_WEIGHTS, prep.popularity[i]))
        .collect();

    let n = prep.clustering.sizes.len();
    let mut best: Vec<Option<usize>> = vec![None; n];
    for i in 0..prep.decks.len() {
        let c = prep.clustering.cluster_of[i];
        if best[c].is_none_or(|j| scores[i] > scores[j]) {
            best[c] = Some(i);
        }
    }

    let mut clusters: Vec<usize> = (0..n).collect();
    clusters.sort_by(|&a, &b| {
        prep.clustering.sizes[b]
            .cmp(&prep.clustering.sizes[a])
            .then_with(|| {
                let (sb, sa) = (
                    best[b].map_or(0.0, |i| scores[i]),
                    best[a].map_or(0.0, |i| scores[i]),
                );
                sb.partial_cmp(&sa).unwrap_or(Ordering::Equal)
            })
    });

    println!(
        "{:>3}  {:>5}  {:<5}  {:>6}  {:<28}  best representative",
        "#", "decks", "color", "~tix", "archetype"
    );
    for (rank, &c) in clusters.iter().take(limit).enumerate() {
        let Some(i) = best[c] else { continue };
        let d = &prep.decks[i];
        let price = d
            .est_price
            .map_or_else(|| "—".to_string(), |p| format!("{p:.1}"));
        println!(
            "{:>3}  {:>5}  {:<5}  {:>6}  {:<28}  {} {} {} {}",
            rank + 1,
            prep.clustering.sizes[c],
            deck_colors_label(d),
            price,
            truncate(&prep.clustering.labels[c], 28),
            d.date,
            d.event_type,
            result_label(&d.result),
            truncate(d.player.as_deref().unwrap_or("-"), 14),
        );
    }
}

// ---- export ----

fn export_ranked(
    format: Format,
    rank: usize,
    filter: Option<(HashSet8, ColorMatch)>,
    view: View,
    out: &PathBuf,
) -> Result<()> {
    if view == View::Archetypes {
        bail!("--view archetypes is not a deck list; use ranked / buildable / cheapest / balance");
    }
    let prep = prepare(format, filter)?;
    let order = order_decks(&prep, view, Utc::now().date_naive())?;
    let pos = rank
        .checked_sub(1)
        .filter(|&i| i < order.len())
        .ok_or_else(|| anyhow!("rank {rank} out of range (1..={})", order.len()))?;
    let i = order[pos];
    let d = &prep.decks[i];
    write_deck(d, out)?;
    let price = d
        .est_price
        .map_or_else(|| "?".to_string(), |p| format!("~{p:.1} tix"));
    let cost = prep.gaps[i]
        .as_ref()
        .map(|g| {
            format!(
                ", complete for ~{:.1} tix ({} missing)",
                g.cost_to_complete, g.cards_missing
            )
        })
        .unwrap_or_default();
    println!(
        "  #{rank}  {}  {}  [{}] {}  {price}{cost}",
        d.date,
        d.event_type,
        deck_colors_label(d),
        d.archetype.as_deref().unwrap_or("-"),
    );
    Ok(())
}

fn write_deck(deck: &Deck, out: &PathBuf) -> Result<()> {
    let text = export_mtgo_txt(deck);
    std::fs::write(out, &text)?;
    println!("Wrote {} ({} bytes)", out.display(), text.len());
    Ok(())
}

fn deck_colors_label(deck: &Deck) -> String {
    deck.colors
        .as_ref()
        .map_or_else(|| "?".to_string(), colors_label)
}

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

// ---- fetch / import ----

fn fetch(format: Format, refresh: bool) -> Result<()> {
    let cache = Cache::default_location()?;
    let now = Utc::now();

    let cards = load_cards(&cache, refresh, now)?;

    let decks = if !refresh
        && let Some(cached) = cache.read_decks(format)?
        && !cached.is_stale(Duration::hours(DECKS_TTL_HOURS), now)
    {
        println!(
            "Using {} cached {} decks (fetched {}). Pass --refresh to refetch.",
            cached.data.len(),
            format.as_str(),
            cached.fetched_at.format("%Y-%m-%d %H:%M UTC"),
        );
        cached.data
    } else {
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
        decks
    };

    // Always ensure prices exist for the format's cards (cheap if already cached).
    price_cards(&cache, &decks, refresh, now)?;
    Ok(())
}

fn import_collection(path: &PathBuf) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let collection = parse_collection_csv(&text)?;
    Cache::default_location()?.write_collection(&collection, Utc::now())?;
    println!(
        "Imported collection: {} distinct cards. Collection-aware views (buildable / cheapest) are now active.",
        collection.distinct_cards(),
    );
    Ok(())
}

/// Price the cards across `decks` via Scryfall, caching results and only
/// fetching names not already priced.
fn price_cards(cache: &Cache, decks: &[Deck], refresh: bool, now: DateTime<Utc>) -> Result<()> {
    let mut pairs = match (refresh, cache.read_prices()?) {
        (false, Some(c)) if !c.is_stale(Duration::hours(PRICE_TTL_HOURS), now) => c.data,
        _ => Vec::new(),
    };
    let known: HashSet<String> = pairs.iter().flat_map(|(n, _)| card_keys(n)).collect();

    let mut need: Vec<String> = decks
        .iter()
        .flat_map(|d| d.maindeck.iter().chain(&d.sideboard))
        .map(|c| c.name.clone())
        .filter(|n| !known.contains(&lookup_key(n)))
        .collect();
    need.sort();
    need.dedup();
    if need.is_empty() {
        return Ok(());
    }

    println!("Pricing {} cards via Scryfall…", need.len());
    let scryfall = PoliteClient::new(StdDuration::from_millis(150))?;
    pairs.extend(fetch_prices(&scryfall, &need)?);
    cache.write_prices(&pairs, now)?;
    Ok(())
}

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

fn load_cached_reference(cache: &Cache) -> Result<CardReference> {
    let cached = cache
        .read_cards()?
        .ok_or_else(|| anyhow!("card reference missing — run a `fetch` first"))?;
    Ok(CardReference::from_records(&cached.data))
}
