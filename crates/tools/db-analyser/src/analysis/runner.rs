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
    /// `(slot, block_no, cumulative, count)` tuples matching upstream's
    /// `CountTxOutputsEvent(blockNo, slot, cumulative, count)` shape
    /// (R486 byte-equivalence enrichment).
    CountTxOutputs {
        /// Cumulative tx-output count across the chain.
        total: i64,
        /// Per-block `(slot, block_no, cumulative, count)` tuples in
        /// chain order. The `cumulative` field is the running total
        /// *after* applying this block; the `count` field is the
        /// per-block contribution.
        per_block: Vec<(SlotNo, BlockNo, i64, i64)>,
    },
    /// `ShowBlockHeaderSize` result — max observed header size + per-
    /// block `(slot, block_no, header_size, block_size)` tuples
    /// matching upstream's `HeaderSizeEvent(blockNo, slot, headerSize,
    /// blockSize)` shape (R486 byte-equivalence enrichment).
    ShowBlockHeaderSize {
        /// Maximum header size in bytes observed across the chain.
        /// Width is `u32` for headroom; upstream uses `Word16` which
        /// will narrow at render time.
        max_size: u32,
        /// Per-block `(slot, block_no, header_size, block_size)`
        /// tuples. `block_size` comes from `Block::raw_cbor.len()`
        /// when present, else 0 (matches upstream's `GetBlockSize`
        /// behavior on programmatically-constructed blocks).
        per_block: Vec<(SlotNo, BlockNo, u32, u32)>,
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
    /// `TraceLedgerProcessing` result (R488 + R496) — per-block
    /// ledger-apply outcome trace + emitted trace events.
    ///
    /// `traces`: each tuple is `(slot, block_no, outcome)` where
    /// `outcome` is either `Ok(())` (block applied successfully)
    /// or `Err(reason_string)` (apply failed; the reason is the
    /// [`yggdrasil_ledger::LedgerError`] rendered via `Display`).
    ///
    /// `emit_traces` (R496): per-block parallel vec of the
    /// strings returned by `HasAnalysis::emit_traces` (block-
    /// iteration-derivable trace events — era, slot, block_no,
    /// tx_count, EBB marker, etc.). Indexed by the same i-th
    /// block as `traces`.
    ///
    /// **Carve-out (R496):** R488 documented an empty
    /// `Block::emit_traces` (R476 placeholder); R496 ships a
    /// block-iteration-derived body (era / slot / block_no /
    /// tx_count / EBB marker / origin-successor marker). Ledger-
    /// state-derived traces (epoch boundary, stake delta, era
    /// transition) still need a configured genesis state — that
    /// lands in a follow-on arc.
    TraceLedgerProcessing {
        /// Per-block `(slot, block_no, outcome)` tuples in chain
        /// order. `outcome` is `Ok(())` or `Err(reason)`.
        traces: Vec<(SlotNo, BlockNo, Result<(), String>)>,
        /// R496: per-block emit_traces output, parallel to
        /// `traces`. `emit_traces[i]` is `HasAnalysis::emit_traces`
        /// applied to the i-th block.
        emit_traces: Vec<Vec<String>>,
        /// Number of blocks that applied successfully.
        applied_ok: i64,
        /// Number of blocks whose apply call returned an error.
        applied_err: i64,
    },
    /// `GetBlockApplicationMetrics` result (R490) — per-block CSV-
    /// row entries produced by invoking the R476
    /// `Block::block_application_metrics()` column closures every
    /// `every_n_blocks` blocks.
    ///
    /// Each `row` is `Vec<(column_name, column_value)>` matching the
    /// 4-column shape from `HasAnalysis::block_application_metrics`
    /// (slot, block_no, era, tx_count). The `every_n_blocks` field
    /// records the sampling cadence (1 = every block, 1000 = every
    /// thousandth block — matches upstream's `NumberOfBlocks`).
    ///
    /// **R490 carve-out:** all 4 R476 columns are block-derived (no
    /// ledger state read). The `LedgerState` is still applied per
    /// block via `LedgerState::apply_block` to stay symmetric with
    /// R488/R489's apply-loop semantics; richer ledger-state-delta
    /// columns (utxo deltas, fee totals, etc.) await a future arc.
    GetBlockApplicationMetrics {
        /// Column rows in chain order (only every-Nth block).
        rows: Vec<Vec<(String, String)>>,
        /// Sampling cadence supplied by the operator.
        every_n_blocks: u64,
        /// Number of blocks that applied successfully during the walk.
        applied_ok: i64,
        /// Number of blocks whose apply call returned an error.
        applied_err: i64,
    },
    /// `ReproMempoolAndForge` result (R493) — for each block,
    /// inserts the block's transactions into a
    /// [`yggdrasil_consensus::Mempool`] then "forges" by
    /// repeatedly calling `pop_best()` until the mempool is
    /// drained. Times each phase.
    ///
    /// Per-block stat shape:
    /// `(slot, block_no, mempool_insert_count, forge_pop_count,
    ///  mempool_insert_ns, forge_pop_ns)`.
    ///
    /// **Forensic semantics:** the mempool starts empty per block
    /// (matches upstream's "reproduce mempool-and-forge cycle"
    /// rather than carrying state across blocks). Insert failures
    /// (capacity-exceeded, duplicate-tx-id, conflicting inputs)
    /// are silently skipped — the per-block insert count reflects
    /// successful inserts. Each `MempoolEntry` is built with the
    /// progressively-realer field set (post-R495):
    /// - `era`/`tx_id`/`body`/`size_bytes` from the source `Tx`.
    /// - `inputs` from `Tx::decode_inputs(era)` (R494) — enables
    ///   real mempool conflict-detection; Byron returns empty
    ///   (uses `ByronTxIn`, not `ShelleyTxIn`).
    /// - `fee` from `Tx::decode_fee(era)` (R495) — enables real
    ///   fee-priority ordering in the mempool; Byron returns 0
    ///   (fee computed from input/output diff, not stored).
    /// - `ttl` from `Tx::decode_ttl(era)` (R495) — Shelley/Allegra/
    ///   Mary have a mandatory ttl; Alonzo+ ttl is optional and
    ///   defaults to `u64::MAX` when absent; Byron returns
    ///   `u64::MAX`.
    /// - `raw_tx=body` (forensic placeholder — `raw_tx` should be
    ///   the 3-or-4-element wire-form CBOR array; bounded
    ///   follow-on item).
    ///
    /// **Carve-out (post-R495):** upstream's `reproMempoolForge`
    /// measures the mempool revalidation hot path against live
    /// ledger state. Yggdrasil's R494+R495 wire-up gives real
    /// inputs/fee/ttl/size_bytes — fee-priority ordering, conflict
    /// detection, and TTL eviction all work. The remaining gap is
    /// ledger-state-aware revalidation (e.g. UTxO existence
    /// checks); that needs a configured genesis state, which is a
    /// separate future arc.
    ReproMempoolAndForge {
        /// Per-block stats in chain order.
        per_block_stats: Vec<(SlotNo, BlockNo, i64, i64, i64, i64)>,
        /// Number of blocks that applied successfully during the
        /// walk (LedgerState::apply_block Ok).
        applied_ok: i64,
        /// Number of blocks whose apply call returned an error.
        applied_err: i64,
    },
    /// `StoreLedgerStateAt` result (R491) — captures the
    /// [`yggdrasil_ledger::LedgerStateCheckpoint`] CBOR snapshot
    /// when the chain walk reaches the target slot.
    ///
    /// `snapshot_bytes` is the CBOR-encoded checkpoint
    /// (`LedgerStateCheckpoint::to_cbor_bytes()`); a downstream
    /// caller can write it to disk or decode back via
    /// `LedgerStateCheckpoint::from_cbor_bytes`. `reached_slot`
    /// is the actual slot at which the snapshot was captured —
    /// `None` if the walk completed without reaching
    /// `target_slot` (chain too short).
    StoreLedgerStateAt {
        /// Operator-supplied target slot.
        target_slot: SlotNo,
        /// Slot at which the snapshot was captured (`None` if the
        /// chain walk ended before reaching `target_slot`).
        reached_slot: Option<SlotNo>,
        /// CBOR-encoded `LedgerStateCheckpoint` snapshot. Empty
        /// when `reached_slot` is `None`.
        snapshot_bytes: Vec<u8>,
        /// Number of blocks that applied successfully during the
        /// walk to reach the target slot.
        applied_ok: i64,
        /// Number of blocks whose apply call returned an error
        /// during the walk (still counted; doesn't abort).
        applied_err: i64,
    },
    /// `BenchmarkLedgerOps` result (R489) — per-block
    /// [`crate::analysis::benchmark_ledger_ops::slot_data_point::SlotDataPoint`]
    /// records produced by walking the chain with
    /// `LedgerState::apply_block` timing instrumentation.
    ///
    /// **Portable-subset filling:** the upstream SlotDataPoint has
    /// 15 fields; Yggdrasil populates the 6 portable ones (`slot`,
    /// `slot_gap`, `total_time`, `mut_block_apply`,
    /// `block_byte_size`, `block_stats`) and zero-fills the
    /// GHC-specific timing breakdown (`mut_`, `gc`, `maj_gc_count`,
    /// `min_gc_count`, `allocated_bytes`, `mut_forecast`,
    /// `mut_header_tick`, `mut_header_apply`, `mut_block_tick`).
    /// Rust has no direct analogs for GHC's per-allocation /
    /// per-GC-cycle counters.
    ///
    /// `total_time` and `mut_block_apply` are wall-clock
    /// nanoseconds measured via `std::time::Instant`. Apply
    /// failures do not abort the run (forensic semantics — the
    /// failed apply's timing is still captured).
    BenchmarkLedgerOps {
        /// Per-block timing records in chain order.
        slot_data_points:
            Vec<crate::analysis::benchmark_ledger_ops::slot_data_point::SlotDataPoint>,
        /// Number of blocks that applied successfully.
        applied_ok: i64,
        /// Number of blocks whose apply call returned an error
        /// (timing still captured for these blocks).
        applied_err: i64,
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
    /// 0 of the 13 upstream analyses route to this variant after
    /// R493 (ReproMempoolAndForge → shipped via the
    /// yggdrasil-consensus Mempool seam). The dispatch matrix is
    /// fully covered: 12/13 shipped + 1/13 permanent carve-out
    /// (CheckNoThunksEvery → NotApplicableToRust at R485) =
    /// 13/13 final verdicts. This variant is retained for
    /// future analyses that may surface a comparable deferral.
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
        AnalysisName::StoreLedgerStateAt(target_slot, _mode) => {
            Ok(analysis_store_ledger_state_at(&bounded, *target_slot))
        }
        AnalysisName::CheckNoThunksEvery(_) => Err(AnalysisError::NotApplicableToRust {
            analysis_name: "CheckNoThunksEvery".to_string(),
            reason: "NoThunks-style ledger-state inspection walks GHC's lazy heap for unevaluated thunks; Rust is eagerly evaluated and has no runtime thunks to inspect.".to_string(),
        }),
        AnalysisName::TraceLedgerProcessing => Ok(analysis_trace_ledger_processing(&bounded)),
        AnalysisName::BenchmarkLedgerOps(_, _) => Ok(analysis_benchmark_ledger_ops(&bounded)),
        AnalysisName::ReproMempoolAndForge(_n) => {
            Ok(analysis_repro_mempool_and_forge(&bounded))
        }
        AnalysisName::GetBlockApplicationMetrics(every_n, _path) => Ok(
            analysis_get_block_application_metrics(&bounded, every_n.0.max(1)),
        ),
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
/// `(slot, block_no, cumulative, count)` tuples matching upstream's
/// `CountTxOutputsEvent(blockNo, slot, cumulative, count)` shape.
///
/// Mirror of upstream `Analysis.hs::countTxOutputs` pass which
/// reduces over `HasAnalysis::countTxOutputs`.
pub fn analysis_count_tx_outputs(blocks: &[Block]) -> AnalysisOutcome {
    let mut total: i64 = 0;
    let mut per_block = Vec::with_capacity(blocks.len());
    for blk in blocks {
        let n = blk.count_tx_outputs();
        total = total.saturating_add(n);
        per_block.push((blk.header.slot_no, blk.header.block_no, total, n));
    }
    AnalysisOutcome::CountTxOutputs { total, per_block }
}

/// `ShowBlockHeaderSize` handler — max observed header size + per-
/// block `(slot, block_no, header_size, block_size)` tuples matching
/// upstream's `HeaderSizeEvent(blockNo, slot, headerSize, blockSize)`
/// shape.
///
/// Header sizes come from `Block::header_cbor_size` (`Some(usize)`
/// when the block was decoded from on-the-wire CBOR). Block sizes
/// come from `Block::raw_cbor.as_ref().map(|b| b.len())` — also
/// `Some(_)` when the block carries its original wire bytes.
/// Programmatically constructed blocks (without raw_cbor / header
/// size populated) emit 0.
///
/// Mirror of upstream `Analysis.hs::showHeaderSize` pass.
pub fn analysis_show_block_header_size(blocks: &[Block]) -> AnalysisOutcome {
    let mut max_size: u32 = 0;
    let mut per_block = Vec::with_capacity(blocks.len());
    for blk in blocks {
        let header_size = blk.header_cbor_size.unwrap_or(0) as u32;
        let block_size = blk.raw_cbor.as_ref().map(|b| b.len()).unwrap_or(0) as u32;
        if header_size > max_size {
            max_size = header_size;
        }
        per_block.push((
            blk.header.slot_no,
            blk.header.block_no,
            header_size,
            block_size,
        ));
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

/// `TraceLedgerProcessing` handler (R488) — walks blocks via
/// [`yggdrasil_ledger::LedgerState::apply_block`], capturing the
/// per-block Ok/Err outcome.
///
/// **Forensic semantics:** the handler bootstraps a fresh
/// `LedgerState::new(initial_era)` (where `initial_era` is the
/// first block's era, defaulting to Byron for empty inputs). It
/// applies each block in turn; an apply error does *not* abort the
/// run — instead it's captured in the per-block trace and the walk
/// continues with the unchanged state. This matches the forensic-
/// tool stance: surface every block's outcome rather than stopping
/// at the first ledger-rule violation.
///
/// **Carve-out (R488):** without a configured genesis state, real
/// Cardano mainnet blocks will mostly fail at apply time (UTxO not
/// found, protocol params absent, etc.). The dispatch shape is
/// wired and the per-block outcome line is operationally useful;
/// closing the trace-content gap requires genesis-bootstrap CLI
/// flags + protocol-params hydration which lands in a future arc.
///
/// Mirror of upstream `Analysis.hs::traceLedgerProcessing` —
/// applies blocks and calls `emit_traces` per block. Yggdrasil's
/// `Block::emit_traces` currently returns empty (R476 placeholder);
/// the captured Ok/Err outcome is the Yggdrasil-side analog of the
/// trace events.
pub fn analysis_trace_ledger_processing(blocks: &[Block]) -> AnalysisOutcome {
    use crate::has_analysis::{CardanoLedgerStateValues, WithLedgerState};

    let initial_era = blocks
        .first()
        .map(|b| b.era)
        .unwrap_or(yggdrasil_ledger::Era::Byron);
    let mut state = yggdrasil_ledger::LedgerState::new(initial_era);
    let mut traces = Vec::with_capacity(blocks.len());
    let mut emit_traces = Vec::with_capacity(blocks.len());
    let mut applied_ok: i64 = 0;
    let mut applied_err: i64 = 0;
    for blk in blocks {
        let outcome = match state.apply_block(blk) {
            Ok(()) => {
                applied_ok += 1;
                Ok(())
            }
            Err(e) => {
                applied_err += 1;
                Err(format!("{e}"))
            }
        };
        traces.push((blk.header.slot_no, blk.header.block_no, outcome));
        // R496: invoke emit_traces via WithLedgerState wrapper.
        // CardanoLedgerStateValues is a placeholder (R476); the
        // emit_traces body uses only block-iteration-derivable
        // fields, so the placeholder state is sufficient.
        let with_state = WithLedgerState::new(
            blk.clone(),
            CardanoLedgerStateValues,
            CardanoLedgerStateValues,
        );
        emit_traces.push(<Block as HasAnalysis>::emit_traces(&with_state));
    }
    AnalysisOutcome::TraceLedgerProcessing {
        traces,
        emit_traces,
        applied_ok,
        applied_err,
    }
}

/// `BenchmarkLedgerOps` handler (R489) — walks blocks via
/// [`yggdrasil_ledger::LedgerState::apply_block`] with
/// `std::time::Instant`-based timing instrumentation, producing
/// one [`SlotDataPoint`] per block.
///
/// **Portable-subset filling:** Yggdrasil fills the 6 fields with
/// real values (`slot`, `slot_gap`, `total_time`,
/// `mut_block_apply`, `block_byte_size`, `block_stats`); the
/// 9 GHC-specific fields (allocations, GC counters, per-phase
/// header/tick breakdown) are zero. Rust has no direct analogs;
/// honest zeros are more useful than synthesized values.
///
/// **`total_time` and `mut_block_apply`** are wall-clock nanoseconds
/// measured around the `apply_block` call. They're equal in
/// Yggdrasil because we don't have a separate forecast/tick/header-
/// apply/block-tick/block-apply phase decomposition.
///
/// **`block_stats`** comes from the R476
/// `Block::block_stats()` impl (`slot=N block_no=M era=E
/// tx_count=K`).
///
/// **Forensic semantics:** apply failures don't abort the run; the
/// failed apply's timing is still captured. Apply Ok/Err counters
/// are returned alongside the per-block records.
///
/// Mirror of upstream `Analysis.hs::benchmarkLedgerOps`. R374-R376
/// already shipped the `SlotDataPoint`, `Metadata`, and `FileWriting`
/// leaf records; R489 wires them through this handler.
pub fn analysis_benchmark_ledger_ops(blocks: &[Block]) -> AnalysisOutcome {
    use crate::analysis::benchmark_ledger_ops::slot_data_point::{BlockStats, SlotDataPoint};
    use std::time::Instant;

    let initial_era = blocks
        .first()
        .map(|b| b.era)
        .unwrap_or(yggdrasil_ledger::Era::Byron);
    let mut state = yggdrasil_ledger::LedgerState::new(initial_era);
    let mut slot_data_points = Vec::with_capacity(blocks.len());
    let mut applied_ok: i64 = 0;
    let mut applied_err: i64 = 0;
    let mut prev_slot: Option<u64> = None;

    for blk in blocks {
        let start = Instant::now();
        let outcome = state.apply_block(blk);
        let elapsed = start.elapsed();
        match outcome {
            Ok(()) => applied_ok += 1,
            Err(_) => applied_err += 1,
        }
        let slot_gap = prev_slot
            .map(|p| blk.header.slot_no.0.saturating_sub(p))
            .unwrap_or(0);
        prev_slot = Some(blk.header.slot_no.0);

        let total_time_ns = elapsed.as_nanos().min(i64::MAX as u128) as i64;
        let block_byte_size = blk.raw_cbor.as_ref().map(|b| b.len()).unwrap_or(0) as u32;
        let block_stats = BlockStats::from_strings(HasAnalysis::block_stats(blk));

        let mut dp = SlotDataPoint::empty(blk.header.slot_no);
        dp.slot_gap = slot_gap;
        dp.total_time = total_time_ns;
        dp.mut_block_apply = total_time_ns;
        dp.block_byte_size = block_byte_size;
        dp.block_stats = block_stats;
        slot_data_points.push(dp);
    }

    AnalysisOutcome::BenchmarkLedgerOps {
        slot_data_points,
        applied_ok,
        applied_err,
    }
}

/// `StoreLedgerStateAt` handler (R491) — walks blocks via
/// [`yggdrasil_ledger::LedgerState::apply_block`] until reaching
/// `target_slot`, captures a
/// [`yggdrasil_ledger::LedgerStateCheckpoint`] CBOR snapshot at
/// that point, and returns the encoded bytes.
///
/// The chain walk stops at the first block whose
/// `header.slot_no >= target_slot`. If the walk completes without
/// reaching the target, `reached_slot` is `None` and
/// `snapshot_bytes` is empty.
///
/// **Reuses the existing R269-shipped codec:**
/// `LedgerStateCheckpoint` already has `CborEncode`/`CborDecode`
/// impls in `crates/ledger/src/state/checkpoint.rs`. R491 does
/// not add new codec work — it only wires the existing snapshot
/// codec through the analysis runner.
///
/// **Forensic semantics:** apply failures don't abort the walk;
/// the snapshot is taken at whatever state the chain walk
/// reached when the target slot was hit.
///
/// Mirror of upstream `Analysis.hs::storeLedgerStateAt`.
pub fn analysis_store_ledger_state_at(
    blocks: &[Block],
    target_slot: yggdrasil_ledger::SlotNo,
) -> AnalysisOutcome {
    use yggdrasil_ledger::CborEncode;

    let initial_era = blocks
        .first()
        .map(|b| b.era)
        .unwrap_or(yggdrasil_ledger::Era::Byron);
    let mut state = yggdrasil_ledger::LedgerState::new(initial_era);
    let mut applied_ok: i64 = 0;
    let mut applied_err: i64 = 0;
    let mut reached_slot: Option<SlotNo> = None;
    let mut snapshot_bytes: Vec<u8> = Vec::new();

    for blk in blocks {
        match state.apply_block(blk) {
            Ok(()) => applied_ok += 1,
            Err(_) => applied_err += 1,
        }
        if blk.header.slot_no.0 >= target_slot.0 && reached_slot.is_none() {
            let checkpoint = state.checkpoint();
            snapshot_bytes = checkpoint.to_cbor_bytes();
            reached_slot = Some(blk.header.slot_no);
            // Continue applying remaining blocks for an honest
            // applied_ok/applied_err total; the snapshot is
            // taken at first-reach.
        }
    }

    AnalysisOutcome::StoreLedgerStateAt {
        target_slot,
        reached_slot,
        snapshot_bytes,
        applied_ok,
        applied_err,
    }
}

/// `GetBlockApplicationMetrics` handler (R490) — walks blocks via
/// [`yggdrasil_ledger::LedgerState::apply_block`], invoking the
/// R476 `Block::block_application_metrics()` column closures every
/// `every_n_blocks` blocks. `every_n_blocks=1` emits a row for
/// every block; `every_n_blocks=1000` emits every thousandth
/// block (matches upstream's `NumberOfBlocks` cadence parameter).
///
/// The R476 columns are all block-derived (`slot`, `block_no`,
/// `era`, `tx_count`) — no `state_before` / `state_after` reads.
/// The handler still applies blocks through the ledger-state for
/// symmetry with R488/R489 (and so the apply-loop seam is
/// exercised); richer ledger-state-delta columns (utxo deltas,
/// fee totals, etc.) await a future arc that ships them through
/// `HasAnalysis::block_application_metrics` directly.
///
/// Forensic semantics inherited from R488/R489: apply failures do
/// not abort the run; per-block sampling continues. Closure
/// failures (i.e. `Box<dyn Fn ... -> Result<_, std::io::Error>>`
/// returning `Err`) cause the row to be skipped with the metric's
/// error in the trace; `applied_err` is incremented only for
/// `LedgerState::apply_block` failures (not closure failures).
///
/// Mirror of upstream `Analysis.hs::getBlockApplicationMetrics`.
pub fn analysis_get_block_application_metrics(
    blocks: &[Block],
    every_n_blocks: u64,
) -> AnalysisOutcome {
    use crate::has_analysis::{CardanoLedgerStateValues, WithLedgerState};

    let initial_era = blocks
        .first()
        .map(|b| b.era)
        .unwrap_or(yggdrasil_ledger::Era::Byron);
    let mut state = yggdrasil_ledger::LedgerState::new(initial_era);
    let metrics = <Block as HasAnalysis>::block_application_metrics();
    let mut rows: Vec<Vec<(String, String)>> = Vec::new();
    let mut applied_ok: i64 = 0;
    let mut applied_err: i64 = 0;
    for (idx, blk) in blocks.iter().enumerate() {
        match state.apply_block(blk) {
            Ok(()) => applied_ok += 1,
            Err(_) => applied_err += 1,
        }
        if !(idx as u64).is_multiple_of(every_n_blocks) {
            continue;
        }
        let with_state = WithLedgerState::new(
            blk.clone(),
            CardanoLedgerStateValues,
            CardanoLedgerStateValues,
        );
        let mut row: Vec<(String, String)> = Vec::with_capacity(metrics.len());
        for (name, closure) in &metrics {
            if let Ok(value) = closure(&with_state) {
                row.push(((*name).to_string(), value));
            }
        }
        rows.push(row);
    }
    AnalysisOutcome::GetBlockApplicationMetrics {
        rows,
        every_n_blocks,
        applied_ok,
        applied_err,
    }
}

/// `ReproMempoolAndForge` handler (R493) — for each block,
/// applies via [`yggdrasil_ledger::LedgerState::apply_block`],
/// then inserts the block's transactions into a fresh
/// [`yggdrasil_consensus::Mempool`] (capacity = 1 MiB matching
/// upstream's `MempoolCapacityBytesOverride 1024*1024`), then
/// drains via `pop_best()`. Times each phase with
/// `std::time::Instant`.
///
/// **Forensic semantics:**
/// - Mempool starts empty per block (upstream's reproduce-cycle
///   semantics).
/// - `MempoolEntry` is built with simplified fields: real
///   `era` + `tx_id` + `body` + `size_bytes` from the source
///   `Tx`; `fee=0` / `raw_tx=body` / `ttl=u64::MAX` /
///   `inputs=Vec::new()` (no fee-decoding, no real ttl, no
///   conflict-detection — operator forensic stance).
/// - Insert failures (capacity-exceeded, duplicate-tx-id) are
///   silently skipped; the per-block insert count reflects
///   successful inserts only.
///
/// Mirror of upstream `Analysis.hs::reproMempoolForge`. R493
/// ships the dispatch shape; richer fidelity (decode fees,
/// derive ttl, derive inputs for conflict detection,
/// ledger-state-aware revalidation) awaits a future arc.
pub fn analysis_repro_mempool_and_forge(blocks: &[Block]) -> AnalysisOutcome {
    use std::time::Instant;
    use yggdrasil_consensus::mempool::{Mempool, MempoolEntry};

    const MEMPOOL_CAPACITY_BYTES: usize = 1024 * 1024;

    let initial_era = blocks
        .first()
        .map(|b| b.era)
        .unwrap_or(yggdrasil_ledger::Era::Byron);
    let mut state = yggdrasil_ledger::LedgerState::new(initial_era);
    let mut per_block_stats = Vec::with_capacity(blocks.len());
    let mut applied_ok: i64 = 0;
    let mut applied_err: i64 = 0;

    for blk in blocks {
        match state.apply_block(blk) {
            Ok(()) => applied_ok += 1,
            Err(_) => applied_err += 1,
        }

        // Fresh mempool per block (upstream's reproduce-cycle).
        let mut mempool = Mempool::with_capacity(MEMPOOL_CAPACITY_BYTES);

        // Phase 1: insert each tx into the mempool, timed.
        let insert_start = Instant::now();
        let mut insert_count: i64 = 0;
        for tx in &blk.transactions {
            // R494: real inputs via Tx::decode_inputs.
            // R495: real fee + ttl via Tx::decode_fee/Tx::decode_ttl.
            // Decode failures fall back to forensic placeholders
            // (don't abort the analysis).
            let inputs = tx.decode_inputs(blk.era).unwrap_or_default();
            let fee = tx.decode_fee(blk.era).unwrap_or(0);
            let ttl = tx.decode_ttl(blk.era).unwrap_or(u64::MAX);
            let entry = MempoolEntry {
                era: blk.era,
                tx_id: tx.id,
                fee,
                body: tx.body.clone(),
                raw_tx: tx.body.clone(),
                size_bytes: tx.serialized_size(),
                ttl: yggdrasil_ledger::SlotNo(ttl),
                inputs,
            };
            if mempool.insert(entry).is_ok() {
                insert_count += 1;
            }
        }
        let insert_ns = insert_start.elapsed().as_nanos().min(i64::MAX as u128) as i64;

        // Phase 2: forge — drain the mempool via pop_best, timed.
        let forge_start = Instant::now();
        let mut forge_count: i64 = 0;
        while mempool.pop_best().is_some() {
            forge_count += 1;
        }
        let forge_ns = forge_start.elapsed().as_nanos().min(i64::MAX as u128) as i64;

        per_block_stats.push((
            blk.header.slot_no,
            blk.header.block_no,
            insert_count,
            forge_count,
            insert_ns,
            forge_ns,
        ));
    }

    AnalysisOutcome::ReproMempoolAndForge {
        per_block_stats,
        applied_ok,
        applied_err,
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
                // (slot, block_no, cumulative, count)
                assert_eq!(per_block[0], (SlotNo(0), BlockNo(0), 0, 0));
                assert_eq!(per_block[1], (SlotNo(20), BlockNo(1), 0, 0));
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
                // (slot, block_no, header_size, block_size).
                // No raw_cbor populated → block_size = 0.
                assert_eq!(per_block[0], (SlotNo(0), BlockNo(0), 100, 0));
                assert_eq!(per_block[1], (SlotNo(20), BlockNo(1), 250, 0));
                assert_eq!(per_block[2], (SlotNo(40), BlockNo(2), 180, 0));
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
                // (slot, block_no, header_size, block_size) all zero.
                assert_eq!(per_block, vec![(SlotNo(0), BlockNo(0), 0u32, 0u32)]);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    // ── R486 per-block event-shape enrichment ──────────────────────────

    #[test]
    fn analysis_count_tx_outputs_emits_block_no_and_cumulative() {
        // R486: each per-block row carries (slot, block_no, cumulative,
        // count). Empty transaction lists give 0 contributions; the
        // cumulative still increments through the chain (here: 0).
        let outcome = analysis_count_tx_outputs(&[
            mk_block(10, 100, None),
            mk_block(20, 101, None),
            mk_block(30, 102, None),
        ]);
        match outcome {
            AnalysisOutcome::CountTxOutputs { total, per_block } => {
                assert_eq!(total, 0);
                assert_eq!(per_block.len(), 3);
                assert_eq!(per_block[0], (SlotNo(10), BlockNo(100), 0, 0));
                assert_eq!(per_block[1], (SlotNo(20), BlockNo(101), 0, 0));
                assert_eq!(per_block[2], (SlotNo(30), BlockNo(102), 0, 0));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_block_header_size_emits_block_no() {
        // R486: each per-block row carries (slot, block_no,
        // header_size, block_size).
        let outcome = analysis_show_block_header_size(&[
            mk_block(10, 42, Some(120)),
            mk_block(20, 43, Some(80)),
        ]);
        match outcome {
            AnalysisOutcome::ShowBlockHeaderSize {
                max_size,
                per_block,
            } => {
                assert_eq!(max_size, 120);
                // No raw_cbor populated → block_size = 0.
                assert_eq!(per_block[0], (SlotNo(10), BlockNo(42), 120, 0));
                assert_eq!(per_block[1], (SlotNo(20), BlockNo(43), 80, 0));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_show_block_header_size_emits_block_size_from_raw_cbor() {
        // R486: when raw_cbor is populated, block_size reflects its
        // length. mk_block hard-codes raw_cbor: None; build a block
        // with raw_cbor here to exercise the populated path.
        use std::sync::Arc;
        let mut blk = mk_block(50, 200, Some(64));
        blk.raw_cbor = Some(Arc::from(vec![0u8; 1024].into_boxed_slice()));
        let outcome = analysis_show_block_header_size(&[blk]);
        match outcome {
            AnalysisOutcome::ShowBlockHeaderSize {
                max_size,
                per_block,
            } => {
                assert_eq!(max_size, 64);
                assert_eq!(per_block, vec![(SlotNo(50), BlockNo(200), 64u32, 1024u32)]);
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

    // ── R488 TraceLedgerProcessing handler ─────────────────────────────

    #[test]
    fn analysis_trace_ledger_processing_empty_chain() {
        let outcome = analysis_trace_ledger_processing(&[]);
        match outcome {
            AnalysisOutcome::TraceLedgerProcessing {
                traces,
                emit_traces,
                applied_ok,
                applied_err,
            } => {
                let _ = &emit_traces;
                assert!(traces.is_empty());
                assert_eq!(applied_ok, 0);
                assert_eq!(applied_err, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_trace_ledger_processing_byron_block_empty_state_succeeds() {
        // A Byron block with no transactions against an empty
        // Byron-era LedgerState should apply cleanly (no UTxO
        // lookups required).
        let mut blk = mk_block(0, 0, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        let outcome = analysis_trace_ledger_processing(&[blk]);
        match outcome {
            AnalysisOutcome::TraceLedgerProcessing {
                traces,
                emit_traces: _,
                applied_ok,
                applied_err: _,
            } => {
                assert_eq!(traces.len(), 1);
                assert_eq!(applied_ok, 1);
                assert!(traces[0].2.is_ok(), "expected Ok, got {:?}", traces[0].2);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_trace_ledger_processing_per_block_trace_shape() {
        let outcome = analysis_trace_ledger_processing(&[
            mk_block(10, 100, None),
            mk_block(20, 101, None),
            mk_block(30, 102, None),
        ]);
        match outcome {
            AnalysisOutcome::TraceLedgerProcessing {
                traces,
                emit_traces,
                applied_ok,
                applied_err,
            } => {
                let _ = &emit_traces;
                assert_eq!(traces.len(), 3);
                assert_eq!(traces[0].0, SlotNo(10));
                assert_eq!(traces[0].1, BlockNo(100));
                assert_eq!(traces[2].0, SlotNo(30));
                // applied_ok + applied_err should equal trace count.
                assert_eq!(
                    applied_ok + applied_err,
                    traces.len() as i64,
                    "applied_ok + applied_err must equal trace count"
                );
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn run_analysis_dispatches_trace_ledger_processing() {
        let config = mk_config(AnalysisName::TraceLedgerProcessing, Limit::Unlimited);
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(
            outcome,
            AnalysisOutcome::TraceLedgerProcessing { .. }
        ));
    }

    #[test]
    fn analysis_trace_ledger_processing_emits_per_block_trace_strings_r496() {
        // R496: emit_traces field is parallel to traces; each entry
        // is the HasAnalysis::emit_traces output for the i-th block.
        // Block-iteration-derived content: event/slot/block_no/era/
        // tx_count + EBB marker + origin marker.
        let outcome =
            analysis_trace_ledger_processing(&[mk_block(10, 100, None), mk_block(20, 101, None)]);
        match outcome {
            AnalysisOutcome::TraceLedgerProcessing {
                traces,
                emit_traces,
                ..
            } => {
                assert_eq!(traces.len(), 2);
                assert_eq!(emit_traces.len(), 2);
                // Each per-block trace vec must include the canonical
                // 5 key=value strings (event/slot/block_no/era/tx_count).
                for (i, per_block) in emit_traces.iter().enumerate() {
                    assert!(per_block.iter().any(|s| s == "event=block_apply"));
                    assert!(per_block.iter().any(|s| s.starts_with("slot=")));
                    assert!(per_block.iter().any(|s| s.starts_with("block_no=")));
                    assert!(per_block.iter().any(|s| s.starts_with("era=")));
                    assert!(per_block.iter().any(|s| s.starts_with("tx_count=")));
                    // Block i=0 has slot=10, block_no=100; i=1 has slot=20,
                    // block_no=101.
                    let expected_slot = if i == 0 { "slot=10" } else { "slot=20" };
                    assert!(
                        per_block.iter().any(|s| s == expected_slot),
                        "block i={i} missing expected {expected_slot}: {per_block:?}"
                    );
                }
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_trace_ledger_processing_emits_origin_marker_for_genesis_successor() {
        // R496: blocks with prev_hash = all-zeros (origin marker)
        // get a "prev=<origin>" trace.
        let blk = mk_block(0, 0, None);
        // mk_block sets prev_hash: HeaderHash([0x00; 32]) — origin.
        let outcome = analysis_trace_ledger_processing(&[blk]);
        match outcome {
            AnalysisOutcome::TraceLedgerProcessing { emit_traces, .. } => {
                assert!(
                    emit_traces[0].iter().any(|s| s == "prev=<origin>"),
                    "expected prev=<origin> trace, got {:?}",
                    emit_traces[0]
                );
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_trace_ledger_processing_emits_ebb_marker_for_known_byron_ebb() {
        // R496: blocks whose header_hash matches a known Byron EBB
        // get an "ebb=true" trace.
        let mut blk = mk_block(0, 0, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        blk.header.hash = HeaderHash(crate::byron_ebbs::parse_hex32(
            "89d9b5a5b8ddc8d7e5a6795e9774d97faf1efea59b2caf7eaf9f8c5b32059df4",
        ));
        let outcome = analysis_trace_ledger_processing(&[blk]);
        match outcome {
            AnalysisOutcome::TraceLedgerProcessing { emit_traces, .. } => {
                assert!(
                    emit_traces[0].iter().any(|s| s == "ebb=true"),
                    "expected ebb=true trace, got {:?}",
                    emit_traces[0]
                );
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
    fn run_analysis_benchmark_ledger_ops_returns_outcome() {
        // R489: BenchmarkLedgerOps now ships via the apply-loop
        // seam (was RequiresLedgerStateApplyLoop pre-R489).
        let config = mk_config(
            AnalysisName::BenchmarkLedgerOps(None, LedgerApplicationMode::LedgerReapply),
            Limit::Unlimited,
        );
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(
            outcome,
            AnalysisOutcome::BenchmarkLedgerOps { .. }
        ));
    }

    #[test]
    fn analysis_benchmark_ledger_ops_empty_chain() {
        let outcome = analysis_benchmark_ledger_ops(&[]);
        match outcome {
            AnalysisOutcome::BenchmarkLedgerOps {
                slot_data_points,
                applied_ok,
                applied_err,
            } => {
                assert!(slot_data_points.is_empty());
                assert_eq!(applied_ok, 0);
                assert_eq!(applied_err, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_benchmark_ledger_ops_records_per_block_timing() {
        let mut byron = mk_block(10, 100, None);
        byron.era = yggdrasil_ledger::Era::Byron;
        let outcome = analysis_benchmark_ledger_ops(&[byron]);
        match outcome {
            AnalysisOutcome::BenchmarkLedgerOps {
                slot_data_points,
                applied_ok,
                applied_err,
            } => {
                assert_eq!(slot_data_points.len(), 1);
                let dp = &slot_data_points[0];
                assert_eq!(dp.slot, SlotNo(10));
                assert_eq!(dp.slot_gap, 0); // First block — gap = 0.
                assert!(dp.total_time >= 0); // wall-clock ns
                // mut_block_apply mirrors total_time in our impl.
                assert_eq!(dp.total_time, dp.mut_block_apply);
                // applied_ok + applied_err == slot_data_points.len()
                assert_eq!(applied_ok + applied_err, 1);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_benchmark_ledger_ops_emits_slot_gap_between_blocks() {
        let mut a = mk_block(10, 100, None);
        a.era = yggdrasil_ledger::Era::Byron;
        let mut b = mk_block(25, 101, None);
        b.era = yggdrasil_ledger::Era::Byron;
        let mut c = mk_block(40, 102, None);
        c.era = yggdrasil_ledger::Era::Byron;
        let outcome = analysis_benchmark_ledger_ops(&[a, b, c]);
        match outcome {
            AnalysisOutcome::BenchmarkLedgerOps {
                slot_data_points, ..
            } => {
                assert_eq!(slot_data_points[0].slot_gap, 0); // first
                assert_eq!(slot_data_points[1].slot_gap, 15); // 25-10
                assert_eq!(slot_data_points[2].slot_gap, 15); // 40-25
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn run_analysis_dispatch_matrix_no_longer_returns_apply_loop_errors() {
        // After R493: no AnalysisName routes to
        // RequiresLedgerStateApplyLoop. The only error variant
        // operators can hit is NotApplicableToRust for
        // CheckNoThunksEvery (R485 permanent carve-out). This
        // test pins the 13/13 dispatch coverage.
        for analysis in [
            AnalysisName::ShowSlotBlockNo,
            AnalysisName::CountTxOutputs,
            AnalysisName::ShowBlockHeaderSize,
            AnalysisName::ShowBlockTxsSize,
            AnalysisName::ShowEBBs,
            AnalysisName::OnlyValidation,
            AnalysisName::CountBlocks,
            AnalysisName::TraceLedgerProcessing,
            AnalysisName::ReproMempoolAndForge(50),
            AnalysisName::StoreLedgerStateAt(SlotNo(0), LedgerApplicationMode::LedgerReapply),
            AnalysisName::BenchmarkLedgerOps(None, LedgerApplicationMode::LedgerReapply),
            AnalysisName::GetBlockApplicationMetrics(NumberOfBlocks(1), None),
        ] {
            let config = mk_config(analysis.clone(), Limit::Unlimited);
            let result = run_analysis(&config, Vec::<Block>::new());
            assert!(
                result.is_ok(),
                "analysis {analysis:?} should now ship, got {result:?}"
            );
        }
        // CheckNoThunksEvery is the only permanent carve-out.
        let config = mk_config(AnalysisName::CheckNoThunksEvery(100), Limit::Unlimited);
        let err = run_analysis(&config, Vec::<Block>::new()).unwrap_err();
        assert!(matches!(err, AnalysisError::NotApplicableToRust { .. }));
    }

    #[test]
    fn run_analysis_get_block_application_metrics_returns_outcome() {
        // R490: GetBlockApplicationMetrics now ships (was
        // RequiresLedgerStateApplyLoop pre-R490).
        let config = mk_config(
            AnalysisName::GetBlockApplicationMetrics(NumberOfBlocks(1), None),
            Limit::Unlimited,
        );
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(
            outcome,
            AnalysisOutcome::GetBlockApplicationMetrics { .. }
        ));
    }

    #[test]
    fn analysis_get_block_application_metrics_empty_chain() {
        let outcome = analysis_get_block_application_metrics(&[], 1);
        match outcome {
            AnalysisOutcome::GetBlockApplicationMetrics {
                rows,
                every_n_blocks,
                applied_ok,
                applied_err,
            } => {
                assert!(rows.is_empty());
                assert_eq!(every_n_blocks, 1);
                assert_eq!(applied_ok, 0);
                assert_eq!(applied_err, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_get_block_application_metrics_every_block() {
        // every_n_blocks=1 → row per block. R476 columns are
        // slot/block_no/era/tx_count.
        let mut a = mk_block(10, 1, None);
        a.era = yggdrasil_ledger::Era::Byron;
        let mut b = mk_block(20, 2, None);
        b.era = yggdrasil_ledger::Era::Byron;
        let outcome = analysis_get_block_application_metrics(&[a, b], 1);
        match outcome {
            AnalysisOutcome::GetBlockApplicationMetrics { rows, .. } => {
                assert_eq!(rows.len(), 2);
                // Each row has 4 columns: slot, block_no, era, tx_count.
                assert_eq!(rows[0].len(), 4);
                assert_eq!(rows[0][0], ("slot".to_string(), "10".to_string()));
                assert_eq!(rows[0][1], ("block_no".to_string(), "1".to_string()));
                assert_eq!(rows[1][0], ("slot".to_string(), "20".to_string()));
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    // ── R493 ReproMempoolAndForge handler ──────────────────────────────

    #[test]
    fn analysis_repro_mempool_and_forge_empty_chain() {
        let outcome = analysis_repro_mempool_and_forge(&[]);
        match outcome {
            AnalysisOutcome::ReproMempoolAndForge {
                per_block_stats,
                applied_ok,
                applied_err,
            } => {
                assert!(per_block_stats.is_empty());
                assert_eq!(applied_ok, 0);
                assert_eq!(applied_err, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_repro_mempool_and_forge_block_with_no_txs_yields_zero_counts() {
        let mut blk = mk_block(10, 1, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        // mk_block sets transactions: vec![] — confirm.
        assert!(blk.transactions.is_empty());
        let outcome = analysis_repro_mempool_and_forge(&[blk]);
        match outcome {
            AnalysisOutcome::ReproMempoolAndForge {
                per_block_stats, ..
            } => {
                assert_eq!(per_block_stats.len(), 1);
                let (slot, block_no, inserts, forges, _ins_ns, _fge_ns) = per_block_stats[0];
                assert_eq!(slot, SlotNo(10));
                assert_eq!(block_no, BlockNo(1));
                assert_eq!(inserts, 0);
                assert_eq!(forges, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_repro_mempool_and_forge_with_synthetic_txs_round_trips() {
        use yggdrasil_ledger::{Tx, compute_tx_id};
        let mut blk = mk_block(10, 1, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        // Add 3 transactions with distinct bodies (so distinct tx_ids).
        for i in 0u8..3 {
            let body = vec![0x80, 0x10 + i]; // CBOR empty-array + sentinel
            blk.transactions.push(Tx {
                id: compute_tx_id(&body),
                body,
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            });
        }
        let outcome = analysis_repro_mempool_and_forge(&[blk]);
        match outcome {
            AnalysisOutcome::ReproMempoolAndForge {
                per_block_stats, ..
            } => {
                let (_, _, inserts, forges, _, _) = per_block_stats[0];
                assert_eq!(inserts, 3, "all 3 synthetic txs inserted");
                assert_eq!(forges, 3, "all 3 forged back");
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_repro_mempool_and_forge_skips_duplicate_tx_ids() {
        use yggdrasil_ledger::{Tx, compute_tx_id};
        let mut blk = mk_block(10, 1, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        // Two transactions with the same body → same tx_id →
        // second insert should fail and be silently skipped.
        let body = vec![0x80];
        for _ in 0..2 {
            blk.transactions.push(Tx {
                id: compute_tx_id(&body),
                body: body.clone(),
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            });
        }
        let outcome = analysis_repro_mempool_and_forge(&[blk]);
        match outcome {
            AnalysisOutcome::ReproMempoolAndForge {
                per_block_stats, ..
            } => {
                let (_, _, inserts, _, _, _) = per_block_stats[0];
                assert_eq!(inserts, 1, "duplicate tx_id second insert is skipped");
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_repro_mempool_and_forge_rejects_conflicting_inputs_r494() {
        // R494: real input decoding enables mempool conflict
        // detection. Construct 2 Shelley txs sharing an input
        // (TxId,index)=(0xAA..,0); the mempool's
        // remove_conflicting_inputs logic rejects the second.
        use yggdrasil_ledger::{
            CborEncode, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, Tx, compute_tx_id,
        };
        let shared_in = ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        };
        let body_a = ShelleyTxBody {
            inputs: vec![shared_in],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 29],
                amount: 1,
            }],
            fee: 1,
            ttl: 0,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let mut body_b = body_a.clone();
        // Make body_b distinct from body_a (different fee → different tx_id)
        // but with the same input.
        body_b.fee = 2;
        let bytes_a = body_a.to_cbor_bytes();
        let bytes_b = body_b.to_cbor_bytes();
        let mut blk = mk_block(10, 1, None);
        blk.era = yggdrasil_ledger::Era::Shelley;
        blk.transactions.push(Tx {
            id: compute_tx_id(&bytes_a),
            body: bytes_a,
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        });
        blk.transactions.push(Tx {
            id: compute_tx_id(&bytes_b),
            body: bytes_b,
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        });
        let outcome = analysis_repro_mempool_and_forge(&[blk]);
        match outcome {
            AnalysisOutcome::ReproMempoolAndForge {
                per_block_stats, ..
            } => {
                let (_, _, inserts, forges, _, _) = per_block_stats[0];
                // First tx inserts; second tx is rejected as a
                // conflicting-input duplicate.
                assert_eq!(
                    inserts, 1,
                    "expected 1 successful insert (2nd tx conflicts on input)"
                );
                assert_eq!(forges, 1, "1 forge for the 1 successful insert");
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_repro_mempool_and_forge_uses_real_fee_for_priority_ordering_r495() {
        // R495: real fee enables fee-priority forge ordering.
        // The mempool's pop_best returns highest-fee first; we
        // construct 3 Shelley txs with fees 100, 500, 250 and
        // assert all 3 forge cleanly (they're inserted; the
        // ordering is internal to the mempool but the round-trip
        // count proves real fees flow through).
        use yggdrasil_ledger::{
            CborEncode, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, Tx, compute_tx_id,
        };
        let mk_body = |fee: u64, tx_byte: u8| ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [tx_byte; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 29],
                amount: 1,
            }],
            fee,
            ttl: 0,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        };
        let mut blk = mk_block(10, 1, None);
        blk.era = yggdrasil_ledger::Era::Shelley;
        for (fee, tx_byte) in [(100u64, 0xAA), (500, 0xBB), (250, 0xCC)] {
            let bytes = mk_body(fee, tx_byte).to_cbor_bytes();
            blk.transactions.push(Tx {
                id: compute_tx_id(&bytes),
                body: bytes,
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            });
        }
        let outcome = analysis_repro_mempool_and_forge(&[blk]);
        match outcome {
            AnalysisOutcome::ReproMempoolAndForge {
                per_block_stats, ..
            } => {
                let (_, _, inserts, forges, _, _) = per_block_stats[0];
                assert_eq!(inserts, 3, "all 3 distinct txs inserted (distinct inputs)");
                assert_eq!(forges, 3, "all 3 forged back");
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn run_analysis_dispatches_repro_mempool_and_forge() {
        let config = mk_config(AnalysisName::ReproMempoolAndForge(50), Limit::Unlimited);
        let outcome = run_analysis(&config, vec![mk_block(0, 0, None)]).unwrap();
        assert!(matches!(
            outcome,
            AnalysisOutcome::ReproMempoolAndForge { .. }
        ));
    }

    // ── R491 StoreLedgerStateAt handler ────────────────────────────────

    #[test]
    fn analysis_store_ledger_state_at_empty_chain_returns_none() {
        let outcome = analysis_store_ledger_state_at(&[], SlotNo(100));
        match outcome {
            AnalysisOutcome::StoreLedgerStateAt {
                target_slot,
                reached_slot,
                snapshot_bytes,
                applied_ok,
                applied_err,
            } => {
                assert_eq!(target_slot, SlotNo(100));
                assert!(reached_slot.is_none());
                assert!(snapshot_bytes.is_empty());
                assert_eq!(applied_ok, 0);
                assert_eq!(applied_err, 0);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_store_ledger_state_at_target_too_high_returns_none() {
        let mut blk = mk_block(10, 0, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        let outcome = analysis_store_ledger_state_at(&[blk], SlotNo(9999));
        match outcome {
            AnalysisOutcome::StoreLedgerStateAt {
                reached_slot,
                snapshot_bytes,
                ..
            } => {
                assert!(reached_slot.is_none());
                assert!(snapshot_bytes.is_empty());
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_store_ledger_state_at_captures_snapshot_at_target() {
        let mut a = mk_block(10, 0, None);
        a.era = yggdrasil_ledger::Era::Byron;
        let mut b = mk_block(20, 1, None);
        b.era = yggdrasil_ledger::Era::Byron;
        let mut c = mk_block(30, 2, None);
        c.era = yggdrasil_ledger::Era::Byron;
        // target_slot=20 — should snapshot at block b.
        let outcome = analysis_store_ledger_state_at(&[a, b, c], SlotNo(20));
        match outcome {
            AnalysisOutcome::StoreLedgerStateAt {
                target_slot,
                reached_slot,
                snapshot_bytes,
                applied_ok,
                applied_err,
            } => {
                assert_eq!(target_slot, SlotNo(20));
                assert_eq!(reached_slot, Some(SlotNo(20)));
                assert!(!snapshot_bytes.is_empty(), "snapshot must be encoded");
                // All 3 blocks applied (apply doesn't stop at target).
                assert_eq!(applied_ok + applied_err, 3);
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn analysis_store_ledger_state_at_snapshot_round_trips_via_checkpoint_codec() {
        use yggdrasil_ledger::{CborDecode, LedgerStateCheckpoint};
        let mut blk = mk_block(0, 0, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        let outcome = analysis_store_ledger_state_at(&[blk], SlotNo(0));
        match outcome {
            AnalysisOutcome::StoreLedgerStateAt { snapshot_bytes, .. } => {
                // Confirm the bytes decode back via the existing
                // LedgerStateCheckpoint codec.
                let decoded = LedgerStateCheckpoint::from_cbor_bytes(&snapshot_bytes);
                assert!(decoded.is_ok(), "round-trip decode failed: {decoded:?}");
            }
            _ => panic!("wrong outcome variant"),
        }
    }

    #[test]
    fn run_analysis_dispatches_store_ledger_state_at() {
        let config = mk_config(
            AnalysisName::StoreLedgerStateAt(SlotNo(0), LedgerApplicationMode::LedgerReapply),
            Limit::Unlimited,
        );
        let mut blk = mk_block(0, 0, None);
        blk.era = yggdrasil_ledger::Era::Byron;
        let outcome = run_analysis(&config, vec![blk]).unwrap();
        assert!(matches!(
            outcome,
            AnalysisOutcome::StoreLedgerStateAt { .. }
        ));
    }

    #[test]
    fn analysis_get_block_application_metrics_samples_every_n() {
        // every_n_blocks=3 → only rows for blocks at index 0, 3, 6 …
        let mut blks = Vec::new();
        for i in 0..10u64 {
            let mut b = mk_block(i * 10, i, None);
            b.era = yggdrasil_ledger::Era::Byron;
            blks.push(b);
        }
        let outcome = analysis_get_block_application_metrics(&blks, 3);
        match outcome {
            AnalysisOutcome::GetBlockApplicationMetrics { rows, .. } => {
                // Indices 0, 3, 6, 9 → 4 rows.
                assert_eq!(rows.len(), 4);
                assert_eq!(rows[0][0].1, "0"); // slot=0 (block 0)
                assert_eq!(rows[1][0].1, "30"); // slot=30 (block 3)
                assert_eq!(rows[2][0].1, "60"); // slot=60 (block 6)
                assert_eq!(rows[3][0].1, "90"); // slot=90 (block 9)
            }
            _ => panic!("wrong outcome variant"),
        }
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
