//! Deck sources. A [`DeckSource`] yields normalized [`Deck`]s for a format;
//! new sources slot in behind the trait without touching core logic.
//!
//! The only implementation so far is [`WotcMtgoSource`], over the official
//! published MTGO decklists at <https://www.mtgo.com/decklists>.
//!
//! ## How mtgo.com is read (investigated June 2026)
//! The index page (`/decklists`, paginated `/decklists/<year>/<month>`) is
//! server-rendered: every event links to `/decklist/<slug>`, where the slug is
//! `<format>-<eventtype>-<n>-<YYYY-MM-DD><id>`. Each detail page embeds the full
//! event as JSON in `window.MTGO.decklists.data = {…};` — far more stable than
//! the rendered HTML, so we parse that JSON rather than scraping markup.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::http::PoliteClient;
use crate::model::{CardEntry, Deck, EventResult, EventType, Format};
use crate::names::normalize_name;

const INDEX_URL: &str = "https://www.mtgo.com/decklists";
const BASE_URL: &str = "https://www.mtgo.com";
/// Cap per fetch: the N most recent events for the format. Each event holds many
/// decks, so this is plenty while staying polite.
/// ponytail: fixed cap; paginate `/decklists/<year>/<month>` for deeper history.
const RECENT_EVENT_LIMIT: usize = 12;

/// A source of normalized decklists for a given format.
pub trait DeckSource {
    fn fetch_recent(&self, format: Format) -> Result<Vec<Deck>>;
}

/// Official WotC/MTGO published decklists.
pub struct WotcMtgoSource {
    client: PoliteClient,
}

impl WotcMtgoSource {
    /// Build with a polite client (≥2s between requests to mtgo.com).
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: PoliteClient::new(std::time::Duration::from_secs(2))?,
        })
    }

    fn fetch_event(&self, url: &str, format: Format) -> Result<Vec<Deck>> {
        let html = self.client.get_text(url)?;
        let json = extract_event_json(&html)?;
        let raw: RawEvent = serde_json::from_str(json)?;
        normalize_event(&raw, format, url)
    }
}

impl DeckSource for WotcMtgoSource {
    fn fetch_recent(&self, format: Format) -> Result<Vec<Deck>> {
        let index = self.client.get_text(INDEX_URL)?;
        let slugs = extract_event_slugs(&index);
        let recent = select_recent(&slugs, format, RECENT_EVENT_LIMIT);

        let mut decks = Vec::new();
        for slug in &recent {
            let url = format!("{BASE_URL}{slug}");
            match self.fetch_event(&url, format) {
                Ok(event_decks) => decks.extend(event_decks),
                // In-progress events have no embedded data yet, and one page that
                // fails to parse shouldn't abort the whole fetch — skip and warn.
                Err(e) => eprintln!("warning: skipping {url}: {e}"),
            }
        }
        Ok(decks)
    }
}

// ---- pure parsing/normalization (unit-tested offline against fixtures) ----

/// All `/decklist/<slug>` hrefs in an index page (may contain duplicates).
fn extract_event_slugs(html: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("/decklist/") {
        let tail = &rest[pos..];
        let end = tail
            .find(['"', '\'', '<', '>', ' ', '\\'])
            .unwrap_or(tail.len());
        if end > "/decklist/".len() {
            out.push(&tail[..end]);
        }
        rest = &tail[end..];
    }
    out
}

/// Filter slugs to `format`, newest first, deduped, capped at `limit`.
fn select_recent(slugs: &[&str], format: Format, limit: usize) -> Vec<String> {
    let prefix = format.slug_prefix();
    let mut dated: Vec<(NaiveDate, String)> = slugs
        .iter()
        .filter(|s| {
            s.strip_prefix("/decklist/")
                .is_some_and(|name| name.starts_with(prefix))
        })
        .filter_map(|s| slug_date(s).map(|d| (d, (*s).to_string())))
        .collect();
    // Newest date first; slug as a deterministic tie-breaker.
    dated.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    dated.dedup_by(|a, b| a.1 == b.1);
    dated.into_iter().take(limit).map(|(_, s)| s).collect()
}

