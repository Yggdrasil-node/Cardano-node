//! Analysis dispatch core — drives a `Block` iterator through one of
//! the 13 [`crate::types::AnalysisName`] variants.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/Analysis.hs.
//!
//! Direct port of upstream's `runAnalysis` dispatch arm. Upstream
//! is 1057 lines covering all 13 analyses; Yggdrasil's port lands
//! the 7 block-iteration-only analyses in R479 + R480 and emits a
//! structured `RequiresLedgerStateApplyLoop` error for the 6
//! analyses that require ledger-state replay (deferred to a future
//! arc per the carve-out in [`crate::status::analysis_dispatch_status`]).
//!
//! ## R479 surface (this slice)
//!
//! Ships 4 block-iteration-only handlers:
//!
//! | AnalysisName            | Yggdrasil handler             | Outcome variant                  |
//! |-------------------------|-------------------------------|----------------------------------|
//! | `ShowSlotBlockNo`       | [`analysis_show_slot_block_no`] | [`AnalysisOutcome::ShowSlotBlockNo`] |
//! | `CountBlocks`           | [`analysis_count_blocks`]        | [`AnalysisOutcome::CountBlocks`] |
//! | `CountTxOutputs`        | [`analysis_count_tx_outputs`]    | [`AnalysisOutcome::CountTxOutputs`] |
//! | `ShowBlockHeaderSize`   | [`analysis_show_block_header_size`] | [`AnalysisOutcome::ShowBlockHeaderSize`] |
//!
//! R480 ships the remaining 3 block-only handlers (`ShowBlockTxsSize`,
//! `ShowEBBs`, `OnlyValidation`) and the 6 ledger-state-dependent
//! deferrals.

use yggdrasil_ledger::{Block, BlockNo, HeaderHash, SlotNo};

use crate::has_analysis::HasAnalysis;
use crate::types::{AnalysisName, DBAnalyserConfig, Limit};

/// Per-analysis output value. Upstream emits text to stdout / a CSV
/// file directly; Yggdrasil returns a structured result so the
/// dispatch is testable in isolation and the CLI wrapper at
/// [`crate::run`] can format / render as needed (or feed directly
/// to the upstream-compatible stdout shape at R481).
///
/// One variant per shipped handler. Lifetimes flow from the input
/// `Block` iterator — outcomes are owned, no borrowed references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisOutcome {
    /// `ShowSlotBlockNo` result — one `(slot, block_no, header_hash)`
    /// tuple per block.
    ShowSlotBlockNo {
        /// Per-block tuples in chain order.
        lines: Vec<(SlotNo, BlockNo, HeaderHash)>,
    },
    /// `CountBlocks` result — total block count and the first/last
    /// `(slot, block_no)` observed.
    CountBlocks {
        /// Total block count processed.
        total: i64,
        /// First block in the iterator (None if empty).
        first: Option<(SlotNo, BlockNo)>,
        /// Last block in the iterator (None if empty).
        last: Option<(SlotNo, BlockNo)>,
    },
    /// `CountTxOutputs` result — cumulative output count + per-block
    /// `(slot, output_count)` tuples.
    CountTxOutputs {
        /// Cumulative tx-output count across the chain.
        total: i64,
        /// Per-block `(slot, output_count)` tuples in chain order.
        per_block: Vec<(SlotNo, i64)>,
    },
    /// `ShowBlockHeaderSize` result — max observed header size + per-
    /// block `(slot, header_size_bytes)` tuples.
    ShowBlockHeaderSize {
        /// Maximum header size in bytes observed across the chain.
        /// Width is `u32` for headroom; upstream uses `Word16` which
        /// will narrow at render time.
        max_size: u32,
        /// Per-block `(slot, header_size_bytes)` tuples.
        per_block: Vec<(SlotNo, u32)>,
    },
}

