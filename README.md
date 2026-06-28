# mtgo-deckfinder

A small, CLI-first Rust utility to find recent competitive **Magic: The Gathering
Online (MTGO)** decklists, rank them, and export them as MTGO-importable text.

> **Status: Phase 0** — data model + pure exporter + local store. No network
> sources yet. `fetch`/`list` are stubs landing in later phases.

## Quickstart

```sh
cargo run -- export --sample        # writes deck.txt
```

Open `deck.txt` and import it in MTGO ("Decks → Import").

## Commands

| Command | Status | Description |
|---------|--------|-------------|
| `export --sample [--out PATH]` | ✅ | Export the built-in sample deck (default `deck.txt`). |
| `fetch <format>` | Phase 1 | Fetch recent decklists for a format. |
| `list <format>` | Phase 2 | List ranked cached decks. |

## Export format

MTGO text: one `"<qty> <name>"` per line — the maindeck block, a blank line, then
the sideboard block (omitted when empty). Card names are emitted verbatim as
canonical MTGO names, so split (`Fire // Ice`), DFC, adventure, and accented
names pass through unchanged.

## Scope & constraints

This tool **only creates/exports decklists** — it never automates or controls the
MTGO client and never acquires cards. Price estimates (a later phase) will always
be labeled approximate. Data sources are used politely (caching, rate limiting,
descriptive User-Agent) and within their stated terms.

## Development

```sh
cargo fmt
cargo clippy -- -D warnings
cargo test
```

The core (model, exporter, future ranker) is pure and unit-tested without
network or filesystem; sources, caching, and IO sit behind traits at the edges.

## License

[MIT](LICENSE)
