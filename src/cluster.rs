//! Archetype clustering by maindeck card overlap. Pure and deterministic.
//!
//! Similarity between two decks is the overlap of their distinct maindeck card
//! names: `|A ∩ B| / max(|A|, |B|)` — the fraction of the larger deck's cards
//! that also appear in the other. Decks at or above [`SIMILARITY_THRESHOLD`] are
//! linked, and clusters are the connected components (single-linkage) of that
//! graph. Each cluster is labeled by its most common non-land cards.

use std::collections::{HashMap, HashSet};

use crate::cards::CardReference;
use crate::model::Deck;

/// Decks sharing at least this fraction of cards are clustered together.
pub const SIMILARITY_THRESHOLD: f64 = 0.8;

/// How many distinctive cards name an archetype.
const LABEL_CARDS: usize = 3;

/// Clustering of a deck slice. All vectors index by cluster id except
/// `cluster_of`, which is parallel to the input decks.
pub struct Clustering {
    /// `cluster_of[i]` = cluster id of input deck `i`.
    pub cluster_of: Vec<usize>,
    /// `labels[c]` = archetype label of cluster `c`.
    pub labels: Vec<String>,
    /// `sizes[c]` = number of decks in cluster `c`.
    pub sizes: Vec<usize>,
}

impl Clustering {
    /// Archetype label for input deck `i`.
    pub fn label_of(&self, i: usize) -> &str {
        &self.labels[self.cluster_of[i]]
    }

    /// Cluster size (popularity) for input deck `i`.
    pub fn size_of(&self, i: usize) -> usize {
        self.sizes[self.cluster_of[i]]
    }
}

/// Cluster decks by maindeck overlap and label each cluster.
/// ponytail: O(n²) pairwise comparison; fine for the few hundred decks per
/// format, revisit (e.g. LSH/minhash) only if a source returns thousands.
pub fn cluster_decks(decks: &[Deck], cards: &CardReference, threshold: f64) -> Clustering {
    let sets: Vec<HashSet<&str>> = decks
        .iter()
        .map(|d| d.maindeck.iter().map(|c| c.name.as_str()).collect())
        .collect();

    let mut uf = UnionFind::new(decks.len());
    for i in 0..decks.len() {
        for j in (i + 1)..decks.len() {
            if similarity(&sets[i], &sets[j]) >= threshold {
                uf.union(i, j);
            }
        }
    }

    // Assign compact cluster ids in first-seen order for determinism.
    let mut root_to_id: HashMap<usize, usize> = HashMap::new();
    let mut cluster_of = vec![0usize; decks.len()];
    for (i, slot) in cluster_of.iter_mut().enumerate() {
        let root = uf.find(i);
        let next = root_to_id.len();
        *slot = *root_to_id.entry(root).or_insert(next);
    }

    let cluster_count = root_to_id.len();
    let mut sizes = vec![0usize; cluster_count];
    for &c in &cluster_of {
        sizes[c] += 1;
    }
    let labels = label_clusters(decks, &cluster_of, cluster_count, cards);

    Clustering {
        cluster_of,
        labels,
        sizes,
    }
}

