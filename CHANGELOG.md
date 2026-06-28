# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Phase 0: source-agnostic data model (`Deck`, `CardEntry`, `Format`,
  `EventType`, `EventResult`, `Color`) with `serde`.
- Pure MTGO-text exporter (`export_mtgo_txt`): maindeck block, blank line,
  sideboard block; special card names pass through verbatim.
- `DeckStore` trait with a flat-JSON-file implementation (`JsonStore`).
- CLI scaffold (`fetch` / `list` / `export`); `export --sample` writes the
  built-in sample Pauper deck to MTGO-importable text.
