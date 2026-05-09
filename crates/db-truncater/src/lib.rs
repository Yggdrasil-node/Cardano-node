#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `db-truncater`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `db-truncater` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping:
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Tools/DBTruncater/Types.hs`                         | `types.rs`                   |
//! | `app/DBTruncater/Parsers.hs`                         | `parser.rs`                  |
//! | `Tools/DBTruncater/Run.hs`                           | `run.rs`                     |
//! | `app/db-truncater.hs::main`                          | `main.rs`                    |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod run;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
pub fn run_main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parser::parse_args(&argv) {
        Ok(args) => match parser::into_config(&args) {
            Ok(config) => match self::run(&config) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    let _ = writeln!(std::io::stderr(), "Error: {err}");
                    ExitCode::FAILURE
                }
            },
            Err(err) => {
                let _ = writeln!(std::io::stderr(), "Error: {err}");
                ExitCode::FAILURE
            }
        },
        Err(parser::ParseError::HelpRequested) => {
            let _ = std::io::stdout().write_all(parser::HELP_TEXT.as_bytes());
            ExitCode::SUCCESS
        }
        Err(parser::ParseError::VersionRequested) => {
            let _ = std::io::stdout().write_all(parser::VERSION_TEXT.as_bytes());
            ExitCode::SUCCESS
        }
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Concrete run-loop entry. Opens the configured ChainDB and applies
/// the requested truncation, reporting the resolved slot + the number
/// of blocks removed to stderr.
///
/// Mirror of upstream `Cardano.Tools.DBTruncater.Run.run`.
pub fn run(config: &types::DBTruncaterConfig) -> eyre::Result<()> {
    if config.verbose {
        eprintln!(
            "[db-truncater] opening ChainDB at {}",
            config.db_dir.display()
        );
    }

    let outcome = run::run(config).map_err(|err| eyre::eyre!("truncate failed: {err}"))?;

    if config.verbose {
        eprintln!(
            "[db-truncater] resolved truncate target → slot {}",
            outcome.resolved_slot.0
        );
    }
    eprintln!(
        "[db-truncater] truncated immutable DB at slot {}: {} block(s) removed",
        outcome.resolved_slot.0, outcome.blocks_removed
    );
    Ok(())
}