/// First `YYYY-MM-DD` embedded in a slug.
///
/// The window must start on a digit, otherwise the separator dash before the
/// year (`...-2026-06-28...`) parses as a negative year (`-2026`), which sorted
/// every day ≥10 to the bottom. The year is also range-checked for safety.
fn slug_date(slug: &str) -> Option<NaiveDate> {
    let bytes = slug.as_bytes();
    for i in 0..bytes.len().saturating_sub(9) {
        if !bytes[i].is_ascii_digit() {
            continue;
        }
        if let Ok(d) = NaiveDate::parse_from_str(&slug[i..i + 10], "%Y-%m-%d")
            && (2000..2100).contains(&d.year())
        {
            return Some(d);
        }
    }
    None
}

/// Extract the `window.MTGO.decklists.data = {…}` object literal from a detail
/// page by brace-matching (strings/escapes aware).
fn extract_event_json(html: &str) -> Result<&str> {
    const MARKER: &str = "window.MTGO.decklists.data";
    let marker = html
        .find(MARKER)
        .ok_or_else(|| Error::Parse("decklist data marker not found".into()))?;
    let after_eq = html[marker..]
        .find('=')
        .map(|i| marker + i + 1)
        .ok_or_else(|| Error::Parse("malformed decklist data assignment".into()))?;
    let start = html[after_eq..]
        .find('{')
        .map(|i| after_eq + i)
        .ok_or_else(|| Error::Parse("decklist json object not found".into()))?;

    let bytes = html.as_bytes();
    let (mut depth, mut in_str, mut escaped) = (0i32, false, false);
    for (i, &c) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(&html[start..=i]);
                    }
                }
                _ => {}
            }
        }
    }
    Err(Error::Parse("unterminated decklist json".into()))
}

/// Normalize either event schema into decks. mtgo.com serves two distinct
/// shapes: tournaments (Challenge/Preliminary, with `starttime` and separate
/// `winloss`/`final_rank` arrays joined by `loginid`) and leagues (5-0 lists,
/// with `publish_date` and a per-deck win/loss record).
fn normalize_event(raw: &RawEvent, format: Format, source_url: &str) -> Result<Vec<Deck>> {
    match raw {
        RawEvent::Tournament(t) => normalize_tournament(t, format, source_url),
        RawEvent::League(l) => normalize_league(l, format, source_url),
    }
}

fn normalize_tournament(t: &RawTournament, format: Format, source_url: &str) -> Result<Vec<Deck>> {
    let date = parse_date(&t.starttime)?;
    let event_type = classify_event(&t.description);

    let records: HashMap<&str, (Option<u32>, Option<u32>)> = t
        .winloss
        .iter()
        .map(|w| {
            (
                w.loginid.as_str(),
                (parse_opt(&w.wins), parse_opt(&w.losses)),
            )
        })
        .collect();
    let ranks: HashMap<&str, u32> = t
        .final_rank
        .iter()
        .filter_map(|r| Some((r.loginid.as_str(), r.rank.parse().ok()?)))
        .collect();

    t.decklists
        .iter()
        .map(|d| {
            let (wins, losses) = records
                .get(d.loginid.as_str())
                .copied()
                .unwrap_or((None, None));
            build_deck(
                format!("wotc-mtgo-{}", d.decktournamentid),
                format,
                source_url,
                date,
                event_type,
                EventResult {
                    rank: ranks.get(d.loginid.as_str()).copied(),
                    wins,
                    losses,
                },
                &d.player,
                &d.main_deck,
                &d.sideboard_deck,
            )
        })
        .collect()
}