/// Errors from the analysis dispatch core.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AnalysisError {
    /// The selected analysis requires ledger-state apply-loop
    /// support that has not yet shipped in Yggdrasil. Mirror of the
    /// upstream `Analysis.hs` pattern where these analyses thread
    /// `LedgerState (CardanoBlock c) ValuesMK` through the per-block
    /// step.
    #[error(
        "yggdrasil-db-analyser: analysis '{analysis_name}' requires a ledger-state apply-loop \
         which is not yet shipped (R475-R481 lands block-iteration-only analyses; the apply-loop \
         arc is a separate future commitment — see status::analysis_dispatch_status)."
    )]
    RequiresLedgerStateApplyLoop {
        /// Name of the analysis that hit the deferral (e.g. `"BenchmarkLedgerOps"`).
        analysis_name: String,
    },
    /// The selected analysis is documented as a block-iteration-only
    /// analysis but has not yet shipped its handler in the R475-R481
    /// arc. Used for the three handlers that R480 ships
    /// (`ShowBlockTxsSize`, `ShowEBBs`, `OnlyValidation`) — R479
    /// returns this variant for them so the runner is wired
    /// end-to-end at R479 even before R480 lands the bodies.
    #[error(
        "yggdrasil-db-analyser: analysis '{analysis_name}' is block-iteration-only but its \
         handler is not yet shipped (lands at R480 per the R475-R481 arc plan)."
    )]
    BlockOnlyHandlerPendingR480 {
        /// Name of the analysis whose handler is pending.
        analysis_name: String,
    },
}

/// Apply the [`Limit`] from the config to a block iterator.
///
/// Mirror of upstream's `take confLimit` short-circuit. Yggdrasil
/// returns a `Vec<Block>` because R479's handlers all need
/// per-block + cumulative outputs (the iterator pattern lands at
/// R480 if a streaming variant becomes needed for memory-bound
/// runs).
fn apply_limit<I: IntoIterator<Item = Block>>(blocks: I, limit: Limit) -> Vec<Block> {
    match limit {
        Limit::Unlimited => blocks.into_iter().collect(),
        Limit::Limit(n) => blocks.into_iter().take(n as usize).collect(),
    }
}

/// Run the analysis selected by `config.analysis` over the supplied
/// block iterator. Mirror of upstream
/// `runAnalysis :: AnalysisName -> AnalysisEnv blk -> IO (Maybe AnalysisResult)`.
///
/// The block iterator is typically `ImmutableStore::suffix_after(&Point::Origin)`
/// at R481 wire-up time; for unit tests it's an in-memory `Vec<Block>`.
pub fn run_analysis<I: IntoIterator<Item = Block>>(
    config: &DBAnalyserConfig,
    blocks: I,
) -> Result<AnalysisOutcome, AnalysisError> {
    let bounded = apply_limit(blocks, config.conf_limit);
    match &config.analysis {
        AnalysisName::ShowSlotBlockNo => Ok(analysis_show_slot_block_no(&bounded)),
        AnalysisName::CountBlocks => Ok(analysis_count_blocks(&bounded)),
        AnalysisName::CountTxOutputs => Ok(analysis_count_tx_outputs(&bounded)),
        AnalysisName::ShowBlockHeaderSize => Ok(analysis_show_block_header_size(&bounded)),
        AnalysisName::ShowBlockTxsSize => Err(AnalysisError::BlockOnlyHandlerPendingR480 {
            analysis_name: "ShowBlockTxsSize".to_string(),
        }),
        AnalysisName::ShowEBBs => Err(AnalysisError::BlockOnlyHandlerPendingR480 {
            analysis_name: "ShowEBBs".to_string(),
        }),
        AnalysisName::OnlyValidation => Err(AnalysisError::BlockOnlyHandlerPendingR480 {
            analysis_name: "OnlyValidation".to_string(),
        }),
        // Ledger-state-dependent analyses — return structured error
        // pending the ledger-state apply-loop arc.
        AnalysisName::StoreLedgerStateAt(_, _) => {
            Err(AnalysisError::RequiresLedgerStateApplyLoop {
                analysis_name: "StoreLedgerStateAt".to_string(),
            })
        }
        AnalysisName::CheckNoThunksEvery(_) => Err(AnalysisError::RequiresLedgerStateApplyLoop {
            analysis_name: "CheckNoThunksEvery".to_string(),
        }),
        AnalysisName::TraceLedgerProcessing => Err(AnalysisError::RequiresLedgerStateApplyLoop {
            analysis_name: "TraceLedgerProcessing".to_string(),
        }),
        AnalysisName::BenchmarkLedgerOps(_, _) => {
            Err(AnalysisError::RequiresLedgerStateApplyLoop {
                analysis_name: "BenchmarkLedgerOps".to_string(),
            })
        }
        AnalysisName::ReproMempoolAndForge(_) => Err(AnalysisError::RequiresLedgerStateApplyLoop {
            analysis_name: "ReproMempoolAndForge".to_string(),
        }),
        AnalysisName::GetBlockApplicationMetrics(_, _) => {
            Err(AnalysisError::RequiresLedgerStateApplyLoop {
                analysis_name: "GetBlockApplicationMetrics".to_string(),
            })
        }
    }
}

