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
pub mod byron_ebbs;
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
/// R481 closes the R475-R481 arc: opens the operator-supplied
/// ChainDB via [`yggdrasil_storage::FileImmutable::open`], walks
/// the immutable chain via [`yggdrasil_storage::ImmutableStore::suffix_after`],
/// and dispatches through [`analysis::runner::run_analysis`].
///
/// Of the 13 upstream `AnalysisName` variants, 7 are
/// block-iteration-only and ship in this arc (`ShowSlotBlockNo`,
/// `CountBlocks`, `CountTxOutputs`, `ShowBlockHeaderSize`,
/// `ShowBlockTxsSize`, `ShowEBBs`, `OnlyValidation`); the
/// remaining 6 require a ledger-state apply-loop and return a
/// structured `RequiresLedgerStateApplyLoop` error pending a
/// future arc. See [`status::analysis_dispatch_status`] for the
/// full inventory.
pub fn run(config: &types::DBAnalyserConfig) -> eyre::Result<()> {
    use yggdrasil_ledger::Point;
    use yggdrasil_storage::{FileImmutable, ImmutableStore};

    let store = FileImmutable::open(&config.db_dir).map_err(RunError::Storage)?;
    let blocks = store
        .iter_after(&Point::Origin)
        .map_err(RunError::Storage)?;
    let outcome = analysis::runner::run_analysis(config, blocks).map_err(RunError::Analysis)?;
    render_outcome(&outcome)?;
    Ok(())
}

/// Render an [`analysis::runner::AnalysisOutcome`] to stdout in a
/// shape compatible with upstream's per-analysis emission. Each
/// variant prints one line per data point; the cumulative-result
/// variants (`CountBlocks`, `CountTxOutputs`,
/// `ShowBlockHeaderSize`) append a trailing summary line.
fn render_outcome(outcome: &analysis::runner::AnalysisOutcome) -> eyre::Result<()> {
    use analysis::runner::AnalysisOutcome;
    let mut out = std::io::stdout().lock();
    match outcome {
        AnalysisOutcome::ShowSlotBlockNo { lines } => {
            for (slot, block_no, hash) in lines {
                writeln!(
                    out,
                    "slot={} block_no={} hash={}",
                    slot.0,
                    block_no.0,
                    hex_render(&hash.0)
                )?;
            }
        }
        AnalysisOutcome::CountBlocks { total, first, last } => {
            for (label, position) in [("first", first), ("last", last)] {
                if let Some((slot, block_no)) = position {
                    writeln!(out, "{label}: slot={} block_no={}", slot.0, block_no.0)?;
                }
            }
            writeln!(out, "total_blocks={total}")?;
        }
        AnalysisOutcome::CountTxOutputs { total, per_block } => {
            for (slot, n) in per_block {
                writeln!(out, "slot={} tx_outputs={}", slot.0, n)?;
            }
            writeln!(out, "total_tx_outputs={total}")?;
        }
        AnalysisOutcome::ShowBlockHeaderSize {
            max_size,
            per_block,
        } => {
            for (slot, size) in per_block {
                writeln!(out, "slot={} header_size={}", slot.0, size)?;
            }
            writeln!(out, "max_header_size={max_size}")?;
        }
        AnalysisOutcome::ShowBlockTxsSize { per_block } => {
            for (slot, tx_count, total_bytes) in per_block {
                writeln!(
                    out,
                    "slot={} tx_count={} total_bytes={}",
                    slot.0, tx_count, total_bytes
                )?;
            }
        }
        AnalysisOutcome::ShowEBBs { ebbs } => {
            for (slot, hash, prev_hash) in ebbs {
                let prev = match prev_hash {
                    Some(p) => hex_render(&p.0),
                    None => "<origin>".to_string(),
                };
                writeln!(
                    out,
                    "ebb slot={} hash={} prev={}",
                    slot.0,
                    hex_render(&hash.0),
                    prev
                )?;
            }
        }
        AnalysisOutcome::OnlyValidation { blocks_processed } => {
            writeln!(out, "only_validation blocks_processed={blocks_processed}")?;
        }
    }
    Ok(())
}

fn hex_render(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Errors from the db-analyser `run` entry point.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// I/O error opening or reading the operator-supplied ChainDB.
    #[error("yggdrasil-db-analyser: storage error: {0}")]
    Storage(#[from] yggdrasil_storage::StorageError),
    /// Per-era HasAnalysis + Analysis.hs dispatch error. Returns
    /// either a [`analysis::runner::AnalysisError::RequiresLedgerStateApplyLoop`]
    /// when one of the 6 ledger-state-dependent analyses is
    /// requested (pending a future arc).
    #[error("yggdrasil-db-analyser: analysis error: {0}")]
    Analysis(#[from] analysis::runner::AnalysisError),
}
