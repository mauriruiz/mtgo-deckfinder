//! Pure deck ranking. No IO, no clock — `today` is passed in — so the whole
//! module is deterministic and unit-testable.
//!
//! `score = w_recency·recency + w_source·source + w_result·result +
//! w_popularity·popularity`, each factor normalized to 0..1 and combined via the
//! single [`Weights`] table. Popularity (archetype cluster size) is supplied by
//! the caller; price (Phase 4) remains a reserved weight hook.

use chrono::NaiveDate;

use crate::model::{Deck, EventResult, EventType};

/// Recency half-life: a deck this many days old counts as half as recent.
pub const RECENCY_HALF_LIFE_DAYS: f64 = 14.0;

/// Tunable factor weights — the one place ranking is configured.
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub recency: f64,
    pub source: f64,
    pub result: f64,
    /// Archetype popularity (cluster size).
    pub popularity: f64,
    /// Phase 4 hook (price) — not yet scored.
    pub price: f64,
}

/// Default weights (sum to 1, so scores land in ~0..1).
pub const DEFAULT_WEIGHTS: Weights = Weights {
    recency: 0.35,
    source: 0.15,
    result: 0.35,
    popularity: 0.15,
    price: 0.0,
};

/// A deck paired with its computed score.
pub struct Scored<'a> {
    pub deck: &'a Deck,
    pub score: f64,
}

/// Combined score for one deck. `popularity` is the deck's normalized archetype
/// popularity (0..1), supplied by the caller.
pub fn score(deck: &Deck, today: NaiveDate, w: &Weights, popularity: f64) -> f64 {
    w.recency * recency(deck.date, today)
        + w.source * source_reliability(&deck.source)
        + w.result * result_strength(deck.event_type, &deck.result)
        + w.popularity * popularity
}

/// Decks ranked best-first. Deterministic: score desc, then date desc, then id.
/// `popularity` is parallel to `decks` (0..1 each); pass all-zero to ignore it.
pub fn rank_decks<'a>(
    decks: &'a [Deck],
    today: NaiveDate,
    w: &Weights,
    popularity: &[f64],
) -> Vec<Scored<'a>> {
    let mut scored: Vec<Scored<'a>> = decks
        .iter()
        .enumerate()
        .map(|(i, d)| Scored {
            deck: d,
            score: score(d, today, w, popularity.get(i).copied().unwrap_or(0.0)),
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.deck.date.cmp(&a.deck.date))
            .then_with(|| a.deck.id.cmp(&b.deck.id))
    });
    scored
}

/// Exponential decay by age in days; clamped so future-dated decks cap at 1.0.
fn recency(date: NaiveDate, today: NaiveDate) -> f64 {
    let age = (today - date).num_days().max(0) as f64;
    0.5_f64.powf(age / RECENCY_HALF_LIFE_DAYS)
}

fn source_reliability(source: &str) -> f64 {
    match source {
        "wotc-mtgo" => 1.0,
        _ => 0.5,
    }
}

/// Event quality scaled by placement, in 0..1. A winning-record Challenge beats a
/// League 5-0, which beats a Preliminary; a sub-.500 Challenge falls below League.
fn result_strength(event_type: EventType, result: &EventResult) -> f64 {
    let placement = placement_score(result);
    match event_type {
        EventType::Challenge => 0.4 + 0.6 * placement,
        EventType::League => 0.7, // published leagues are undefeated (5-0)
        EventType::Preliminary => 0.2 + 0.4 * placement,
        EventType::Other => 0.15 + 0.35 * placement,
    }
}

