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
    let raw_iter = store
        .iter_after(&Point::Origin)
        .map_err(RunError::Storage)?;
    // R503: honor config.select_db. `SelectImmutableDB(Origin)`
    // walks from origin (default); `SelectImmutableDB(At(slot))`
    // skips blocks until reaching `slot` and processes from there.
    // The skip is purely a runner-side filter since the storage
    // layer doesn't accept slot-only starting points; future
    // optimization can plumb the slot through to FileImmutable
    // for streaming-from-slot but the current chain sizes don't
    // need it.
    let blocks: Box<dyn Iterator<Item = yggdrasil_ledger::Block>> = match config.select_db {
        types::SelectDB::SelectImmutableDB(types::WithOrigin::Origin) => raw_iter,
        types::SelectDB::SelectImmutableDB(types::WithOrigin::At(target_slot)) => {
            let target = target_slot.0;
            Box::new(raw_iter.skip_while(move |b| b.header.slot_no.0 < target))
        }
    };
    let outcome = analysis::runner::run_analysis(config, blocks).map_err(RunError::Analysis)?;
    render_outcome(&outcome, config.verbose)?;
    Ok(())
}

/// Render an [`analysis::runner::AnalysisOutcome`] to stdout in a
/// shape compatible with upstream's per-analysis emission.
///
/// **R502 verbose mode:** when `verbose=true` (the default;
/// matches upstream's `--verbose` flag semantic), every per-block
/// row is emitted. When `verbose=false`, per-block rows are
/// suppressed and only the aggregate / summary line is emitted —
/// matches upstream's quiet mode for batch / scripted operator
/// workflows that only need totals.
///
/// Aggregate-only variants (`ShowEBBs`, `OnlyValidation`, etc.)
/// emit their full content regardless of verbose — they don't
/// have separable per-block + summary halves.
fn render_outcome(outcome: &analysis::runner::AnalysisOutcome, verbose: bool) -> eyre::Result<()> {
    use analysis::runner::AnalysisOutcome;
    let mut out = std::io::stdout().lock();
    match outcome {
        AnalysisOutcome::ShowSlotBlockNo { lines } => {
            // ShowSlotBlockNo has no aggregate line; non-verbose
            // emits a single count summary.
            if verbose {
                for (slot, block_no, hash) in lines {
                    writeln!(
                        out,
                        "slot={} block_no={} hash={}",
                        slot.0,
                        block_no.0,
                        hex_render(&hash.0)
                    )?;
                }
            } else {
                writeln!(out, "show_slot_block_no rows={}", lines.len())?;
            }
        }
        AnalysisOutcome::CountBlocks { total, first, last } => {
            if verbose {
                for (label, position) in [("first", first), ("last", last)] {
                    if let Some((slot, block_no)) = position {
                        writeln!(out, "{label}: slot={} block_no={}", slot.0, block_no.0)?;
                    }
                }
            }
            writeln!(out, "total_blocks={total}")?;
        }
        AnalysisOutcome::CountTxOutputs { total, per_block } => {
            if verbose {
                for (slot, block_no, cumulative, count) in per_block {
                    writeln!(
                        out,
                        "slot={} block_no={} cumulative_tx_outputs={} tx_outputs={}",
                        slot.0, block_no.0, cumulative, count
                    )?;
                }
            }
            writeln!(out, "total_tx_outputs={total}")?;
        }
        AnalysisOutcome::ShowBlockHeaderSize {
            max_size,
            per_block,
        } => {
            if verbose {
                for (slot, block_no, header_size, block_size) in per_block {
                    writeln!(
                        out,
                        "slot={} block_no={} header_size={} block_size={}",
                        slot.0, block_no.0, header_size, block_size
                    )?;
                }
            }
            writeln!(out, "max_header_size={max_size}")?;
        }
        AnalysisOutcome::ShowBlockTxsSize { per_block } => {
            if verbose {
                for (slot, tx_count, total_bytes) in per_block {
                    writeln!(
                        out,
                        "slot={} tx_count={} total_bytes={}",
                        slot.0, tx_count, total_bytes
                    )?;
                }
            } else {
                writeln!(out, "show_block_txs_size rows={}", per_block.len())?;
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
        AnalysisOutcome::ReproMempoolAndForge {
            per_block_stats,
            applied_ok,
            applied_err,
        } => {
            if verbose {
                for (slot, block_no, insert_count, forge_count, insert_ns, forge_ns) in
                    per_block_stats
                {
                    writeln!(
                        out,
                        "slot={} block_no={} mempool_inserts={} forge_pops={} insert_ns={} forge_ns={}",
                        slot.0, block_no.0, insert_count, forge_count, insert_ns, forge_ns
                    )?;
                }
            }
            writeln!(
                out,
                "repro_mempool_and_forge applied_ok={applied_ok} applied_err={applied_err}"
            )?;
        }
        AnalysisOutcome::StoreLedgerStateAt {
            target_slot,
            reached_slot,
            snapshot_bytes,
            applied_ok,
            applied_err,
        } => match reached_slot {
            Some(reached) => writeln!(
                out,
                "store_ledger_state_at target_slot={} reached_slot={} snapshot_bytes={} applied_ok={} applied_err={}",
                target_slot.0,
                reached.0,
                snapshot_bytes.len(),
                applied_ok,
                applied_err
            )?,
            None => writeln!(
                out,
                "store_ledger_state_at target_slot={} reached_slot=<not_reached> snapshot_bytes=0 applied_ok={} applied_err={}",
                target_slot.0, applied_ok, applied_err
            )?,
        },
        AnalysisOutcome::GetBlockApplicationMetrics {
            rows,
            every_n_blocks,
            applied_ok,
            applied_err,
        } => {
            if verbose {
                for row in rows {
                    let mut first = true;
                    for (name, value) in row {
                        if !first {
                            write!(out, " ")?;
                        }
                        write!(out, "{name}={value}")?;
                        first = false;
                    }
                    writeln!(out)?;
                }
            }
            writeln!(
                out,
                "get_block_application_metrics every_n_blocks={every_n_blocks} applied_ok={applied_ok} applied_err={applied_err}"
            )?;
        }
        AnalysisOutcome::BenchmarkLedgerOps {
            slot_data_points,
            applied_ok,
            applied_err,
        } => {
            if verbose {
                for dp in slot_data_points {
                    writeln!(
                        out,
                        "slot={} slot_gap={} total_time_ns={} block_size={}",
                        dp.slot.0, dp.slot_gap, dp.total_time, dp.block_byte_size
                    )?;
                }
            }
            writeln!(
                out,
                "benchmark_ledger_ops applied_ok={applied_ok} applied_err={applied_err}"
            )?;
        }
        AnalysisOutcome::TraceLedgerProcessing {
            traces,
            emit_traces,
            applied_ok,
            applied_err,
        } => {
            if verbose {
                for (i, (slot, block_no, result)) in traces.iter().enumerate() {
                    match result {
                        Ok(()) => {
                            writeln!(out, "slot={} block_no={} apply=ok", slot.0, block_no.0)?
                        }
                        Err(reason) => writeln!(
                            out,
                            "slot={} block_no={} apply=err reason={}",
                            slot.0, block_no.0, reason
                        )?,
                    };
                    if let Some(per_block_traces) = emit_traces.get(i) {
                        for trace in per_block_traces {
                            writeln!(out, "  trace: {trace}")?;
                        }
                    }
                }
            }
            writeln!(
                out,
                "trace_ledger_processing applied_ok={applied_ok} applied_err={applied_err}"
            )?;
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
