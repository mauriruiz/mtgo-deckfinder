//! `mtgo-deckfinder` CLI. Phase 0: only `export --sample` does real work;
//! `fetch` and `list` are stubs for Phase 1 and Phase 2.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use mtgo_deckfinder::{export_mtgo_txt, sample_deck};

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

#[derive(Subcommand)]
enum Command {
    /// Fetch recent decklists for a format (Phase 1 — not yet implemented).
    Fetch {
        /// Format, e.g. `pauper`.
        format: String,
    },
    /// List ranked cached decks (Phase 2 — not yet implemented).
    List {
        /// Format, e.g. `pauper`.
        format: String,
    },
    /// Export a deck to MTGO-importable text. Phase 0 supports `--sample` only.
    Export {
        /// Export the built-in sample deck.
        #[arg(long)]
        sample: bool,
        /// Output file path.
        #[arg(long, short, default_value = "deck.txt")]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Fetch { format } => {
            println!("fetch {format}: not yet implemented (Phase 1)");
        }
        Command::List { format } => {
            println!("list {format}: not yet implemented (Phase 2)");
        }
        Command::Export { sample, out } => {
            if !sample {
                bail!("Phase 0 supports only `export --sample`");
            }
            let text = export_mtgo_txt(&sample_deck());
            std::fs::write(&out, &text)?;
            println!("Wrote {} ({} bytes)", out.display(), text.len());
        }
    }
    Ok(())
}
