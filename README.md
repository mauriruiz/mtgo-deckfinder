# mtgo-deckfinder

A small, CLI-first Rust utility to find recent competitive **Magic: The Gathering
Online (MTGO)** decklists, rank them, and export them as MTGO-importable text.

> **Status: Phase 2** — fetches and caches real decklists, validates card names,
> ranks them, and exports the one you pick.

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
| `fetch <format> [--refresh]` | Fetch recent decklists for a format and cache them. |
| `list <format> [--limit N]` | List cached decks ranked best-first. |
| `export <format> <rank> [--out PATH]` | Export the nth-ranked deck (default `deck.txt`). |
| `export --sample [--out PATH]` | Export the built-in sample deck. |

Formats: `standard`, `modern`, `pauper`, `pioneer`, `vintage`, `legacy`,
`limited`, `duel-commander`, `premodern`, `contraption`.

## Ranking

`list`/`export` order decks by a pure, deterministic score combining three
factors (each normalized to 0..1, weighted in one place — `rank::DEFAULT_WEIGHTS`):

- **Recency** — exponential decay, 14-day half-life.
- **Source reliability** — per-source constant (WotC = 1.0).
- **Result strength** — event type × placement: a winning-record Challenge beats
  a League 5-0, which beats a Preliminary.

Popularity (Phase 3) and price (Phase 4) are reserved as weight hooks.

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
- **Card names:** the [MTGJSON](https://mtgjson.com) `AtomicCards` bulk file
  (MIT-licensed), downloaded once, verified against its published `.sha256`, and
  cached locally. Every exported card name is validated against this set so
  export never silently emits a mangled name.

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
