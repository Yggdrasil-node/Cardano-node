//! End-to-end integration tests for the db-synthesizer Phase 4 R1
//! forge-loop slice.
//!
//! These exercise the full argv → `parser::Args` → `lib::run` →
//! `run::synthesize_default` path and verify the synthesized ChainDB
//! is structurally valid: it can be reopened from disk by yggdrasil's
//! own `FileImmutable` and the block count / chaining invariants
//! hold.
//!
//! What these tests establish:
//! - `run()` no longer returns the `ForgeLoopDeferred` stub error.
//! - `--blocks N` produces exactly `N` structurally-valid blocks on
//!   disk under `--db`.
//! - The synthesized chain is genuinely prev-hash-threaded.
//! - `preOpenChainDB` create / append / force semantics behave.
//!
//! What they explicitly do NOT establish (deferred to R2/R3):
//! - Praos validity (no VRF/KES/OpCert).
//! - Byte-equivalence with the upstream `db-synthesizer` binary's
//!   ChainDB chunk format.

#![allow(clippy::unwrap_used)]

use yggdrasil_db_synthesizer::{parser, run};
use yggdrasil_ledger::{BlockNo, HeaderHash, Point, SlotNo};
use yggdrasil_storage::{FileImmutable, ImmutableStore};

/// Build a minimal `parser::Args` for a `--blocks N` invocation.
fn args_for(config: &str, db: &std::path::Path, n: u64, mode: &str) -> parser::Args {
    let db = db.to_string_lossy().into_owned();
    let mut argv = vec![
        "--config".to_string(),
        config.to_string(),
        "--db".to_string(),
        db,
        "--blocks".to_string(),
        n.to_string(),
    ];
    match mode {
        "force" => argv.push("-f".to_string()),
        "append" => argv.push("-a".to_string()),
        _ => {}
    }
    parser::parse_args(&argv).expect("parses")
}

#[test]
fn run_synthesizes_chain_db_that_reopens_from_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");
    let args = args_for("/unused/config.json", &target, 12, "create");

    // The former `ForgeLoopDeferred` stub returned Err here.
    yggdrasil_db_synthesizer::run(&args).expect("run succeeds");

    // Verification: reopen the synthesized ChainDB with yggdrasil's
    // own FileImmutable and confirm the block count.
    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 12, "12 blocks synthesized + persisted");
}

#[test]
fn synthesized_chain_is_prev_hash_threaded() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");
    let args = args_for("/unused/config.json", &target, 8, "create");
    yggdrasil_db_synthesizer::run(&args).expect("run succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    let blocks = store.suffix_after(&Point::Origin).expect("walks chain");
    assert_eq!(blocks.len(), 8);

    // Genesis successor carries the all-zero prev-hash.
    assert_eq!(blocks[0].header.prev_hash, HeaderHash([0u8; 32]));
    assert_eq!(blocks[0].header.block_no, BlockNo(0));
    assert_eq!(blocks[0].header.slot_no, SlotNo(0));

    // Each subsequent block points at its real predecessor and
    // increments the block number.
    for w in blocks.windows(2) {
        assert_eq!(w[1].header.prev_hash, w[0].header.hash);
        assert_eq!(w[1].header.block_no.0, w[0].header.block_no.0 + 1);
        assert_eq!(w[1].header.slot_no.0, w[0].header.slot_no.0 + 1);
        assert!(w[1].transactions.is_empty(), "structural blocks are empty");
    }
}

#[test]
fn append_mode_resumes_and_extends_chain_db() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");

    // First pass: create with 5 blocks.
    let create = args_for("/unused/config.json", &target, 5, "create");
    yggdrasil_db_synthesizer::run(&create).expect("create succeeds");
    assert_eq!(FileImmutable::open(&target).unwrap().len(), 5);

    // Second pass: append 7 more — total 12.
    let append = args_for("/unused/config.json", &target, 7, "append");
    yggdrasil_db_synthesizer::run(&append).expect("append succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 12);
    let blocks = store.suffix_after(&Point::Origin).unwrap();
    // The chain stays consistently threaded across the two passes.
    for w in blocks.windows(2) {
        assert_eq!(w[1].header.prev_hash, w[0].header.hash);
    }
    assert_eq!(blocks.last().unwrap().header.block_no, BlockNo(11));
}

#[test]
fn force_mode_overwrites_existing_chain_db() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");

    let create = args_for("/unused/config.json", &target, 20, "create");
    yggdrasil_db_synthesizer::run(&create).expect("create succeeds");

    // Force-recreate with a shorter chain.
    let force = args_for("/unused/config.json", &target, 3, "force");
    yggdrasil_db_synthesizer::run(&force).expect("force succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 3, "force wiped the 20-block chain");
}

#[test]
fn create_mode_refuses_existing_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");

    let create = args_for("/unused/config.json", &target, 4, "create");
    yggdrasil_db_synthesizer::run(&create).expect("first create succeeds");

    // A second create against the now-existing dir must fail with the
    // upstream-shaped AlreadyExists error.
    let create_again = args_for("/unused/config.json", &target, 4, "create");
    let err = yggdrasil_db_synthesizer::run(&create_again).expect_err("second create fails");
    let msg = format!("{err:#}");
    assert!(msg.contains("already exists"), "got: {msg}");
}

#[test]
fn zero_block_limit_is_a_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("chaindb");
    let args = args_for("/unused/config.json", &target, 0, "create");
    yggdrasil_db_synthesizer::run(&args).expect("run succeeds");

    let store = FileImmutable::open(&target).expect("reopens");
    assert_eq!(store.len(), 0);
}

#[test]
fn synthesize_default_is_deterministic_across_runs() {
    // Two independent synth runs with the same limit produce
    // byte-identical chains (deterministic structural synthesis).
    let make = || {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("chaindb");
        run::synthesize_default(
            parser::parse_args(["--config", "/c", "--db", "/unused", "--blocks", "6"])
                .unwrap()
                .options,
            &target,
        )
        .unwrap();
        let store = FileImmutable::open(&target).unwrap();
        store
            .suffix_after(&Point::Origin)
            .unwrap()
            .into_iter()
            .map(|b| b.header.hash)
            .collect::<Vec<_>>()
    };
    assert_eq!(make(), make(), "structural synthesis is deterministic");
}
