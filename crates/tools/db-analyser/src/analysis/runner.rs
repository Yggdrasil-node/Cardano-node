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
    /// `ShowBlockTxsSize` result — per-block `(slot, tx_count,
    /// total_tx_size_bytes)` tuples.
    ShowBlockTxsSize {
        /// Per-block `(slot, tx_count, total_tx_size_bytes)` tuples
        /// in chain order.
        per_block: Vec<(SlotNo, i64, u64)>,
    },
    /// `ShowEBBs` result — Byron-era epoch-boundary blocks
    /// encountered along the chain. Each tuple is
    /// `(slot, header_hash, prev_hash_from_registry)`. The prev-hash
    /// comes from the Byron known-EBB registry (matches what
    /// upstream emits — registry stays authoritative).
    ShowEBBs {
        /// EBB hits in chain order.
        ebbs: Vec<(SlotNo, HeaderHash, Option<HeaderHash>)>,
    },
    /// `OnlyValidation` result — no per-block output, just the count
    /// of blocks the chain walk processed.
    ///
    /// Upstream's `OnlyValidation` emits nothing on stdout; it
    /// completes successfully when the chain walk validates. The
    /// Yggdrasil port emits the block count so callers / tests can
    /// observe that the walk traversed the expected number of
    /// blocks (the actual validation is performed by
    /// `ImmutableStore::suffix_after` at R481 wire-up time, which
    /// rejects malformed chain data with a different error path).
    OnlyValidation {
        /// Number of blocks the validating chain walk processed.
        blocks_processed: i64,
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
    ///
    /// 5 of the 13 upstream analyses route to this variant (after
    /// R485 carved out `CheckNoThunksEvery`): `StoreLedgerStateAt`,
    /// `TraceLedgerProcessing`, `BenchmarkLedgerOps`,
    /// `ReproMempoolAndForge`, `GetBlockApplicationMetrics`. The
    /// remaining 7 are block-iteration-only and ship handlers in
    /// the R475-R481 arc.
    #[error(
        "yggdrasil-db-analyser: analysis '{analysis_name}' requires a ledger-state apply-loop \
         which is not yet shipped (R475-R481 lands block-iteration-only analyses; the apply-loop \
         arc is a separate future commitment — see status::analysis_dispatch_status)."
    )]
    RequiresLedgerStateApplyLoop {
        /// Name of the analysis that hit the deferral (e.g. `"BenchmarkLedgerOps"`).
        analysis_name: String,
    },
    /// The selected analysis is fundamentally not portable to Rust.
    /// Mirror of upstream's `CheckNoThunksEvery` arm which inspects
    /// the ledger state's GHC heap representation for unevaluated
    /// thunks — a Haskell-only laziness concept that has no Rust
    /// analog (Rust is eagerly evaluated; the language has no
    /// runtime thunks to inspect).
    ///
    /// Mirror upstream `Cardano.Tools.DBAnalyser.Analysis.checkNoThunks`
    /// uses `NoThunks.unsafeNoThunks` which walks GHC's lazy heap;
    /// this is impossible to port to Rust without a Haskell runtime.
    /// R485 documents this as a permanent carve-out.
    #[error(
        "yggdrasil-db-analyser: analysis '{analysis_name}' is fundamentally not portable to Rust \
         (laziness/thunks are a Haskell-specific GHC concept; Rust is eagerly evaluated). \
         This is a permanent carve-out — see status::analysis_dispatch_status."
    )]
    NotApplicableToRust {
        /// Name of the analysis (e.g. `"CheckNoThunksEvery"`).
        analysis_name: String,
        /// Human-readable explanation of why this analysis isn't portable.
        reason: String,
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
        AnalysisName::ShowBlockTxsSize => Ok(analysis_show_block_txs_size(&bounded)),
        AnalysisName::ShowEBBs => Ok(analysis_show_ebbs(&bounded)),
        AnalysisName::OnlyValidation => Ok(analysis_only_validation(&bounded)),
        // Ledger-state-dependent analyses — return structured error
        // pending the ledger-state apply-loop arc.
        AnalysisName::StoreLedgerStateAt(_, _) => {
            Err(AnalysisError::RequiresLedgerStateApplyLoop {
                analysis_name: "StoreLedgerStateAt".to_string(),
            })
        }
        AnalysisName::CheckNoThunksEvery(_) => Err(AnalysisError::NotApplicableToRust {
            analysis_name: "CheckNoThunksEvery".to_string(),
            reason: "NoThunks-style ledger-state inspection walks GHC's lazy heap for unevaluated thunks; Rust is eagerly evaluated and has no runtime thunks to inspect.".to_string(),
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

/// `ShowBlockTxsSize` handler — per-block `(slot, tx_count,
/// total_tx_size_bytes)` tuples. Mirror of upstream `Analysis.hs`
/// `showBlockTxsSize` pass which reduces over
/// `HasAnalysis::blockTxSizes`.
pub fn analysis_show_block_txs_size(blocks: &[Block]) -> AnalysisOutcome {
    let per_block = blocks
        .iter()
        .map(|blk| {
            let sizes = blk.block_tx_sizes();
            let total: u64 = sizes.iter().sum();
            (blk.header.slot_no, sizes.len() as i64, total)
        })
        .collect();
    AnalysisOutcome::ShowBlockTxsSize { per_block }
}

/// `ShowEBBs` handler — Byron-era epoch-boundary-block markers
/// encountered along the chain. Walks each block, checks whether
/// its header-hash is in the Byron known-EBB registry, and emits a
/// `(slot, header_hash, prev_hash_from_registry)` tuple for hits.
///
/// Mirror of upstream `Analysis.hs` `showEBBs` pass which consumes
/// `HasAnalysis::knownEBBs`.
pub fn analysis_show_ebbs(blocks: &[Block]) -> AnalysisOutcome {
    let registry = <Block as HasAnalysis>::known_ebbs();
    let ebbs = blocks
        .iter()
        .filter_map(|blk| {
            registry
                .get(&blk.header.hash)
                .map(|prev| (blk.header.slot_no, blk.header.hash, *prev))
        })
        .collect();
    AnalysisOutcome::ShowEBBs { ebbs }
}

/// `OnlyValidation` handler — completes successfully when the chain
/// walk succeeds; returns the block count for observation. Upstream's
/// `OnlyValidation` emits no output on stdout but the actual
/// validation work happens in `ImmutableStore::suffix_after` at
/// R481 wire-up time. This handler is therefore a sentinel: if it
/// runs, the chain walk reached this dispatch point.
///
/// Mirror of upstream `Analysis.hs` `OnlyValidation` arm
/// (`onlyValidation` returns `Nothing`).
pub fn analysis_only_validation(blocks: &[Block]) -> AnalysisOutcome {
    AnalysisOutcome::OnlyValidation {
        blocks_processed: blocks.len() as i64,
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

    // ── R480 handlers ──────────────────────────────────────────────────

    #[test]
    fn analysis_show_block_txs_size_empty_chain() {
        let outcome = analysis_show_block_txs_size(&[]);
        match outcome {
            AnalysisOutcome::ShowBlockTxsSize { per_block } => assert!(per_block.is_empty()),
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_block_txs_size_empty_blocks_yields_zero_sizes() {
        let outcome = analysis_show_block_txs_size(&[mk_block(0, 0, None)]);
        match outcome {
            AnalysisOutcome::ShowBlockTxsSize { per_block } => {
                assert_eq!(per_block.len(), 1);
                assert_eq!(per_block[0], (SlotNo(0), 0, 0));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_ebbs_empty_chain() {
        let outcome = analysis_show_ebbs(&[]);
        match outcome {
            AnalysisOutcome::ShowEBBs { ebbs } => assert!(ebbs.is_empty()),
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_ebbs_no_match_emits_empty() {
        // Synthetic block hashes don't match real Byron EBBs.
        let outcome = analysis_show_ebbs(&[mk_block(0, 0, None), mk_block(20, 1, None)]);
        match outcome {
            AnalysisOutcome::ShowEBBs { ebbs } => assert!(ebbs.is_empty()),
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_ebbs_matches_byron_genesis_successor() {
        // Plant a block whose header_hash is the first Mainnet
        // Byron EBB → the analysis must report it.
        let genesis_succ_hash = HeaderHash(crate::byron_ebbs::parse_hex32(
            "89d9b5a5b8ddc8d7e5a6795e9774d97faf1efea59b2caf7eaf9f8c5b32059df4",
        ));
        let mut blk = mk_block(0, 0, None);
        blk.era = Era::Byron;
        blk.header.hash = genesis_succ_hash;
        let outcome = analysis_show_ebbs(&[blk]);
        match outcome {
            AnalysisOutcome::ShowEBBs { ebbs } => {
                assert_eq!(ebbs.len(), 1);
                assert_eq!(ebbs[0].0, SlotNo(0));
                assert_eq!(ebbs[0].1, genesis_succ_hash);
                // The genesis successor has no previous hash (the
                // first Mainnet entry in EBBs.hs is `(h "...", Nothing)`).
                assert_eq!(ebbs[0].2, None);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_only_validation_empty_chain() {
        let outcome = analysis_only_validation(&[]);
        match outcome {
            AnalysisOutcome::OnlyValidation { blocks_processed } => {
                assert_eq!(blocks_processed, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_only_validation_counts_blocks() {
        let outcome = analysis_only_validation(&[
            mk_block(0, 0, None),
            mk_block(20, 1, None),
            mk_block(40, 2, None),
        ]);
        match outcome {
            AnalysisOutcome::OnlyValidation { blocks_processed } => {
                assert_eq!(blocks_processed, 3);
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
    fn run_analysis_dispatches_show_block_txs_size() {
        let config = mk_config(AnalysisName::ShowBlockTxsSize, Limit::Unlimited);
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(outcome, AnalysisOutcome::ShowBlockTxsSize { .. }));
    }

    #[test]
    fn run_analysis_dispatches_show_ebbs() {
        let config = mk_config(AnalysisName::ShowEBBs, Limit::Unlimited);
        let outcome = run_analysis(&config, Vec::<Block>::new()).unwrap();
        assert!(matches!(outcome, AnalysisOutcome::ShowEBBs { .. }));
    }

    #[test]
    fn run_analysis_dispatches_only_validation() {
        let config = mk_config(AnalysisName::OnlyValidation, Limit::Unlimited);
        let outcome =
            run_analysis(&config, vec![mk_block(0, 0, None), mk_block(20, 1, None)]).unwrap();
        match outcome {
            AnalysisOutcome::OnlyValidation { blocks_processed } => {
                assert_eq!(blocks_processed, 2);
            }
            _ => panic!("wrong outcome variant"),
        }
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
            _ => panic!("wrong error variant: {err:?}"),
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
    fn run_analysis_check_no_thunks_returns_not_applicable_to_rust() {
        // R485: CheckNoThunksEvery is fundamentally a Haskell-only
        // analysis (inspects GHC lazy heap thunks). Permanent
        // carve-out → NotApplicableToRust variant.
        let config = mk_config(AnalysisName::CheckNoThunksEvery(100), Limit::Unlimited);
        let err = run_analysis(&config, Vec::<Block>::new()).unwrap_err();
        match err {
            AnalysisError::NotApplicableToRust {
                analysis_name,
                reason,
            } => {
                assert_eq!(analysis_name, "CheckNoThunksEvery");
                assert!(
                    reason.contains("thunks") || reason.contains("lazy"),
                    "reason must mention thunks/laziness: {reason}"
                );
            }
            _ => panic!("wrong error variant: {err:?}"),
        }
    }

    #[test]
    fn analysis_error_not_applicable_to_rust_renders_helpful_message() {
        let err = AnalysisError::NotApplicableToRust {
            analysis_name: "CheckNoThunksEvery".to_string(),
            reason: "Rust is eagerly evaluated".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("CheckNoThunksEvery"));
        assert!(msg.contains("not portable to Rust"));
        assert!(msg.contains("permanent carve-out"));
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