/// `ShowSlotBlockNo` handler — emits one `(slot, block_no, header_hash)`
/// tuple per block.
///
/// Mirror of upstream `Analysis.hs` `showSlotBlockNo` pass.
pub fn analysis_show_slot_block_no(blocks: &[Block]) -> AnalysisOutcome {
    let lines = blocks
        .iter()
        .map(|blk| (blk.header.slot_no, blk.header.block_no, blk.header.hash))
        .collect();
    AnalysisOutcome::ShowSlotBlockNo { lines }
}

/// `CountBlocks` handler — total block count + first/last positions.
///
/// Mirror of upstream `Analysis.hs` `countBlocks` pass.
pub fn analysis_count_blocks(blocks: &[Block]) -> AnalysisOutcome {
    let total = blocks.len() as i64;
    let first = blocks
        .first()
        .map(|blk| (blk.header.slot_no, blk.header.block_no));
    let last = blocks
        .last()
        .map(|blk| (blk.header.slot_no, blk.header.block_no));
    AnalysisOutcome::CountBlocks { total, first, last }
}

/// `CountTxOutputs` handler — cumulative output count + per-block
/// `(slot, output_count)` tuples.
///
/// Mirror of upstream `Analysis.hs` `countTxOutputs` pass which
/// reduces over `HasAnalysis::countTxOutputs`.
pub fn analysis_count_tx_outputs(blocks: &[Block]) -> AnalysisOutcome {
    let mut total: i64 = 0;
    let mut per_block = Vec::with_capacity(blocks.len());
    for blk in blocks {
        let n = blk.count_tx_outputs();
        total = total.saturating_add(n);
        per_block.push((blk.header.slot_no, n));
    }
    AnalysisOutcome::CountTxOutputs { total, per_block }
}

