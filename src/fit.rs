//! Collection-fitting: turn a near-buildable proven deck into a buildable one by
//! swapping a few missing cards for interchangeable cards the user already owns.
//!
//! This is **not** a deckbuilder. It starts from a real decklist and makes only
//! local, labeled substitutions; it never originates a card the user does not own.
//! The unmodified deck stays the default — fitting is opt-in (`export --fit`).
//!
//! Substitution decisions go through the [`SubstitutionProvider`] trait. Part A
//! ships [`LandFitProvider`] (a curated, keyless Tier-1 land table). Part B (an
//! LLM provider for spells) plugs in behind the same trait later; the candidate
//! gathering and answer-validation here are provider-agnostic, so a model can
//! only ever pick from owned candidates — a hallucinated or unowned pick is
//! rejected by [`fit_deck`] itself.

use std::collections::{BTreeSet, HashMap};

use crate::cards::{CardReference, is_basic_land, lookup_key};
use crate::collection::Collection;
use crate::model::{CardEntry, Color, Deck};
use crate::price::PriceTable;

/// Where a substitution came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubSource {
    /// A high-confidence interchangeable land from the curated table.
    Tier1Land,
    /// An approximate AI suggestion (Part B).
    Ai,
}

/// A provider's chosen replacement for a missing card.
#[derive(Debug, Clone)]
pub struct Substitution {
    pub replacement: String,
    pub source: SubSource,
}

/// What a provider needs to pick a substitute for one missing card.
pub struct SubRequest<'a> {
    pub missing: &'a str,
    pub is_land: bool,
    pub colors: &'a BTreeSet<Color>,
    pub deck_archetype: &'a str,
    /// Owned, available, on-color candidate card names to choose from.
    pub candidates: &'a [String],
}

/// Chooses an owned substitute for a missing card, or `None` to leave a gap.
pub trait SubstitutionProvider {
    fn substitute(&self, req: &SubRequest) -> Option<Substitution>;
}

/// One applied swap: `copies` of `removed` replaced by `copies` of `added`.
#[derive(Debug, Clone, PartialEq)]
pub struct Swap {
    pub removed: String,
    pub added: String,
    pub copies: u32,
    pub source: SubSource,
}

/// A card still missing after fitting.
#[derive(Debug, Clone, PartialEq)]
pub struct RemainingGap {
    pub name: String,
    pub copies: u32,
    pub tix: Option<f64>,
}

/// The result of fitting: the modified deck plus a full account of what changed.
#[derive(Debug, Clone)]
pub struct FittedDeck {
    pub deck: Deck,
    pub swaps: Vec<Swap>,
    pub remaining: Vec<RemainingGap>,
}

/// Owned card available to donate to a substitution.
struct Avail {
    name: String,
    colors: BTreeSet<Color>,
    is_land: bool,
    qty: u32,
}