fn similarity(a: &HashSet<&str>, b: &HashSet<&str>) -> f64 {
    let larger = a.len().max(b.len());
    if larger == 0 {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    shared as f64 / larger as f64
}

/// Label each cluster by its most common non-land maindeck cards.
fn label_clusters(
    decks: &[Deck],
    cluster_of: &[usize],
    cluster_count: usize,
    cards: &CardReference,
) -> Vec<String> {
    // counts[cluster][card] = number of decks in the cluster running the card.
    let mut counts: Vec<HashMap<&str, usize>> = vec![HashMap::new(); cluster_count];
    for (i, deck) in decks.iter().enumerate() {
        let bucket = &mut counts[cluster_of[i]];
        for name in deck.maindeck.iter().map(|c| c.name.as_str()) {
            if !cards.is_land(name) {
                *bucket.entry(name).or_insert(0) += 1;
            }
        }
    }

    counts
        .into_iter()
        .map(|bucket| {
            let mut ranked: Vec<(&str, usize)> = bucket.into_iter().collect();
            // Most frequent first; alphabetical tie-break for determinism.
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            let top: Vec<&str> = ranked
                .into_iter()
                .take(LABEL_CARDS)
                .map(|(n, _)| n)
                .collect();
            if top.is_empty() {
                "Unknown".to_string()
            } else {
                top.join(" / ")
            }
        })
        .collect()
}

/// Minimal union-find with path compression and union by size.
struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        let (big, small) = if self.size[ra] >= self.size[rb] {
            (ra, rb)
        } else {
            (rb, ra)
        };
        self.parent[small] = big;
        self.size[big] += self.size[small];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{CardRecord, CardReference};
    use crate::model::{CardEntry, EventResult, EventType, Format};

    fn reference() -> CardReference {
        let land = |n: &str| CardRecord {
            name: n.into(),
            colors: vec![],
            is_land: true,
        };
        let spell = |n: &str| CardRecord {
            name: n.into(),
            colors: vec![],
            is_land: false,
        };
        CardReference::from_records(&[
            land("Island"),
            land("Mountain"),
            spell("Counterspell"),
            spell("Psychic Frog"),
            spell("Lightning Bolt"),
            spell("Goblin Guide"),
        ])
    }

    fn deck(id: &str, cards: &[&str]) -> Deck {
        Deck {
            id: id.into(),
            format: Format::Modern,
            source: "wotc-mtgo".into(),
            source_url: String::new(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 6, 28).unwrap(),
            event_type: EventType::Challenge,
            result: EventResult::default(),
            archetype: None,
            colors: None,
            player: None,
            maindeck: cards
                .iter()
                .map(|n| CardEntry {
                    name: (*n).into(),
                    quantity: 4,
                })
                .collect(),
            sideboard: Vec::new(),
            est_price: None,
        }
    }

    #[test]
    fn clusters_similar_decks_and_separates_different_ones() {
        let decks = vec![
            deck(
                "a",
                &[
                    "Island",
                    "Counterspell",
                    "Psychic Frog",
                    "Lightning Bolt",
                    "Mountain",
                ],
            ),
            // 4 of 5 cards shared with a → 0.8, clusters together.
            deck(
                "b",
                &[
                    "Island",
                    "Counterspell",
                    "Psychic Frog",
                    "Lightning Bolt",
                    "Goblin Guide",
                ],
            ),
            deck("c", &["Mountain", "Goblin Guide"]), // little overlap
        ];
        let cl = cluster_decks(&decks, &reference(), SIMILARITY_THRESHOLD);

        assert_eq!(cl.cluster_of[0], cl.cluster_of[1]); // a & b together
        assert_ne!(cl.cluster_of[0], cl.cluster_of[2]); // c apart
        assert_eq!(cl.size_of(0), 2);
        assert_eq!(cl.size_of(2), 1);
        // Label excludes lands (Island/Mountain) and is deterministic.
        let label = cl.label_of(0);
        assert!(!label.contains("Island") && !label.contains("Mountain"));
        assert!(label.contains("Counterspell") && label.contains("Psychic Frog"));
    }

    #[test]
    fn is_deterministic() {
        let decks = vec![
            deck("a", &["Island", "Counterspell", "Psychic Frog"]),
            deck("b", &["Island", "Counterspell", "Psychic Frog"]),
        ];
        let first = cluster_decks(&decks, &reference(), SIMILARITY_THRESHOLD);
        let second = cluster_decks(&decks, &reference(), SIMILARITY_THRESHOLD);
        assert_eq!(first.cluster_of, second.cluster_of);
        assert_eq!(first.labels, second.labels);
    }
}