fn normalize_league(l: &RawLeague, format: Format, source_url: &str) -> Result<Vec<Deck>> {
    let date = parse_date(&l.publish_date)?;
    l.decklists
        .iter()
        .map(|d| {
            build_deck(
                format!("wotc-mtgo-{}", d.loginplayeventcourseid),
                format,
                source_url,
                date,
                EventType::League,
                EventResult {
                    rank: None,
                    wins: parse_opt(&d.wins.wins),
                    losses: parse_opt(&d.wins.losses),
                },
                &d.player,
                &d.main_deck,
                &d.sideboard_deck,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_deck(
    id: String,
    format: Format,
    source_url: &str,
    date: NaiveDate,
    event_type: EventType,
    result: EventResult,
    player: &str,
    main_deck: &[RawCard],
    sideboard_deck: &[RawCard],
) -> Result<Deck> {
    Ok(Deck {
        id,
        format,
        source: "wotc-mtgo".to_string(),
        source_url: source_url.to_string(),
        date,
        event_type,
        result,
        archetype: None,
        colors: None,
        player: Some(player.to_string()),
        maindeck: to_entries(main_deck)?,
        sideboard: to_entries(sideboard_deck)?,
        est_price: None,
    })
}

fn to_entries(cards: &[RawCard]) -> Result<Vec<CardEntry>> {
    cards
        .iter()
        .map(|c| {
            Ok(CardEntry {
                name: normalize_name(&c.card_attributes.card_name),
                quantity: c
                    .qty
                    .parse()
                    .map_err(|_| Error::Parse(format!("bad quantity {:?}", c.qty)))?,
            })
        })
        .collect()
}

fn parse_date(starttime: &str) -> Result<NaiveDate> {
    let day = starttime.get(..10).unwrap_or(starttime);
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|e| Error::Parse(format!("bad starttime {starttime:?}: {e}")))
}

fn classify_event(description: &str) -> EventType {
    let d = description.to_lowercase();
    if d.contains("league") {
        EventType::League
    } else if d.contains("challenge") {
        EventType::Challenge
    } else if d.contains("preliminary") {
        EventType::Preliminary
    } else {
        EventType::Other
    }
}

fn parse_opt(s: &str) -> Option<u32> {
    s.parse().ok()
}

// ---- raw JSON shapes (only the fields we use; serde ignores the rest) ----
//
// mtgo.com serves two schemas. The untagged enum tries `Tournament` first; a
// league page lacks `starttime`/`decktournamentid` so it falls through to
// `League` (which a tournament page can't match — it has no `publish_date`).

#[derive(Deserialize)]
#[serde(untagged)]
enum RawEvent {
    Tournament(RawTournament),
    League(RawLeague),
}

#[derive(Deserialize)]
struct RawTournament {
    description: String,
    starttime: String,
    // Absent on bracket-only / results-pending events → no decks, not an error.
    #[serde(default)]
    decklists: Vec<RawTournamentDeck>,
    #[serde(default)]
    winloss: Vec<RawWinLoss>,
    #[serde(default)]
    final_rank: Vec<RawRank>,
}

#[derive(Deserialize)]
struct RawTournamentDeck {
    loginid: String,
    decktournamentid: String,
    player: String,
    main_deck: Vec<RawCard>,
    sideboard_deck: Vec<RawCard>,
}

#[derive(Deserialize)]
struct RawLeague {
    publish_date: String,
    #[serde(default)]
    decklists: Vec<RawLeagueDeck>,
}

#[derive(Deserialize)]
struct RawLeagueDeck {
    loginplayeventcourseid: String,
    player: String,
    main_deck: Vec<RawCard>,
    sideboard_deck: Vec<RawCard>,
    /// Per-deck record; a published league list is 5-0.
    wins: RawRecord,
}

#[derive(Deserialize)]
struct RawRecord {
    wins: String,
    losses: String,
}

#[derive(Deserialize)]
struct RawCard {
    qty: String,
    card_attributes: RawCardAttrs,
}

#[derive(Deserialize)]
struct RawCardAttrs {
    card_name: String,
}

#[derive(Deserialize)]
struct RawWinLoss {
    loginid: String,
    wins: String,
    losses: String,
}

#[derive(Deserialize)]
struct RawRank {
    loginid: String,
    rank: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::NameReference;

    const INDEX: &str = include_str!("../tests/fixtures/mtgo_index.html");
    const DETAIL: &str = include_str!("../tests/fixtures/mtgo_detail.html");
    const LEAGUE: &str = include_str!("../tests/fixtures/mtgo_league.html");
    const ATOMIC: &str = include_str!("../tests/fixtures/atomic_cards_sample.json");

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn decks_from(html: &str) -> Vec<Deck> {
        let json = extract_event_json(html).unwrap();
        let raw: RawEvent = serde_json::from_str(json).unwrap();
        normalize_event(&raw, Format::Modern, "http://x").unwrap()
    }

    #[test]
    fn extracts_all_event_slugs() {
        let slugs = extract_event_slugs(INDEX);
        assert_eq!(slugs.len(), 9);
        assert!(slugs.iter().all(|s| s.starts_with("/decklist/")));
    }

    #[test]
    fn slug_date_parses_days_past_the_ninth() {
        // Regression: the separator dash must not be read as a negative year.
        assert_eq!(
            slug_date("modern-league-2026-06-2810847"),
            Some(ymd(2026, 6, 28))
        );
        assert_eq!(
            slug_date("modern-challenge-32-2026-06-0112843430"),
            Some(ymd(2026, 6, 1))
        );
        assert_eq!(slug_date("no-date-here"), None);
    }

    #[test]
    fn selects_recent_modern_newest_first_and_capped() {
        let slugs = extract_event_slugs(INDEX);
        let recent = select_recent(&slugs, Format::Modern, 2);
        assert_eq!(recent.len(), 2);
        assert!(recent.iter().all(|s| s.starts_with("/decklist/modern-")));
        // 06-28 and 06-15 must outrank the early-June events.
        assert!(recent[0].contains("2026-06-28"));
        assert!(recent[1].contains("2026-06-15"));
    }

    #[test]
    fn parses_and_normalizes_tournament_page() {
        let decks = decks_from(DETAIL);
        assert_eq!(decks.len(), 2);
        let ranked = decks.iter().find(|d| d.result.rank == Some(1)).unwrap();
        assert_eq!(ranked.player.as_deref(), Some("ashame"));
        assert_eq!(ranked.event_type, EventType::Challenge);
        assert_eq!(ranked.format, Format::Modern);
        assert!(ranked.id.starts_with("wotc-mtgo-"));
        assert!(ranked.result.wins.is_some() && ranked.result.losses.is_some());
        assert_eq!(ranked.maindeck.iter().map(|c| c.quantity).sum::<u32>(), 60);
    }

    #[test]
    fn parses_and_normalizes_league_page() {
        let decks = decks_from(LEAGUE);
        assert_eq!(decks.len(), 2);
        let deck = &decks[0];
        assert_eq!(deck.event_type, EventType::League);
        assert_eq!(deck.result.rank, None);
        assert_eq!(deck.result.wins, Some(5));
        assert_eq!(deck.result.losses, Some(0));
        assert!(deck.player.is_some());
        assert!(deck.id.starts_with("wotc-mtgo-"));
        assert_eq!(deck.maindeck.iter().map(|c| c.quantity).sum::<u32>(), 60);
    }

    #[test]
    fn every_parsed_card_name_passes_validation() {
        let names = NameReference::from_atomic_reader(ATOMIC.as_bytes()).unwrap();
        let decks = decks_from(DETAIL).into_iter().chain(decks_from(LEAGUE));
        for deck in decks {
            for card in deck.maindeck.iter().chain(&deck.sideboard) {
                assert!(names.is_valid(&card.name), "unknown name: {}", card.name);
            }
        }
    }
}
