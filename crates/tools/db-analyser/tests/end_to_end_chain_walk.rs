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
