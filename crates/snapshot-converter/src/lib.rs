#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `snapshot-converter`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `snapshot-converter` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R353 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `app/snapshot-converter.hs` section          | Yggdrasil `.rs`                 |
//! |-------------------------------------------------------|---------------------------------|
//! | `data Config` / `data Snapshot'` / supporting types   | `types.rs`                      |
//! | `parseConfig` (optparse-applicative)                  | `parser.rs`                     |
//! | `convertSnapshot` (LedgerDB conversion logic)         | `convert.rs` (pending; carve-out)|
//! | `withManager` / `watchTree` daemon                    | `daemon.rs` (pending)           |
//! | `main`                                                | `main.rs`                       |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R363 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`types::Config`] is handed to [`run`]; `--help`
/// and `--version` short-circuit with byte-equivalent upstream output.
pub fn run_main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let config = match parser::parse_args(&argv) {
        Ok(config) => config,
        Err(parser::ParseError::HelpRequested) => {
            let _ = std::io::stdout().write_all(parser::HELP_TEXT.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(parser::ParseError::VersionRequested) => {
            let _ = std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            return ExitCode::FAILURE;
        }
    };
    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry.
///
/// R363 lands argv → [`types::Config`] dispatch. The actual mem↔lsm
/// conversion + filesystem-watcher daemon land in subsequent rounds
/// per the per-tool roadmap (gated on yggdrasil-format LedgerStore
/// reader/writer being available).
pub fn run(config: &types::Config) -> eyre::Result<()> {
    use types::Config;
    let mode = match config {
        Config::Daemon { .. } => "daemon",
        Config::Oneshot { .. } => "oneshot",
    };
    Err(eyre::eyre!(
        "yggdrasil-snapshot-converter: {mode} mode dispatch not yet implemented \
         (R363 ships argv → Config dispatch; convertSnapshot LSM/Mem logic + \
         filesystem-watcher daemon land in subsequent rounds gated on \
         yggdrasil-format LedgerStore reader/writer being available)."
    ))
}
