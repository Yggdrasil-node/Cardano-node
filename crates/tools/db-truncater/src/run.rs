//! Run.hs equivalent for db-truncater.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBTruncater/Run.hs.
//!
//! Direct port of upstream's `Cardano.Tools.DBTruncater.Run.run`:
//!
//! 1. Open the ImmutableDB at the operator-supplied path.
//! 2. Resolve the truncate target to a `SlotNo` (passing through for
//!    [`TruncateAfter::TruncateAfterSlot`]; scanning the immutable
//!    chain for a block matching the supplied `BlockNo` for
//!    [`TruncateAfter::TruncateAfterBlock`]).
//! 3. Apply [`ImmutableStore::trim_after_slot`].
//! 4. Report the number of blocks removed.
//!
//! Carve-outs (NOT ported, by design):
//!
//! - Upstream wraps the truncate procedure in an async ChainDB
//!   context (`ChainDB.openDB` + bracket); Yggdrasil's `FileImmutable`
//!   is synchronous so the bracket is collapsed to a `?`-propagated
//!   open call.
//! - Upstream supports custom block representations via
//!   `Ouroboros.Consensus.Block.Abstract.Cardano`; Yggdrasil operates
//!   on era-tagged CBOR pass-through so the type-level dispatch
//!   collapses.

use yggdrasil_ledger::{BlockNo, Point, SlotNo};
use yggdrasil_storage::{FileImmutable, ImmutableStore, StorageError};

use crate::types::{DBTruncaterConfig, TruncateAfter};

/// Errors from the truncate run.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Underlying storage failure.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    /// `--truncate-after-block N` was supplied but no immutable
    /// block has block-number `N`.
    #[error("block number {0:?} not found in immutable DB")]
    BlockNumberNotFound(BlockNo),
}

/// Outcome of a successful truncate run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TruncateOutcome {
    /// Resolved slot number that the truncate operation acted on.
    /// For [`TruncateAfter::TruncateAfterSlot`] this is the supplied
    /// value verbatim; for [`TruncateAfter::TruncateAfterBlock`] it
    /// is the slot of the matching block.
    pub resolved_slot: SlotNo,
    /// Number of blocks removed from the immutable DB.
    pub blocks_removed: usize,
}

/// Open the configured ChainDB and apply the requested truncation.
///
/// This is the operator entry point invoked from `lib.rs::run()`.
/// Returns the [`TruncateOutcome`] on success, or [`RunError`] if
/// the DB cannot be opened, the block-number lookup fails, or the
/// trim itself fails.
pub fn run(config: &DBTruncaterConfig) -> Result<TruncateOutcome, RunError> {
    let mut store = FileImmutable::open(&config.db_dir)?;
    run_with_store(config, &mut store)
}

/// Truncate-procedure body, generic over any [`ImmutableStore`] impl.
///
/// Split out of [`run`] so the procedure is unit-testable against
/// the in-memory store without requiring a temp directory + file IO.
pub fn run_with_store<S: ImmutableStore>(
    config: &DBTruncaterConfig,
    store: &mut S,
) -> Result<TruncateOutcome, RunError> {
    let resolved_slot = resolve_target(&config.truncate_after, store)?;
    let blocks_removed = store.trim_after_slot(resolved_slot)?;
    Ok(TruncateOutcome {
        resolved_slot,
        blocks_removed,
    })
}

