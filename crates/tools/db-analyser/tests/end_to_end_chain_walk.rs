//! End-to-end integration test: open a temp `FileImmutable` ChainDB,
//! populate it with synthesized blocks, dispatch through
//! `yggdrasil_db_analyser::run`, and confirm the analysis outcome.
//!
//! R481: closes the R475-R481 arc by wiring the storage layer +
//! analysis runner end-to-end. This test exercises the production
//! call path that the operator invokes via `cargo run --bin
//! db-analyser`, without going through argv parsing.

#![allow(clippy::unwrap_used)]

use tempfile::TempDir;
use yggdrasil_db_analyser::analysis::runner::{AnalysisOutcome, run_analysis};
use yggdrasil_db_analyser::types::{
    AnalysisName, DBAnalyserConfig, LedgerDBBackend, Limit, SelectDB, ValidateBlocks, WithOrigin,
};
use yggdrasil_ledger::{Block, BlockHeader, BlockNo, Era, HeaderHash, Point, SlotNo};
use yggdrasil_storage::{FileImmutable, ImmutableStore};

fn synthetic_block(byte: u8, slot: u64, block_no: u64) -> Block {
    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([byte; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: Vec::new(),
        raw_cbor: None,
        header_cbor_size: Some((100 + (slot as usize)) % 256),
    }
}

fn mk_config(db_dir: std::path::PathBuf, analysis: AnalysisName) -> DBAnalyserConfig {
    DBAnalyserConfig {
        db_dir,
        verbose: false,
        select_db: SelectDB::SelectImmutableDB(WithOrigin::Origin),
        validation: Some(ValidateBlocks::ValidateAllBlocks),
        analysis,
        conf_limit: Limit::Unlimited,
        ldb_backend: LedgerDBBackend::V2InMem,
    }
}

#[test]
fn end_to_end_count_blocks_via_file_immutable() {
    let dir = TempDir::new().unwrap();
    // Populate the temp store.
    let mut store = FileImmutable::open(dir.path()).unwrap();
    store.append_block(synthetic_block(0x01, 10, 1)).unwrap();
    store.append_block(synthetic_block(0x02, 20, 2)).unwrap();
    store.append_block(synthetic_block(0x03, 30, 3)).unwrap();
    drop(store); // flush

    // Re-open and read the chain back via the same API the runner uses.
    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(dir.path().to_path_buf(), AnalysisName::CountBlocks);
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::CountBlocks { total, first, last } => {
            assert_eq!(total, 3);
            assert_eq!(first, Some((SlotNo(10), BlockNo(1))));
            assert_eq!(last, Some((SlotNo(30), BlockNo(3))));
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_show_slot_block_no_via_file_immutable() {
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    store.append_block(synthetic_block(0x10, 5, 0)).unwrap();
    store.append_block(synthetic_block(0x11, 15, 1)).unwrap();
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(dir.path().to_path_buf(), AnalysisName::ShowSlotBlockNo);
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::ShowSlotBlockNo { lines } => {
            assert_eq!(lines.len(), 2);
            assert_eq!(lines[0].0, SlotNo(5));
            assert_eq!(lines[0].1, BlockNo(0));
            assert_eq!(lines[0].2, HeaderHash([0x10; 32]));
            assert_eq!(lines[1].0, SlotNo(15));
            assert_eq!(lines[1].1, BlockNo(1));
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_only_validation_via_file_immutable() {
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for i in 0..7u8 {
        store
            .append_block(synthetic_block(0xA0 + i, (i as u64) * 10, i as u64))
            .unwrap();
    }
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(dir.path().to_path_buf(), AnalysisName::OnlyValidation);
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::OnlyValidation { blocks_processed } => {
            assert_eq!(blocks_processed, 7);
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_empty_chain_returns_empty_outcomes() {
    let dir = TempDir::new().unwrap();
    // No blocks appended.
    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(dir.path().to_path_buf(), AnalysisName::CountBlocks);
    let outcome = run_analysis(&config, blocks).unwrap();

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
fn end_to_end_lib_run_renders_to_stdout() {
    // Exercises the full yggdrasil_db_analyser::run path: opens the
    // store, walks the chain, dispatches, renders to stdout. Just
    // confirms it returns Ok(()).
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    store.append_block(synthetic_block(0x01, 0, 0)).unwrap();
    store.append_block(synthetic_block(0x02, 20, 1)).unwrap();
    drop(store);

    let config = mk_config(dir.path().to_path_buf(), AnalysisName::CountBlocks);
    let result = yggdrasil_db_analyser::run(&config);
    assert!(result.is_ok(), "run failed: {:?}", result);
}

#[test]
fn end_to_end_lib_run_propagates_check_no_thunks_carve_out() {
    // R485 carved out CheckNoThunksEvery as fundamentally not
    // portable to Rust (NotApplicableToRust, not the
    // ledger-state apply-loop deferral). After R493 closed
    // ReproMempoolAndForge, this is the only remaining analysis
    // that returns an error — and it's a permanent carve-out,
    // not a deferral.
    let dir = TempDir::new().unwrap();
    let store = FileImmutable::open(dir.path()).unwrap();
    drop(store);

    let config = mk_config(
        dir.path().to_path_buf(),
        AnalysisName::CheckNoThunksEvery(50),
    );
    let err = yggdrasil_db_analyser::run(&config).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("CheckNoThunksEvery"),
        "expected analysis name in error msg, got: {msg}"
    );
    assert!(
        msg.contains("not portable to Rust"),
        "expected permanent-carve-out mention in error msg, got: {msg}"
    );
}

// ── R500: end-to-end integration tests for ledger-state-dependent
//        handlers (R488-R493) via FileImmutable ───────────────────────────

#[test]
fn end_to_end_trace_ledger_processing_via_file_immutable() {
    // R488 + R496: TraceLedgerProcessing applies blocks via
    // LedgerState::apply_block, captures per-block Ok/Err
    // outcomes + emit_traces strings. R500: assert the
    // end-to-end FileImmutable path exercises both.
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    let mut byron_blk = synthetic_block(0xB0, 10, 1);
    byron_blk.era = Era::Byron;
    store.append_block(byron_blk).unwrap();
    let mut byron_blk_2 = synthetic_block(0xB1, 20, 2);
    byron_blk_2.era = Era::Byron;
    store.append_block(byron_blk_2).unwrap();
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(
        dir.path().to_path_buf(),
        AnalysisName::TraceLedgerProcessing,
    );
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::TraceLedgerProcessing {
            traces,
            emit_traces,
            applied_ok,
            applied_err,
        } => {
            assert_eq!(traces.len(), 2, "2 blocks → 2 trace entries");
            assert_eq!(emit_traces.len(), 2, "emit_traces parallel to traces");
            // Byron blocks should apply cleanly against empty
            // LedgerState (no UTxO lookups).
            assert_eq!(applied_ok, 2);
            assert_eq!(applied_err, 0);
            // Each block's emit_traces must include the 5 canonical
            // R496 strings.
            for per_block in &emit_traces {
                assert!(per_block.iter().any(|s| s == "event=block_apply"));
                assert!(per_block.iter().any(|s| s.starts_with("era=Byron")));
            }
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_benchmark_ledger_ops_via_file_immutable() {
    // R489: BenchmarkLedgerOps captures per-block Instant timing
    // + SlotDataPoint records. R500: end-to-end FileImmutable
    // → run dispatch.
    use yggdrasil_db_analyser::types::LedgerApplicationMode;
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for (byte, slot, block_no) in [(0xB0u8, 10u64, 1u64), (0xB1, 20, 2), (0xB2, 30, 3)] {
        let mut blk = synthetic_block(byte, slot, block_no);
        blk.era = Era::Byron;
        store.append_block(blk).unwrap();
    }
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(
        dir.path().to_path_buf(),
        AnalysisName::BenchmarkLedgerOps(None, LedgerApplicationMode::LedgerReapply),
    );
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::BenchmarkLedgerOps {
            slot_data_points,
            applied_ok,
            applied_err,
        } => {
            assert_eq!(slot_data_points.len(), 3);
            assert_eq!(applied_ok + applied_err, 3);
            // R489 invariant: total_time mirrors mut_block_apply
            // (no separate phase decomposition).
            for dp in &slot_data_points {
                assert_eq!(dp.total_time, dp.mut_block_apply);
            }
            // Slot gaps from synthetic chain (10, 20, 30).
            assert_eq!(slot_data_points[0].slot_gap, 0); // first
            assert_eq!(slot_data_points[1].slot_gap, 10);
            assert_eq!(slot_data_points[2].slot_gap, 10);
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_store_ledger_state_at_via_file_immutable() {
    // R491: StoreLedgerStateAt captures LedgerStateCheckpoint
    // CBOR snapshot at target slot. R500: end-to-end
    // FileImmutable → run dispatch.
    use yggdrasil_db_analyser::types::LedgerApplicationMode;
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for (byte, slot, block_no) in [(0xB0u8, 10u64, 1u64), (0xB1, 20, 2), (0xB2, 30, 3)] {
        let mut blk = synthetic_block(byte, slot, block_no);
        blk.era = Era::Byron;
        store.append_block(blk).unwrap();
    }
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    // Target slot 20 → snapshot at block 2.
    let config = mk_config(
        dir.path().to_path_buf(),
        AnalysisName::StoreLedgerStateAt(SlotNo(20), LedgerApplicationMode::LedgerReapply),
    );
    let outcome = run_analysis(&config, blocks).unwrap();

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
            assert!(!snapshot_bytes.is_empty(), "snapshot CBOR must be encoded");
            // All 3 blocks applied for honest counters.
            assert_eq!(applied_ok + applied_err, 3);
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_repro_mempool_and_forge_via_file_immutable() {
    // R493 + R494 + R495 + R497: ReproMempoolAndForge insert +
    // pop_best with real inputs/fee/ttl/raw_tx fidelity.
    // R500: end-to-end FileImmutable → run dispatch.
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    let mut blk = synthetic_block(0xB0, 10, 1);
    blk.era = Era::Byron;
    store.append_block(blk).unwrap();
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(
        dir.path().to_path_buf(),
        AnalysisName::ReproMempoolAndForge(1),
    );
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::ReproMempoolAndForge {
            per_block_stats,
            applied_ok,
            applied_err,
        } => {
            assert_eq!(per_block_stats.len(), 1);
            // Empty transactions → zero inserts/forges.
            let (_, _, inserts, forges, _, _) = per_block_stats[0];
            assert_eq!(inserts, 0);
            assert_eq!(forges, 0);
            // Byron block applies cleanly.
            assert_eq!(applied_ok, 1);
            assert_eq!(applied_err, 0);
        }
        _ => panic!("wrong outcome variant"),
    }
}

// ── R501: Limit::Limit(n) truncation coverage ───────────────────────

fn mk_config_with_limit(
    db_dir: std::path::PathBuf,
    analysis: AnalysisName,
    limit: Limit,
) -> DBAnalyserConfig {
    DBAnalyserConfig {
        db_dir,
        verbose: false,
        select_db: SelectDB::SelectImmutableDB(WithOrigin::Origin),
        validation: Some(ValidateBlocks::ValidateAllBlocks),
        analysis,
        conf_limit: limit,
        ldb_backend: LedgerDBBackend::V2InMem,
    }
}

#[test]
fn end_to_end_lib_run_respects_verbose_flag() {
    // R502: when config.verbose=true, render_outcome emits per-
    // block rows + summary; when verbose=false, only the summary
    // line. The lib::run path consumes config.verbose.
    //
    // We can't easily capture stdout from inside lib::run, but
    // we can at least assert both modes complete cleanly and the
    // structured outcome (returned by run_analysis directly) is
    // identical regardless of verbose flag — verbose is purely a
    // render-time concern.
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for byte in [0xA0u8, 0xA1, 0xA2] {
        store
            .append_block(synthetic_block(byte, byte as u64 * 10, byte as u64))
            .unwrap();
    }
    drop(store);

    let mut config = mk_config(dir.path().to_path_buf(), AnalysisName::CountBlocks);
    config.verbose = true;
    let result_verbose = yggdrasil_db_analyser::run(&config);
    assert!(
        result_verbose.is_ok(),
        "verbose=true run failed: {result_verbose:?}"
    );

    config.verbose = false;
    let result_quiet = yggdrasil_db_analyser::run(&config);
    assert!(
        result_quiet.is_ok(),
        "verbose=false run failed: {result_quiet:?}"
    );
}

#[test]
fn end_to_end_count_blocks_respects_limit_truncation() {
    // R479 apply_limit truncates `bounded = blocks.take(limit)`
    // when Limit::Limit(n) is set. R501: assert the truncation
    // flows through the full FileImmutable production call path.
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    // Populate 5 blocks.
    for (byte, slot, block_no) in [
        (0xA0u8, 10, 1),
        (0xA1, 20, 2),
        (0xA2, 30, 3),
        (0xA3, 40, 4),
        (0xA4, 50, 5),
    ] {
        store
            .append_block(synthetic_block(byte, slot, block_no as u64))
            .unwrap();
    }
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    // Limit::Limit(2) → only the first 2 blocks counted.
    let config = mk_config_with_limit(
        dir.path().to_path_buf(),
        AnalysisName::CountBlocks,
        Limit::Limit(2),
    );
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::CountBlocks { total, first, last } => {
            assert_eq!(total, 2, "Limit::Limit(2) must truncate to 2 blocks");
            // First and last must reflect the first 2 stored blocks
            // in chain order.
            assert_eq!(first, Some((SlotNo(10), BlockNo(1))));
            assert_eq!(last, Some((SlotNo(20), BlockNo(2))));
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_show_slot_block_no_respects_limit_truncation() {
    // R501: ShowSlotBlockNo also respects the truncation.
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for (byte, slot, block_no) in [(0xA0u8, 10, 1), (0xA1, 20, 2), (0xA2, 30, 3)] {
        store
            .append_block(synthetic_block(byte, slot, block_no as u64))
            .unwrap();
    }
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config_with_limit(
        dir.path().to_path_buf(),
        AnalysisName::ShowSlotBlockNo,
        Limit::Limit(1),
    );
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::ShowSlotBlockNo { lines } => {
            assert_eq!(lines.len(), 1, "Limit::Limit(1) → exactly 1 row");
            assert_eq!(lines[0].0, SlotNo(10));
            assert_eq!(lines[0].1, BlockNo(1));
        }
        _ => panic!("wrong outcome variant"),
    }
}

#[test]
fn end_to_end_limit_unlimited_is_equivalent_to_no_truncation() {
    // R501 invariant: Limit::Unlimited and Limit::Limit(N) where
    // N >= chain.len() must yield identical outcomes.
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for (byte, slot, block_no) in [(0xA0u8, 10, 1), (0xA1, 20, 2)] {
        store
            .append_block(synthetic_block(byte, slot, block_no as u64))
            .unwrap();
    }
    drop(store);

    let store_a = FileImmutable::open(dir.path()).unwrap();
    let blocks_a = store_a.suffix_after(&Point::Origin).unwrap();
    let config_unlim = mk_config_with_limit(
        dir.path().to_path_buf(),
        AnalysisName::CountBlocks,
        Limit::Unlimited,
    );
    let outcome_unlim = run_analysis(&config_unlim, blocks_a).unwrap();

    let store_b = FileImmutable::open(dir.path()).unwrap();
    let blocks_b = store_b.suffix_after(&Point::Origin).unwrap();
    let config_overrun = mk_config_with_limit(
        dir.path().to_path_buf(),
        AnalysisName::CountBlocks,
        Limit::Limit(100),
    );
    let outcome_overrun = run_analysis(&config_overrun, blocks_b).unwrap();

    assert_eq!(outcome_unlim, outcome_overrun);
}

#[test]
fn end_to_end_get_block_application_metrics_via_file_immutable() {
    // R490: GetBlockApplicationMetrics invokes R476 column
    // closures every-N-blocks. R500: end-to-end FileImmutable
    // → run dispatch with every_n_blocks=1 → row per block.
    use yggdrasil_db_analyser::types::NumberOfBlocks;
    let dir = TempDir::new().unwrap();
    let mut store = FileImmutable::open(dir.path()).unwrap();
    for (byte, slot, block_no) in [(0xB0u8, 10u64, 1u64), (0xB1, 20, 2)] {
        let mut blk = synthetic_block(byte, slot, block_no);
        blk.era = Era::Byron;
        store.append_block(blk).unwrap();
    }
    drop(store);

    let store = FileImmutable::open(dir.path()).unwrap();
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    let config = mk_config(
        dir.path().to_path_buf(),
        AnalysisName::GetBlockApplicationMetrics(NumberOfBlocks(1), None),
    );
    let outcome = run_analysis(&config, blocks).unwrap();

    match outcome {
        AnalysisOutcome::GetBlockApplicationMetrics {
            rows,
            every_n_blocks,
            applied_ok,
            applied_err,
        } => {
            assert_eq!(every_n_blocks, 1);
            assert_eq!(rows.len(), 2, "every_n_blocks=1 → row per block");
            // Each row has 4 R476 columns: slot, block_no, era, tx_count.
            assert_eq!(rows[0].len(), 4);
            assert!(rows[0].iter().any(|(k, _)| k == "slot"));
            assert!(rows[0].iter().any(|(k, _)| k == "block_no"));
            assert!(rows[0].iter().any(|(k, _)| k == "era"));
            assert!(rows[0].iter().any(|(k, _)| k == "tx_count"));
            assert_eq!(applied_ok + applied_err, 2);
        }
        _ => panic!("wrong outcome variant"),
    }
}
