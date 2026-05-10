#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Pure-Rust port of upstream `db-analyser`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell + R335-pattern
//! file-mirror + CLI-parser skeleton for the `db-analyser` sister-tool crate.
//! Per-leaf module mirrors land in subsequent rounds per the
//! Sister-Tools Pure-Rust Port plan.
//!
//! Layout mapping (R351 ships types.rs; later rounds populate the rest):
//!
//! | Upstream `.hs`                                       | Yggdrasil `.rs`              |
//! |------------------------------------------------------|------------------------------|
//! | `Tools/DBAnalyser/Types.hs`                          | `types.rs`                   |
//! | `app/DBAnalyser/Parsers.hs`                          | `parser.rs`                  |
//! | `Tools/DBAnalyser/HasAnalysis.hs`                    | `has_analysis.rs` (pending)  |
//! | `Tools/DBAnalyser/Analysis.hs`                       | `analysis.rs` (pending)      |
//! | `Tools/DBAnalyser/CSV.hs`                            | `csv.rs` (pending)           |
//! | `Tools/DBAnalyser/Run.hs`                            | `run.rs` (pending)           |

use std::io::Write;
use std::process::ExitCode;

pub mod parser;
pub mod types;

/// Process-exit-code wrapper around the run-loop dispatch.
///
/// R365 wires the typed parser dispatcher end-to-end. On successful
/// parse the resolved [`types::DBAnalyserConfig`] is handed to
/// [`run`]; `--help` and `--version` short-circuit with byte-equivalent
/// upstream output.
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
/// R365 lands argv → [`types::DBAnalyserConfig`] dispatch. The actual
/// per-era HasAnalysis surface + Analysis.hs dispatch (1057 upstream
/// lines covering 13 analysis-name variants) + CSV writers + Run.hs
/// supervisor land in subsequent rounds per the per-tool roadmap.
pub fn run(config: &types::DBAnalyserConfig) -> eyre::Result<()> {
    Err(eyre::eyre!(
        "yggdrasil-db-analyser: per-era HasAnalysis + Analysis.hs dispatch \
         not yet implemented (R365 ships argv → DBAnalyserConfig validation; \
         later rounds wire the per-era block iterator + 13-variant analysis \
         dispatch). Resolved: db={}, analysis={:?}, backend={:?}, limit={:?}.",
        config.db_dir.display(),
        config.analysis,
        config.ldb_backend,
        config.conf_limit,
    ))
}