/// `ShowBlockHeaderSize` handler — max observed header size + per-
/// block `(slot, header_size_bytes)` tuples.
///
/// Block header sizes come from `Block::header_cbor_size`, which is
/// `Some(usize)` when the block was decoded from on-the-wire CBOR.
/// For programmatically constructed blocks (none in production
/// chain-walks), the size is zero.
///
/// Mirror of upstream `Analysis.hs` `showBlockHeaderSize` pass.
pub fn analysis_show_block_header_size(blocks: &[Block]) -> AnalysisOutcome {
    let mut max_size: u32 = 0;
    let mut per_block = Vec::with_capacity(blocks.len());
    for blk in blocks {
        let size = blk.header_cbor_size.unwrap_or(0) as u32;
        if size > max_size {
            max_size = size;
        }
        per_block.push((blk.header.slot_no, size));
    }
    AnalysisOutcome::ShowBlockHeaderSize {
        max_size,
        per_block,
    }
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;
    use crate::types::{
        DBAnalyserConfig, LedgerApplicationMode, LedgerDBBackend, NumberOfBlocks, SelectDB,
        ValidateBlocks,
    };
    use std::path::PathBuf;
    use yggdrasil_ledger::{Block, BlockHeader, BlockNo, Era, HeaderHash, SlotNo};

    fn mk_block(slot: u64, block_no: u64, header_size: Option<usize>) -> Block {
        Block {
            era: Era::Conway,
            header: BlockHeader {
                hash: HeaderHash([slot as u8; 32]),
                prev_hash: HeaderHash([0x00; 32]),
                slot_no: SlotNo(slot),
                block_no: BlockNo(block_no),
                issuer_vkey: [0x00; 32],
                protocol_version: None,
            },
            transactions: vec![],
            raw_cbor: None,
            header_cbor_size: header_size,
        }
    }

    fn mk_config(analysis: AnalysisName, limit: Limit) -> DBAnalyserConfig {
        DBAnalyserConfig {
            db_dir: PathBuf::from("/tmp/test-chaindb"),
            verbose: false,
            select_db: SelectDB::SelectImmutableDB(crate::types::WithOrigin::Origin),
            validation: Some(ValidateBlocks::ValidateAllBlocks),
            analysis,
            conf_limit: limit,
            ldb_backend: LedgerDBBackend::V2InMem,
        }
    }

    #[test]
    fn analysis_show_slot_block_no_empty_chain() {
        let outcome = analysis_show_slot_block_no(&[]);
        match outcome {
            AnalysisOutcome::ShowSlotBlockNo { lines } => assert!(lines.is_empty()),
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_slot_block_no_per_block_emission() {
        let blocks = vec![
            mk_block(0, 0, None),
            mk_block(20, 1, None),
            mk_block(40, 2, None),
        ];
        let outcome = analysis_show_slot_block_no(&blocks);
        match outcome {
            AnalysisOutcome::ShowSlotBlockNo { lines } => {
                assert_eq!(lines.len(), 3);
                assert_eq!(lines[0].0, SlotNo(0));
                assert_eq!(lines[0].1, BlockNo(0));
                assert_eq!(lines[1].0, SlotNo(20));
                assert_eq!(lines[2].0, SlotNo(40));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_count_blocks_empty_chain() {
        let outcome = analysis_count_blocks(&[]);
        match outcome {
            AnalysisOutcome::CountBlocks { total, first, last } => {
                assert_eq!(total, 0);
                assert!(first.is_none());
                assert!(last.is_none());
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_count_blocks_single_block() {
        let outcome = analysis_count_blocks(&[mk_block(100, 42, None)]);
        match outcome {
            AnalysisOutcome::CountBlocks { total, first, last } => {
                assert_eq!(total, 1);
                assert_eq!(first, Some((SlotNo(100), BlockNo(42))));
                assert_eq!(last, Some((SlotNo(100), BlockNo(42))));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_count_blocks_multi_block() {
        let blocks = vec![
            mk_block(0, 0, None),
            mk_block(20, 1, None),
            mk_block(40, 2, None),
            mk_block(60, 3, None),
        ];
        let outcome = analysis_count_blocks(&blocks);
        match outcome {
            AnalysisOutcome::CountBlocks { total, first, last } => {
                assert_eq!(total, 4);
                assert_eq!(first, Some((SlotNo(0), BlockNo(0))));
                assert_eq!(last, Some((SlotNo(60), BlockNo(3))));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_count_tx_outputs_empty_chain() {
        let outcome = analysis_count_tx_outputs(&[]);
        match outcome {
            AnalysisOutcome::CountTxOutputs { total, per_block } => {
                assert_eq!(total, 0);
                assert!(per_block.is_empty());
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_count_tx_outputs_empty_blocks_yields_zero() {
        // Empty transaction lists → 0 outputs.
        let outcome = analysis_count_tx_outputs(&[mk_block(0, 0, None), mk_block(20, 1, None)]);
        match outcome {
            AnalysisOutcome::CountTxOutputs { total, per_block } => {
                assert_eq!(total, 0);
                assert_eq!(per_block.len(), 2);
                assert_eq!(per_block[0], (SlotNo(0), 0));
                assert_eq!(per_block[1], (SlotNo(20), 0));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_block_header_size_empty_chain() {
        let outcome = analysis_show_block_header_size(&[]);
        match outcome {
            AnalysisOutcome::ShowBlockHeaderSize {
                max_size,
                per_block,
            } => {
                assert_eq!(max_size, 0);
                assert!(per_block.is_empty());
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_block_header_size_tracks_max() {
        let blocks = vec![
            mk_block(0, 0, Some(100)),
            mk_block(20, 1, Some(250)),
            mk_block(40, 2, Some(180)),
        ];
        let outcome = analysis_show_block_header_size(&blocks);
        match outcome {
            AnalysisOutcome::ShowBlockHeaderSize {
                max_size,
                per_block,
            } => {
                assert_eq!(max_size, 250);
                assert_eq!(per_block.len(), 3);
                assert_eq!(per_block[0], (SlotNo(0), 100));
                assert_eq!(per_block[1], (SlotNo(20), 250));
                assert_eq!(per_block[2], (SlotNo(40), 180));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_block_header_size_treats_missing_as_zero() {
        let outcome = analysis_show_block_header_size(&[mk_block(0, 0, None)]);
        match outcome {
            AnalysisOutcome::ShowBlockHeaderSize {
                max_size,
                per_block,
            } => {
                assert_eq!(max_size, 0);
                assert_eq!(per_block, vec![(SlotNo(0), 0)]);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    // ── Dispatch core ──────────────────────────────────────────────────

    #[test]
    fn run_analysis_dispatches_show_slot_block_no() {
        let config = mk_config(AnalysisName::ShowSlotBlockNo, Limit::Unlimited);
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(outcome, AnalysisOutcome::ShowSlotBlockNo { .. }));
    }

    #[test]
    fn run_analysis_dispatches_count_blocks() {
        let config = mk_config(AnalysisName::CountBlocks, Limit::Unlimited);
        let outcome = run_analysis(
            &config,
            vec![
                mk_block(0, 0, None),
                mk_block(20, 1, None),
                mk_block(40, 2, None),
            ],
        )
        .unwrap();
        match outcome {
            AnalysisOutcome::CountBlocks { total, .. } => assert_eq!(total, 3),
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn run_analysis_dispatches_count_tx_outputs() {
        let config = mk_config(AnalysisName::CountTxOutputs, Limit::Unlimited);
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(outcome, AnalysisOutcome::CountTxOutputs { .. }));
    }

    #[test]
    fn run_analysis_dispatches_show_block_header_size() {
        let config = mk_config(AnalysisName::ShowBlockHeaderSize, Limit::Unlimited);
        let outcome = run_analysis(&config, vec![mk_block(0, 0, Some(123))]).unwrap();
        match outcome {
            AnalysisOutcome::ShowBlockHeaderSize { max_size, .. } => assert_eq!(max_size, 123),
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn run_analysis_show_block_txs_size_returns_pending_r480() {
        let config = mk_config(AnalysisName::ShowBlockTxsSize, Limit::Unlimited);
        let err = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap_err();
        match err {
            AnalysisError::BlockOnlyHandlerPendingR480 { analysis_name } => {
                assert_eq!(analysis_name, "ShowBlockTxsSize");
            }
            _ => panic!("wrong error variant"),
        }
    }

    #[test]
    fn run_analysis_show_ebbs_returns_pending_r480() {
        let config = mk_config(AnalysisName::ShowEBBs, Limit::Unlimited);
        let err = run_analysis(&config, Vec::<Block>::new()).unwrap_err();
        assert!(matches!(
            err,
            AnalysisError::BlockOnlyHandlerPendingR480 { .. }
        ));
    }

    #[test]
    fn run_analysis_benchmark_ledger_ops_returns_requires_apply_loop() {
        let config = mk_config(
            AnalysisName::BenchmarkLedgerOps(None, LedgerApplicationMode::LedgerReapply),
            Limit::Unlimited,
        );
        let err = run_analysis(&config, Vec::<Block>::new()).unwrap_err();
        match err {
            AnalysisError::RequiresLedgerStateApplyLoop { analysis_name } => {
                assert_eq!(analysis_name, "BenchmarkLedgerOps");
            }
            _ => panic!("wrong error variant"),
        }
    }

    #[test]
    fn run_analysis_repro_mempool_returns_requires_apply_loop() {
        let config = mk_config(AnalysisName::ReproMempoolAndForge(50), Limit::Unlimited);
        let err = run_analysis(&config, Vec::<Block>::new()).unwrap_err();
        assert!(matches!(
            err,
            AnalysisError::RequiresLedgerStateApplyLoop { .. }
        ));
    }

    #[test]
    fn run_analysis_get_block_application_metrics_returns_requires_apply_loop() {
        let config = mk_config(
            AnalysisName::GetBlockApplicationMetrics(NumberOfBlocks(1000), None),
            Limit::Unlimited,
        );
        let err = run_analysis(&config, Vec::<Block>::new()).unwrap_err();
        assert!(matches!(
            err,
            AnalysisError::RequiresLedgerStateApplyLoop { .. }
        ));
    }

    #[test]
    fn run_analysis_respects_conf_limit() {
        let config = mk_config(AnalysisName::CountBlocks, Limit::Limit(2));
        let outcome = run_analysis(
            &config,
            vec![
                mk_block(0, 0, None),
                mk_block(20, 1, None),
                mk_block(40, 2, None),
            ],
        )
        .unwrap();
        match outcome {
            AnalysisOutcome::CountBlocks { total, .. } => assert_eq!(total, 2),
            _ => panic!("wrong outcome variant"),
        }
    }

    // Unused-import shield: HasAnalysis + HeaderHash are referenced
    // by surrounding outcomes but not directly by these tests.
    #[test]
    fn _shield_unused_imports() {
        let _ = HeaderHash([0u8; 32]);
        let _ = <Block as HasAnalysis>::block_application_metrics();
    }
}
