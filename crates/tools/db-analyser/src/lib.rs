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
//! Layout mapping (R374 ships analysis/benchmark_ledger_ops/slot_data_point.rs;
//! later rounds populate the rest):
//!
//! | Upstream `.hs`                                                          | Yggdrasil `.rs`                                       |
//! |-------------------------------------------------------------------------|-------------------------------------------------------|
//! | `Tools/DBAnalyser/Types.hs`                                             | `types.rs`                                            |
//! | `app/DBAnalyser/Parsers.hs`                                             | `parser.rs`                                           |
//! | `Tools/DBAnalyser/HasAnalysis.hs`                                       | `has_analysis.rs`                                     |
//! | `Tools/DBAnalyser/Analysis.hs`                                          | `analysis.rs` shell (body pending)                    |
//! | `Tools/DBAnalyser/CSV.hs`                                               | `csv.rs`                                              |
//! | `Tools/DBAnalyser/Run.hs`                                               | `run.rs` (pending)                                    |
//! | `Tools/DBAnalyser/Analysis/BenchmarkLedgerOps/SlotDataPoint.hs`         | `analysis/benchmark_ledger_ops/slot_data_point.rs`    |
//! | `Tools/DBAnalyser/Analysis/BenchmarkLedgerOps/Metadata.hs`              | `analysis/benchmark_ledger_ops/metadata.rs` (pending) |
//! | `Tools/DBAnalyser/Analysis/BenchmarkLedgerOps/FileWriting.hs`           | `analysis/benchmark_ledger_ops/file_writing.rs` (pending) |

use std::io::Write;
use std::process::ExitCode;

pub mod analysis;
pub mod csv;
pub mod has_analysis;
pub mod parser;
pub mod status;
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
    Err(RunError::AnalysisDispatchDeferred {
        db: config.db_dir.display().to_string(),
        analysis: format!("{:?}", config.analysis),
        backend: format!("{:?}", config.ldb_backend),
        limit: format!("{:?}", config.conf_limit),
    }
    .into())
}

/// Errors from the db-analyser `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Per-era HasAnalysis + Analysis.hs dispatch is deferred.
    /// Mirror of upstream `Cardano.Tools.DBAnalyser.{HasAnalysis,
    /// Analysis, Run}` — gated on yggdrasil's per-era
    /// ImmutableStore block-iteration surface (Phase B.2 per the
    /// playful-tickling-plum.md plan).
    #[error(
        "yggdrasil-db-analyser: per-era HasAnalysis + Analysis.hs dispatch deferred \
         (see crates/db-analyser/src/status.rs::analysis_dispatch_status for the full \
         deferral rationale). Resolved CLI: db={db}, analysis={analysis}, backend={backend}, \
         limit={limit}."
    )]
    AnalysisDispatchDeferred {
        /// Path to the ChainDB the operator supplied.
        db: String,
        /// Analysis-name rendering.
        analysis: String,
        /// Ledger-DB backend rendering.
        backend: String,
        /// Conf-limit rendering.
        limit: String,
    },
}
