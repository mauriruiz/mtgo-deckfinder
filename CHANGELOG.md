# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Phase 1: real `WotcMtgoSource` (`DeckSource` trait) fetching recent decklists
  from mtgo.com via the per-event embedded JSON; handles both tournament
  (Challenge/Preliminary) and league (5-0) schemas.
- Polite blocking HTTP client (`PoliteClient`): descriptive User-Agent, ≥2 s
  rate limiting, retry-with-backoff.
- MTGJSON-backed card-name validation (`NameReference`): bulk `AtomicCards`
  download verified against its `.sha256`, cached locally; normalization handles
  whitespace, accents (NFC), casing, and split/DFC/adventure names. Unknown
  names are surfaced as warnings on fetch.
- Timestamped disk `Cache` (per-OS cache dir): cache-first reads, `--refresh`
  flag, staleness thresholds.
- `fetch <format>` populates the cache from real data; GitHub Actions CI
  (`fmt` / `clippy -D warnings` / `test`).

- Phase 0: source-agnostic data model (`Deck`, `CardEntry`, `Format`,
  `EventType`, `EventResult`, `Color`) with `serde`.
- Pure MTGO-text exporter (`export_mtgo_txt`): maindeck block, blank line,
  sideboard block; special card names pass through verbatim.
- `DeckStore` trait with a flat-JSON-file implementation (`JsonStore`).
- CLI scaffold (`fetch` / `list` / `export`); `export --sample` writes the
  built-in sample Pauper deck to MTGO-importable text.