/// Fit `deck` to `collection`: substitute missing cards with owned equivalents
/// the `provider` chooses. Pure — the provider is the only (injected) edge.
pub fn fit_deck(
    deck: &Deck,
    collection: &Collection,
    cards: &CardReference,
    prices: &PriceTable,
    provider: &dyn SubstitutionProvider,
) -> FittedDeck {
    let deck_colors = deck.colors.clone().unwrap_or_default();
    let archetype = deck.archetype.as_deref().unwrap_or("");

    // What the deck needs (by key), excluding free basics; deterministic order.
    let mut deck_usage: HashMap<String, u32> = HashMap::new();
    let mut needed: HashMap<String, (String, u32)> = HashMap::new();
    for card in deck.maindeck.iter().chain(&deck.sideboard) {
        let k = lookup_key(&card.name);
        *deck_usage.entry(k.clone()).or_insert(0) += card.quantity;
        if !is_basic_land(&card.name) {
            let e = needed.entry(k).or_insert((card.name.clone(), 0));
            e.1 += card.quantity;
        }
    }

    // Owned cards available to donate: owned − used in this deck, never basics,
    // never a card the deck already runs.
    let mut avail: HashMap<String, Avail> = HashMap::new();
    for (k, owned) in collection.entries() {
        if is_basic_land(k) || deck_usage.contains_key(k) {
            continue;
        }
        let Some(info) = cards.info(k) else { continue };
        if owned > 0 {
            avail.insert(
                k.to_string(),
                Avail {
                    name: info.name.clone(),
                    colors: info.colors.clone(),
                    is_land: info.is_land,
                    qty: owned,
                },
            );
        }
    }

    let mut order: Vec<&String> = needed.keys().collect();
    order.sort();

    let mut swaps = Vec::new();
    let mut remaining = Vec::new();
    for k in order {
        let (name, total) = &needed[k];
        let mut missing = total.saturating_sub(collection.owned(name));
        if missing == 0 {
            continue;
        }

        let (is_land, colors) = cards
            .info(name)
            .map(|i| (i.is_land, i.colors.clone()))
            .unwrap_or((false, BTreeSet::new()));

        // On-color, same-kind owned candidates the provider may choose from.
        let candidate_names: Vec<String> = avail
            .values()
            .filter(|a| a.qty > 0 && a.is_land == is_land && a.colors.is_subset(&deck_colors))
            .map(|a| a.name.clone())
            .collect();

        if !candidate_names.is_empty() {
            let req = SubRequest {
                missing: name,
                is_land,
                colors: &colors,
                deck_archetype: archetype,
                candidates: &candidate_names,
            };
            if let Some(sub) = provider.substitute(&req) {
                let rk = lookup_key(&sub.replacement);
                // Validate: the pick must be an owned, available candidate.
                let valid = candidate_names.iter().any(|c| lookup_key(c) == rk);
                if let (true, Some(donor)) = (valid, avail.get_mut(&rk)) {
                    let take = missing.min(donor.qty);
                    if take > 0 {
                        donor.qty -= take;
                        swaps.push(Swap {
                            removed: name.clone(),
                            added: sub.replacement,
                            copies: take,
                            source: sub.source,
                        });
                        missing -= take;
                    }
                }
            }
        }

        if missing > 0 {
            remaining.push(RemainingGap {
                name: name.clone(),
                copies: missing,
                tix: prices.get(name),
            });
        }
    }

    remaining.sort_by(|a, b| a.name.cmp(&b.name));
    let mut fitted = deck.clone();
    apply_swaps(&mut fitted, &swaps);
    FittedDeck {
        deck: fitted,
        swaps,
        remaining,
    }
}

/// Apply swaps to a deck, distributing each across the maindeck then sideboard.
fn apply_swaps(deck: &mut Deck, swaps: &[Swap]) {
    for sw in swaps {
        let mut left = sw.copies;
        for block in [&mut deck.maindeck, &mut deck.sideboard] {
            if left == 0 {
                break;
            }
            let take = match block
                .iter_mut()
                .find(|e| lookup_key(&e.name) == lookup_key(&sw.removed))
            {
                Some(e) => {
                    let t = left.min(e.quantity);
                    e.quantity -= t;
                    t
                }
                None => 0,
            };
            if take > 0 {
                left -= take;
                match block
                    .iter_mut()
                    .find(|e| lookup_key(&e.name) == lookup_key(&sw.added))
                {
                    Some(e) => e.quantity += take,
                    None => block.push(CardEntry {
                        name: sw.added.clone(),
                        quantity: take,
                    }),
                }
            }
        }
    }
    deck.maindeck.retain(|e| e.quantity > 0);
    deck.sideboard.retain(|e| e.quantity > 0);
}

// ---- Part A: curated Tier-1 land equivalents ----

/// Curated groups of interchangeable dual lands (one group per color pair). A
/// dictionary, not a heuristic — extend by hand. All Pioneer-legal.
const LAND_GROUPS: &[&[&str]] = &[
    &["Hallowed Fountain", "Glacial Fortress", "Hengegate Pathway"], // WU
    &["Watery Grave", "Drowned Catacomb", "Clearwater Pathway"],     // UB
    &["Blood Crypt", "Dragonskull Summit", "Blightstep Pathway"],    // BR
    &["Stomping Ground", "Rootbound Crag", "Cragcrown Pathway"],     // RG
    &["Temple Garden", "Sunpetal Grove", "Branchloft Pathway"],      // GW
    &["Godless Shrine", "Isolated Chapel", "Brightclimb Pathway"],   // WB
    &["Steam Vents", "Sulfur Falls", "Riverglide Pathway"],          // UR
    &["Overgrown Tomb", "Woodland Cemetery", "Darkbore Pathway"],    // BG
    &["Sacred Foundry", "Clifftop Retreat", "Needleverge Pathway"],  // RW
    &["Breeding Pool", "Hinterland Harbor", "Barkchannel Pathway"],  // GU
];