/// Resolve a [`TruncateAfter`] to the concrete `SlotNo` that
/// [`ImmutableStore::trim_after_slot`] expects.
pub fn resolve_target<S: ImmutableStore>(
    target: &TruncateAfter,
    store: &S,
) -> Result<SlotNo, RunError> {
    match target {
        TruncateAfter::TruncateAfterSlot(slot) => Ok(*slot),
        TruncateAfter::TruncateAfterBlock(block_no) => {
            let blocks = store.suffix_after(&Point::Origin)?;
            let found = blocks
                .iter()
                .find(|b| b.header.block_no == *block_no)
                .ok_or(RunError::BlockNumberNotFound(*block_no))?;
            Ok(found.header.slot_no)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use yggdrasil_ledger::{Block, BlockHeader, Era, HeaderHash};
    use yggdrasil_storage::InMemoryImmutable;

    fn test_block(byte: u8, slot: u64, block_no: u64) -> Block {
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
            header_cbor_size: None,
        }
    }

    fn populated_store() -> InMemoryImmutable {
        let mut store = InMemoryImmutable::default();
        store.append_block(test_block(0x01, 10, 1)).unwrap();
        store.append_block(test_block(0x02, 20, 2)).unwrap();
        store.append_block(test_block(0x03, 30, 3)).unwrap();
        store.append_block(test_block(0x04, 40, 4)).unwrap();
        store
    }

    #[test]
    fn resolve_target_truncate_after_slot_passes_through() {
        let store = populated_store();
        let target = TruncateAfter::TruncateAfterSlot(SlotNo(25));
        let resolved = resolve_target(&target, &store).expect("resolves");
        assert_eq!(resolved, SlotNo(25));
    }

    #[test]
    fn resolve_target_truncate_after_block_finds_slot() {
        let store = populated_store();
        // block_no 3 → slot 30
        let target = TruncateAfter::TruncateAfterBlock(BlockNo(3));
        let resolved = resolve_target(&target, &store).expect("resolves");
        assert_eq!(resolved, SlotNo(30));
    }

    #[test]
    fn resolve_target_truncate_after_block_errors_when_not_found() {
        let store = populated_store();
        let target = TruncateAfter::TruncateAfterBlock(BlockNo(999));
        let err = resolve_target(&target, &store).expect_err("errors");
        assert!(matches!(err, RunError::BlockNumberNotFound(BlockNo(999))));
    }

    #[test]
    fn run_with_store_truncate_after_slot_happy_path() {
        let mut store = populated_store();
        let config = DBTruncaterConfig {
            db_dir: std::path::PathBuf::from("/unused"),
            truncate_after: TruncateAfter::TruncateAfterSlot(SlotNo(25)),
            verbose: false,
        };
        let outcome = run_with_store(&config, &mut store).expect("succeeds");
        assert_eq!(outcome.resolved_slot, SlotNo(25));
        assert_eq!(outcome.blocks_removed, 2);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn run_with_store_truncate_after_block_happy_path() {
        let mut store = populated_store();
        let config = DBTruncaterConfig {
            db_dir: std::path::PathBuf::from("/unused"),
            truncate_after: TruncateAfter::TruncateAfterBlock(BlockNo(2)),
            verbose: false,
        };
        let outcome = run_with_store(&config, &mut store).expect("succeeds");
        assert_eq!(outcome.resolved_slot, SlotNo(20));
        assert_eq!(outcome.blocks_removed, 2);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn run_with_store_truncate_after_block_not_found_propagates_error() {
        let mut store = populated_store();
        let config = DBTruncaterConfig {
            db_dir: std::path::PathBuf::from("/unused"),
            truncate_after: TruncateAfter::TruncateAfterBlock(BlockNo(999)),
            verbose: false,
        };
        let err = run_with_store(&config, &mut store).expect_err("errors");
        assert!(matches!(err, RunError::BlockNumberNotFound(BlockNo(999))));
        assert_eq!(store.len(), 4, "no blocks removed on error");
    }

    #[test]
    fn run_with_store_truncate_beyond_tip_is_noop() {
        let mut store = populated_store();
        let config = DBTruncaterConfig {
            db_dir: std::path::PathBuf::from("/unused"),
            truncate_after: TruncateAfter::TruncateAfterSlot(SlotNo(9999)),
            verbose: false,
        };
        let outcome = run_with_store(&config, &mut store).expect("succeeds");
        assert_eq!(outcome.blocks_removed, 0);
        assert_eq!(store.len(), 4);
    }

    #[test]
    fn run_opens_file_immutable_at_db_dir() {
        // Smoke test: build a tempdir-backed FileImmutable, append a
        // few blocks, persist, then `run()` against the dir.
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut store = yggdrasil_storage::FileImmutable::open(dir.path()).expect("open");
            store.append_block(test_block(0x01, 10, 1)).unwrap();
            store.append_block(test_block(0x02, 20, 2)).unwrap();
            store.append_block(test_block(0x03, 30, 3)).unwrap();
        }
        let config = DBTruncaterConfig {
            db_dir: dir.path().to_path_buf(),
            truncate_after: TruncateAfter::TruncateAfterSlot(SlotNo(15)),
            verbose: false,
        };
        let outcome = run(&config).expect("run succeeds");
        assert_eq!(outcome.blocks_removed, 2);
        assert_eq!(outcome.resolved_slot, SlotNo(15));

        // Re-open and verify persistence.
        let store2 = yggdrasil_storage::FileImmutable::open(dir.path()).expect("reopen");
        assert_eq!(store2.len(), 1);
    }
}