/// 0..1 placement proxy: match-win rate, or 0.5 when unknown.
/// ponytail: win-rate proxy; fold in `final_rank` standings if finer ordering is needed.
fn placement_score(result: &EventResult) -> f64 {
    match (result.wins, result.losses) {
        (Some(w), Some(l)) if w + l > 0 => w as f64 / (w + l) as f64,
        _ => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Format;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn deck(
        id: &str,
        date: NaiveDate,
        et: EventType,
        wins: Option<u32>,
        losses: Option<u32>,
    ) -> Deck {
        Deck {
            id: id.to_string(),
            format: Format::Modern,
            source: "wotc-mtgo".to_string(),
            source_url: String::new(),
            date,
            event_type: et,
            result: EventResult {
                rank: None,
                wins,
                losses,
            },
            archetype: None,
            colors: None,
            player: None,
            maindeck: Vec::new(),
            sideboard: Vec::new(),
            est_price: None,
        }
    }

    const TODAY: fn() -> NaiveDate = || NaiveDate::from_ymd_opt(2026, 6, 28).unwrap();

    #[test]
    fn recency_half_life_is_exact() {
        let w = Weights {
            recency: 1.0,
            source: 0.0,
            result: 0.0,
            popularity: 0.0,
            price: 0.0,
        };
        let d = deck("x", ymd(2026, 6, 14), EventType::Challenge, None, None); // 14 days old
        assert!((score(&d, TODAY(), &w, 0.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn popularity_raises_score() {
        let w = Weights {
            recency: 0.0,
            source: 0.0,
            result: 0.0,
            popularity: 1.0,
            price: 0.0,
        };
        let d = deck("x", TODAY(), EventType::Challenge, Some(5), Some(0));
        assert!(score(&d, TODAY(), &w, 0.9) > score(&d, TODAY(), &w, 0.1));
    }

    #[test]
    fn newer_deck_scores_higher() {
        let new = deck(
            "a",
            ymd(2026, 6, 28),
            EventType::Challenge,
            Some(5),
            Some(0),
        );
        let old = deck("b", ymd(2026, 6, 1), EventType::Challenge, Some(5), Some(0));
        assert!(
            score(&new, TODAY(), &DEFAULT_WEIGHTS, 0.0)
                > score(&old, TODAY(), &DEFAULT_WEIGHTS, 0.0)
        );
    }

    #[test]
    fn result_strength_ordering() {
        let w = Weights {
            recency: 0.0,
            source: 0.0,
            result: 1.0,
            popularity: 0.0,
            price: 0.0,
        };
        let t = TODAY();
        let chal_top = deck("c", t, EventType::Challenge, Some(5), Some(0)); // win rate 1.0
        let league = deck("l", t, EventType::League, Some(5), Some(0));
        let prelim_top = deck("p", t, EventType::Preliminary, Some(5), Some(0));
        let chal_weak = deck("w", t, EventType::Challenge, Some(2), Some(3)); // sub-.500

        assert!(score(&chal_top, t, &w, 0.0) > score(&league, t, &w, 0.0));
        assert!(score(&league, t, &w, 0.0) > score(&prelim_top, t, &w, 0.0));
        assert!(score(&league, t, &w, 0.0) > score(&chal_weak, t, &w, 0.0));
    }

    #[test]
    fn ranking_is_ordered_and_deterministic() {
        let decks = vec![
            deck("b", ymd(2026, 6, 1), EventType::League, Some(5), Some(0)),
            deck(
                "a",
                ymd(2026, 6, 28),
                EventType::Challenge,
                Some(7),
                Some(2),
            ),
            deck(
                "c",
                ymd(2026, 6, 20),
                EventType::Preliminary,
                Some(4),
                Some(2),
            ),
        ];
        let pop = vec![0.0; decks.len()];
        let r = rank_decks(&decks, TODAY(), &DEFAULT_WEIGHTS, &pop);
        assert!(r[0].score >= r[1].score && r[1].score >= r[2].score);
        assert_eq!(r[0].deck.id, "a"); // newest + strong challenge ranks first

        let ids = |v: &[Scored]| v.iter().map(|s| s.deck.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&r),
            ids(&rank_decks(&decks, TODAY(), &DEFAULT_WEIGHTS, &pop))
        );
    }
}