/// The Tier-1 land-equivalence table.
pub struct LandTable {
    groups: Vec<Vec<String>>,
}

impl LandTable {
    /// The built-in curated table.
    pub fn seed() -> Self {
        Self {
            groups: LAND_GROUPS
                .iter()
                .map(|g| g.iter().map(|s| (*s).to_string()).collect())
                .collect(),
        }
    }

    /// Lands interchangeable with `name` (its group, minus itself), in table order.
    fn equivalents(&self, name: &str) -> impl Iterator<Item = &str> {
        let target = lookup_key(name);
        self.groups
            .iter()
            .find(|g| g.iter().any(|l| lookup_key(l) == target))
            .into_iter()
            .flatten()
            .map(String::as_str)
            .filter(move |l| lookup_key(l) != target)
    }
}

/// Keyless default provider: swaps a missing land for an owned Tier-1 equivalent.
pub struct LandFitProvider {
    table: LandTable,
}

impl LandFitProvider {
    pub fn new(table: LandTable) -> Self {
        Self { table }
    }
}

impl SubstitutionProvider for LandFitProvider {
    fn substitute(&self, req: &SubRequest) -> Option<Substitution> {
        if !req.is_land {
            return None;
        }
        // First table-equivalent the user owns (candidates are owned cards).
        for equivalent in self.table.equivalents(req.missing) {
            if let Some(owned) = req
                .candidates
                .iter()
                .find(|c| lookup_key(c) == lookup_key(equivalent))
            {
                return Some(Substitution {
                    replacement: owned.clone(),
                    source: SubSource::Tier1Land,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardRecord;
    use crate::collection::parse_collection_csv;
    use crate::model::{EventResult, EventType, Format};

    fn reference() -> CardReference {
        let dual = |n: &str, c: &[Color]| CardRecord {
            name: n.into(),
            colors: c.into(),
            is_land: true,
        };
        let spell = |n: &str, c: &[Color]| CardRecord {
            name: n.into(),
            colors: c.into(),
            is_land: false,
        };
        CardReference::from_records(&[
            dual("Steam Vents", &[Color::U, Color::R]),
            dual("Sulfur Falls", &[Color::U, Color::R]),
            dual("Riverglide Pathway", &[Color::U, Color::R]),
            spell("Lightning Bolt", &[Color::R]),
            spell("Counterspell", &[Color::U]),
            spell("Snapcaster Mage", &[Color::U]),
        ])
    }

    fn deck(main: &[(&str, u32)]) -> Deck {
        Deck {
            id: "t".into(),
            format: Format::Pioneer,
            source: "wotc-mtgo".into(),
            source_url: String::new(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 6, 28).unwrap(),
            event_type: EventType::Challenge,
            result: EventResult::default(),
            archetype: Some("Izzet".into()),
            colors: Some(BTreeSet::from([Color::U, Color::R])),
            player: None,
            maindeck: main
                .iter()
                .map(|(n, q)| CardEntry {
                    name: (*n).into(),
                    quantity: *q,
                })
                .collect(),
            sideboard: Vec::new(),
            est_price: None,
        }
    }

    /// Mock AI-style provider that always names a fixed replacement.
    struct MockProvider(Option<&'static str>);
    impl SubstitutionProvider for MockProvider {
        fn substitute(&self, _req: &SubRequest) -> Option<Substitution> {
            self.0.map(|r| Substitution {
                replacement: r.into(),
                source: SubSource::Ai,
            })
        }
    }

    #[test]
    fn swaps_missing_land_for_owned_tier1_equivalent() {
        // Owns 4 Sulfur Falls (UR), deck wants 4 Steam Vents it does not own.
        let coll = parse_collection_csv("Card Name,Quantity\nSulfur Falls,4\n").unwrap();
        let provider = LandFitProvider::new(LandTable::seed());
        let d = deck(&[("Steam Vents", 4), ("Lightning Bolt", 4)]);

        let fitted = fit_deck(
            &d,
            &coll,
            &reference(),
            &PriceTable::from_pairs([]),
            &provider,
        );

        assert_eq!(fitted.swaps.len(), 1);
        let s = &fitted.swaps[0];
        assert_eq!(
            (s.removed.as_str(), s.added.as_str(), s.copies, s.source),
            ("Steam Vents", "Sulfur Falls", 4, SubSource::Tier1Land)
        );
        // Lightning Bolt is the only remaining gap; the land gap is closed.
        assert_eq!(
            fitted
                .remaining
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["Lightning Bolt"]
        );
        // Fitted deck runs Sulfur Falls, not Steam Vents.
        assert!(
            fitted
                .deck
                .maindeck
                .iter()
                .any(|c| c.name == "Sulfur Falls" && c.quantity == 4)
        );
        assert!(fitted.deck.maindeck.iter().all(|c| c.name != "Steam Vents"));
        // Original deck untouched.
        assert!(d.maindeck.iter().any(|c| c.name == "Steam Vents"));
    }

    #[test]
    fn basics_are_never_substituted() {
        let coll = parse_collection_csv("Card Name,Quantity\nSulfur Falls,4\n").unwrap();
        let provider = LandFitProvider::new(LandTable::seed());
        // Deck needs Mountains it does not own — basics are free, never a gap/swap.
        let d = deck(&[("Mountain", 20)]);
        let fitted = fit_deck(
            &d,
            &coll,
            &reference(),
            &PriceTable::from_pairs([]),
            &provider,
        );
        assert!(fitted.swaps.is_empty());
        assert!(fitted.remaining.is_empty());
    }

    #[test]
    fn ai_pick_outside_candidates_is_rejected() {
        let coll = parse_collection_csv("Card Name,Quantity\nSnapcaster Mage,4\n").unwrap();
        // Model "hallucinates" a card not in the owned candidate list.
        let provider = MockProvider(Some("Brainstorm"));
        let d = deck(&[("Counterspell", 4)]); // missing spell
        let fitted = fit_deck(
            &d,
            &coll,
            &reference(),
            &PriceTable::from_pairs([]),
            &provider,
        );
        assert!(
            fitted.swaps.is_empty(),
            "non-candidate pick must be rejected"
        );
        assert_eq!(
            fitted
                .remaining
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["Counterspell"]
        );
    }

    #[test]
    fn ai_pick_from_candidates_is_accepted_and_flagged() {
        let coll = parse_collection_csv("Card Name,Quantity\nSnapcaster Mage,4\n").unwrap();
        let provider = MockProvider(Some("Snapcaster Mage")); // owned, on-color, non-land
        let d = deck(&[("Counterspell", 4)]);
        let fitted = fit_deck(
            &d,
            &coll,
            &reference(),
            &PriceTable::from_pairs([]),
            &provider,
        );
        assert_eq!(fitted.swaps.len(), 1);
        assert_eq!(
            (fitted.swaps[0].added.as_str(), fitted.swaps[0].source),
            ("Snapcaster Mage", SubSource::Ai)
        );
    }

    #[test]
    fn none_suitable_leaves_an_honest_gap() {
        let coll = parse_collection_csv("Card Name,Quantity\nSnapcaster Mage,4\n").unwrap();
        let provider = MockProvider(None);
        let d = deck(&[("Counterspell", 4)]);
        let fitted = fit_deck(
            &d,
            &coll,
            &reference(),
            &PriceTable::from_pairs([("Counterspell".to_string(), 2.5)]),
            &provider,
        );
        assert!(fitted.swaps.is_empty());
        assert_eq!(
            fitted.remaining[0],
            RemainingGap {
                name: "Counterspell".into(),
                copies: 4,
                tix: Some(2.5)
            }
        );
    }
}
