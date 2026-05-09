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
//! | `Tools/DBTruncater/Run.hs`                           | `run.rs` (R349 — pending)    |
//! | `app/db-truncater.hs::main`                          | `main.rs`                    |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
pub fn run_main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parser::parse_args(&argv) {
        Ok(args) => match parser::into_config(&args) {
            Ok(_config) => match run() {
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

/// Concrete run-loop entry.
///
/// R348 lands argv → `DBTruncaterConfig` validation; the actual
/// `Run.hs`-equivalent ChainDB-open + truncate dispatch lands at R349
/// using the `ImmutableStore::trim_after_slot` primitive added at R347.
pub fn run() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-db-truncater: ChainDB open + truncate dispatch not yet \
         implemented (R348 ships argv → DBTruncaterConfig validation; R349 \
         lands the Run.hs equivalent using ImmutableStore::trim_after_slot)."
    ))
}
