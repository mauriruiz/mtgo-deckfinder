# mtgo-deckfinder

A small, CLI-first Rust utility to find recent competitive **Magic: The Gathering
Online (MTGO)** decklists, rank them, and export them as MTGO-importable text.

> **Status: Phase 4** — everything in Phase 3 plus approximate MTGO prices and
> collection-aware views: import your MTGO collection to see what's cheapest to
> *complete* and what you can build right now.

> **New to the command line?** See **[HOW_TO_USE.md](HOW_TO_USE.md)** — a plain-language,
> copy-paste runbook that needs no coding knowledge.

## Quickstart

```sh
cargo run -- fetch modern          # fetch + cache recent Modern decks
cargo run -- list modern           # show them ranked, best first
cargo run -- export modern 1       # export the #1 deck to deck.txt
```

The first `fetch` downloads the MTGJSON card-name reference (~50 MB, cached
afterwards). Subsequent fetches reuse the local cache until it goes stale or you
pass `--refresh`.

## Commands

| Command | Description |
|---------|-------------|
| `fetch <format> [--refresh]` | Fetch recent decklists (and prices) for a format and cache them. |
| `import-collection <file.csv>` | Load your MTGO collection export to enable collection-aware views. |
| `list <format> [--limit N] [--colors WUBRG] [--color-match MODE] [--view VIEW]` | List cached decks under a selection view. |
| `export <format> <rank> [--colors WUBRG] [--view VIEW] [--out PATH]` | Export the nth deck in that view (default `deck.txt`). |
| `export --sample [--out PATH]` | Export the built-in sample deck. |

Formats: `standard`, `modern`, `pauper`, `pioneer`, `vintage`, `legacy`,
`limited`, `duel-commander`, `premodern`, `contraption`.

Views (`--view`): `ranked` (default, by strength), `archetypes` (most popular
first), `buildable` (only decks you own — needs a collection), `cheapest` (lowest
cost among competitive decks), `balance` (best strength-for-cost).

## Ranking

`list`/`export` order decks by a pure, deterministic score combining four factors
(each normalized to 0..1, weighted in one place — `rank::DEFAULT_WEIGHTS`):

- **Recency** — exponential decay, 14-day half-life.
- **Source reliability** — per-source constant (WotC = 1.0).
- **Result strength** — event type × placement: a winning-record Challenge beats
  a League 5-0, which beats a Preliminary.
- **Popularity** — size of the deck's archetype cluster.

Price (Phase 4) is reserved as a weight hook.

## Colors & archetypes

Each deck's colors are derived from its cards' color identity (via the MTGJSON
reference). Filter with `--colors` (any of `WUBRG`) and choose how it matches:

- `--color-match subset` (default) — decks that fit *within* your colors (a `UR`
  filter shows mono-U, mono-R, and UR decks). Best for "decks I can build".
- `--color-match exact` — decks that are exactly those colors.
- `--color-match includes` — decks that contain at least those colors.

Decks are grouped into archetypes by maindeck card overlap (single-linkage at
≥80% shared cards), each labeled by its most common non-land cards.
`list --archetypes` shows the largest archetypes first, with a best representative
deck for each.

```sh
cargo run -- list modern --colors UR              # UR (or within) decks, ranked
cargo run -- list modern --view archetypes        # most popular archetypes
cargo run -- export modern 1 --colors UR          # export the best UR deck
```

## Prices & your collection

`fetch` also estimates each card's MTGO price (in **tix**) from Scryfall, so
`list` shows an approximate total price per deck. **All prices are estimates.**

Import your MTGO collection to unlock the more useful views — the central
question being *"what's the best deck I can play for the least added cost?"*:

```sh
cargo run -- import-collection my-collection.csv   # MTGO → Export Collection (CSV)
cargo run -- list modern --view buildable          # decks you can build right now
cargo run -- list modern --view cheapest           # cheapest to COMPLETE (only missing cards)
cargo run -- list modern --view balance            # strongest per tix
cargo run -- export modern 1 --view cheapest        # export the cheapest-to-complete deck
```

With a collection loaded, `list` adds **miss** (missing card copies, quantity-
aware) and **+tix** (cost to buy only those copies) columns, and `cheapest`
sorts by cost-to-complete. Without a collection, `cheapest`/`balance` fall back
to total deck price — the tool is fully usable either way.

The collection file is MTGO's own collection CSV export (a `Card Name` and a
`Quantity` column; other columns ignored, foil/non-foil summed). The exact
assumed format is documented in `src/collection.rs` and easy to adjust.

## Export format

MTGO text: one `"<qty> <name>"` per line — the maindeck block, a blank line, then
the sideboard block (omitted when empty). Card names are emitted verbatim as
canonical MTGO names, so split (`Wear/Tear`), DFC, adventure, and accented names
pass through unchanged.

## Data sources & politeness

- **Decklists:** official MTGO published decklists at
  [mtgo.com/decklists](https://www.mtgo.com/decklists). The per-event JSON
  embedded in each page is parsed (more stable than the rendered HTML). Requests
  carry a descriptive User-Agent, are rate-limited (≥2 s apart), and retry
  transient failures with backoff. Only recent events are fetched, then cached.
- **Card names / colors:** the [MTGJSON](https://mtgjson.com) `AtomicCards` bulk
  file (MIT-licensed), downloaded once, verified against its published `.sha256`,
  and cached locally. Provides name validation, color identity, and land flags.
- **Prices:** [Scryfall](https://scryfall.com) `prices.tix` via its batched
  `/cards/collection` endpoint — only the cards in the cached decks are looked up
  (a handful of requests), cached locally, and labeled approximate. GoatBots is
  *not* used: its price files are `robots.txt`-disallowed and Cloudflare-gated,
  so fetching them would breach polite-sourcing rules.

Cache location: `<OS cache dir>/mtgo-deckfinder` (e.g. `~/Library/Caches/...`).

## Scope & constraints

This tool **only creates/exports decklists** — it never automates or controls the
MTGO client and never acquires cards. Price estimates (a later phase) will always
be labeled approximate. Sources are used politely and within their stated terms.

### Known limitations

- Cards newer than the cached MTGJSON build are flagged with a validation
  warning (their names still export verbatim — they are valid MTGO names). Run
  `fetch --refresh` once MTGJSON catches up.
- The largest league pages on mtgo.com occasionally return empty responses; such
  events are skipped with a warning and picked up on a later fetch.

## Development

```sh
cargo fmt
cargo clippy -- -D warnings
cargo test          # all parser/normalizer tests run offline against fixtures
```

The core (model, exporter, name validation, future ranker) is pure and
unit-tested without network or filesystem; sources, caching, and IO sit behind
traits at the edges.

## License

[MIT](LICENSE)
