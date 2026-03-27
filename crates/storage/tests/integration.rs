use yggdrasil_ledger::{
    Block, BlockHeader, BlockNo, CborEncode, Era, HeaderHash, LedgerState, MultiEraTxOut,
    Point, ShelleyTxIn, ShelleyTxOut, SlotNo, Tx, TxId,
};
use yggdrasil_storage::{
    ChainDb, FileImmutable, FileLedgerStore, FileVolatile, ImmutableStore,
    InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile, LedgerStore, VolatileStore,
};

/// Helper: build a minimal block with the given hash byte and slot.
fn test_block(hash_byte: u8, slot: u64) -> Block {
    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([hash_byte; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(slot),
            issuer_vkey: [0; 32],
        },
        transactions: Vec::new(),
        raw_cbor: None,
    }
}

// ---------------------------------------------------------------------------
// Immutable store
// ---------------------------------------------------------------------------

#[test]
fn immutable_starts_at_origin() {
    let store = InMemoryImmutable::default();
    assert_eq!(store.get_tip(), Point::Origin);
    assert!(store.is_empty());
}

#[test]
fn immutable_append_and_tip() {
    let mut store = InMemoryImmutable::default();
    store
        .append_block(test_block(0xAA, 1))
        .expect("first append should succeed");
    assert_eq!(store.len(), 1);
    assert_eq!(
        store.get_tip(),
        Point::BlockPoint(SlotNo(1), HeaderHash([0xAA; 32]))
    );
}

#[test]
fn immutable_get_block() {
    let mut store = InMemoryImmutable::default();
    store
        .append_block(test_block(0xBB, 5))
        .expect("append should succeed");

    let hash = HeaderHash([0xBB; 32]);
    let block = store.get_block(&hash).expect("block should be found");
    assert_eq!(block.header.slot_no, SlotNo(5));

    let missing = HeaderHash([0xFF; 32]);
    assert!(store.get_block(&missing).is_none());
}

#[test]
fn immutable_suffix_after_returns_expected_range() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 10)).expect("append 10");
    store.append_block(test_block(0x02, 20)).expect("append 20");
    store.append_block(test_block(0x03, 30)).expect("append 30");

    let all = store.suffix_after(&Point::Origin).expect("suffix from origin");
    assert_eq!(all.len(), 3);

    let after_first = store
        .suffix_after(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])))
        .expect("suffix after first");
    assert_eq!(after_first.len(), 2);
    assert_eq!(after_first[0].header.slot_no, SlotNo(20));

    let before_first = store
        .suffix_after(&Point::BlockPoint(SlotNo(5), HeaderHash([0xFF; 32])))
        .expect("suffix before first immutable block");
    assert_eq!(before_first.len(), 3);
}

#[test]
fn immutable_rejects_duplicate() {
    let mut store = InMemoryImmutable::default();
    store
        .append_block(test_block(0xCC, 1))
        .expect("first append");
    store
        .append_block(test_block(0xCC, 2))
        .expect_err("duplicate hash should be rejected");
}

// ---------------------------------------------------------------------------
// Volatile store
// ---------------------------------------------------------------------------

#[test]
fn volatile_add_and_rollback() {
    let mut store = InMemoryVolatile::default();
    store
        .add_block(test_block(0x01, 10))
        .expect("add block 1");
    store
        .add_block(test_block(0x02, 11))
        .expect("add block 2");

    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(11), HeaderHash([0x02; 32]))
    );

    // Roll back to block 1.
    store.rollback_to(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])));
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]))
    );

    // Roll back to origin.
    store.rollback_to(&Point::Origin);
    assert_eq!(store.tip(), Point::Origin);
}

#[test]
fn volatile_rejects_duplicate() {
    let mut store = InMemoryVolatile::default();
    store
        .add_block(test_block(0xDD, 1))
        .expect("first add");
    store
        .add_block(test_block(0xDD, 2))
        .expect_err("duplicate hash should be rejected");
}

#[test]
fn volatile_suffix_after_origin_returns_all() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_block(0x01, 10)).unwrap();
    store.add_block(test_block(0x02, 11)).unwrap();
    store.add_block(test_block(0x03, 12)).unwrap();

    let suffix = store.suffix_after(&Point::Origin);
    assert_eq!(suffix.len(), 3);
    assert_eq!(suffix[0].header.slot_no, SlotNo(10));
    assert_eq!(suffix[2].header.slot_no, SlotNo(12));
}

#[test]
fn volatile_suffix_after_mid_block() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_block(0x01, 10)).unwrap();
    store.add_block(test_block(0x02, 11)).unwrap();
    store.add_block(test_block(0x03, 12)).unwrap();

    let suffix = store.suffix_after(&Point::BlockPoint(
        SlotNo(10),
        HeaderHash([0x01; 32]),
    ));
    assert_eq!(suffix.len(), 2);
    assert_eq!(suffix[0].header.hash, HeaderHash([0x02; 32]));
    assert_eq!(suffix[1].header.hash, HeaderHash([0x03; 32]));
}

#[test]
fn volatile_suffix_after_tip_returns_empty() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_block(0x01, 10)).unwrap();
    store.add_block(test_block(0x02, 11)).unwrap();

    let suffix = store.suffix_after(&Point::BlockPoint(
        SlotNo(11),
        HeaderHash([0x02; 32]),
    ));
    assert!(suffix.is_empty());
}

#[test]
fn volatile_suffix_after_unknown_point_returns_empty() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_block(0x01, 10)).unwrap();

    let suffix = store.suffix_after(&Point::BlockPoint(
        SlotNo(99),
        HeaderHash([0xFF; 32]),
    ));
    assert!(suffix.is_empty());
}

// ---------------------------------------------------------------------------
// Ledger snapshot store
// ---------------------------------------------------------------------------

#[test]
fn ledger_store_snapshot_round_trip() {
    let mut store = InMemoryLedgerStore::default();
    assert_eq!(store.count(), 0);
    assert!(store.latest_snapshot().is_none());

    store
        .save_snapshot(SlotNo(100), vec![1, 2, 3])
        .expect("save snapshot");
    assert_eq!(store.count(), 1);

    let (slot, data) = store.latest_snapshot().expect("should have a snapshot");
    assert_eq!(slot, SlotNo(100));
    assert_eq!(data, &[1, 2, 3]);
}

#[test]
fn ledger_store_can_lookup_and_truncate_snapshots() {
    let mut store = InMemoryLedgerStore::default();
    store.save_snapshot(SlotNo(10), vec![0x0A]).expect("save 10");
    store.save_snapshot(SlotNo(20), vec![0x14]).expect("save 20");
    store.save_snapshot(SlotNo(30), vec![0x1E]).expect("save 30");

    let (slot, data) = store
        .latest_snapshot_before_or_at(SlotNo(25))
        .expect("snapshot before or at slot");
    assert_eq!(slot, SlotNo(20));
    assert_eq!(data, &[0x14]);

    store.truncate_after(Some(SlotNo(20))).expect("truncate");
    assert_eq!(store.count(), 2);
    assert!(store.latest_snapshot_before_or_at(SlotNo(25)).is_some());
    assert!(store.latest_snapshot_before_or_at(SlotNo(30)).is_some());

    let (latest_slot, latest_data) = store.latest_snapshot().expect("latest snapshot");
    assert_eq!(latest_slot, SlotNo(20));
    assert_eq!(latest_data, &[0x14]);
}

#[test]
fn ledger_store_replaces_same_slot_and_retains_latest_snapshots() {
    let mut store = InMemoryLedgerStore::default();
    store.save_snapshot(SlotNo(10), vec![0x0A]).expect("save 10");
    store.save_snapshot(SlotNo(20), vec![0x14]).expect("save 20");
    store.save_snapshot(SlotNo(20), vec![0x2A]).expect("replace 20");
    store.save_snapshot(SlotNo(30), vec![0x1E]).expect("save 30");

    assert_eq!(store.count(), 3);

    let (slot, data) = store.latest_snapshot().expect("latest snapshot");
    let latest_slot = slot;
    let latest_data = data.to_vec();
    assert_eq!(slot, SlotNo(30));
    assert_eq!(
        store
            .latest_snapshot_before_or_at(SlotNo(20))
            .expect("snapshot at 20")
            .1,
        &[0x2A]
    );

    store.retain_latest(2).expect("retain latest");
    assert_eq!(store.count(), 2);
    assert_eq!(latest_slot, SlotNo(30));
    let (retained_slot, retained_data) = store
        .latest_snapshot_before_or_at(SlotNo(20))
        .expect("retained snapshot at 20");
    assert_eq!(retained_slot, SlotNo(20));
    assert_eq!(retained_data, &[0x2A]);
    assert!(store.latest_snapshot_before_or_at(SlotNo(10)).is_none());
    assert_eq!(latest_data, vec![0x1E]);
}

// ---------------------------------------------------------------------------
// Cross-store integration
// ---------------------------------------------------------------------------

#[test]
fn cross_store_block_flow() {
    // Simulate a block arriving, landing in volatile, then being finalized
    // into the immutable store.
    let block = Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([0xEE; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(42),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions: vec![Tx {
            id: TxId([0xFF; 32]),
            body: vec![0xCA, 0xFE],
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        }],
        raw_cbor: None,
    };

    let mut volatile = InMemoryVolatile::default();
    volatile
        .add_block(block.clone())
        .expect("volatile add");

    // Finalize into immutable.
    let mut immutable = InMemoryImmutable::default();
    immutable
        .append_block(block)
        .expect("immutable append");

    assert_eq!(immutable.get_tip(), volatile.tip());
}

// ===========================================================================
// File-backed immutable store
// ===========================================================================

#[test]
fn file_immutable_starts_at_origin() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let store = FileImmutable::open(dir.path().join("imm")).expect("open");
    assert_eq!(store.get_tip(), Point::Origin);
    assert!(store.is_empty());
}

#[test]
fn file_immutable_append_and_tip() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileImmutable::open(dir.path().join("imm")).expect("open");
    store
        .append_block(test_block(0xAA, 1))
        .expect("first append");
    assert_eq!(store.len(), 1);
    assert_eq!(
        store.get_tip(),
        Point::BlockPoint(SlotNo(1), HeaderHash([0xAA; 32]))
    );
}

#[test]
fn file_immutable_suffix_after_returns_expected_range() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileImmutable::open(dir.path().join("imm")).expect("open");
    store.append_block(test_block(0x01, 10)).expect("append 10");
    store.append_block(test_block(0x02, 20)).expect("append 20");
    store.append_block(test_block(0x03, 30)).expect("append 30");

    let suffix = store
        .suffix_after(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])))
        .expect("suffix after first");
    assert_eq!(suffix.len(), 2);
    assert_eq!(suffix[0].header.slot_no, SlotNo(20));
    assert_eq!(suffix[1].header.slot_no, SlotNo(30));
}

#[test]
fn file_immutable_get_block() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileImmutable::open(dir.path().join("imm")).expect("open");
    store
        .append_block(test_block(0xBB, 5))
        .expect("append");

    let hash = HeaderHash([0xBB; 32]);
    let block = store.get_block(&hash).expect("found");
    assert_eq!(block.header.slot_no, SlotNo(5));

    let missing = HeaderHash([0xFF; 32]);
    assert!(store.get_block(&missing).is_none());
}

#[test]
fn file_immutable_rejects_duplicate() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileImmutable::open(dir.path().join("imm")).expect("open");
    store.append_block(test_block(0xCC, 1)).expect("first");
    store
        .append_block(test_block(0xCC, 2))
        .expect_err("duplicate");
}

#[test]
fn file_immutable_persists_across_reopens() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let path = dir.path().join("imm");

    {
        let mut store = FileImmutable::open(&path).expect("open");
        store.append_block(test_block(0x01, 10)).expect("append 1");
        store.append_block(test_block(0x02, 20)).expect("append 2");
    }
    // Re-open.
    let store = FileImmutable::open(&path).expect("reopen");
    assert_eq!(store.len(), 2);
    assert_eq!(
        store.get_tip(),
        Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32]))
    );
    assert!(store.get_block(&HeaderHash([0x01; 32])).is_some());
}

// ===========================================================================
// File-backed volatile store
// ===========================================================================

#[test]
fn file_volatile_add_and_rollback() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileVolatile::open(dir.path().join("vol")).expect("open");
    store.add_block(test_block(0x01, 10)).expect("add 1");
    store.add_block(test_block(0x02, 11)).expect("add 2");

    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(11), HeaderHash([0x02; 32]))
    );

    store.rollback_to(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])));
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]))
    );

    store.rollback_to(&Point::Origin);
    assert_eq!(store.tip(), Point::Origin);
}

#[test]
fn file_volatile_rejects_duplicate() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileVolatile::open(dir.path().join("vol")).expect("open");
    store.add_block(test_block(0xDD, 1)).expect("first");
    store
        .add_block(test_block(0xDD, 2))
        .expect_err("duplicate");
}

#[test]
fn file_volatile_persists_and_rollback_removes_files() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let path = dir.path().join("vol");

    {
        let mut store = FileVolatile::open(&path).expect("open");
        store.add_block(test_block(0x01, 10)).expect("add 1");
        store.add_block(test_block(0x02, 11)).expect("add 2");
        store.add_block(test_block(0x03, 12)).expect("add 3");
        // Rollback removes block 3's file.
        store.rollback_to(&Point::BlockPoint(SlotNo(11), HeaderHash([0x02; 32])));
    }

    // Re-open: should see only 2 blocks.
    let store = FileVolatile::open(&path).expect("reopen");
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(11), HeaderHash([0x02; 32]))
    );
    assert!(store.get_block(&HeaderHash([0x03; 32])).is_none());
}

#[test]
fn volatile_prefix_helpers_promote_prefixes() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_block(0x01, 10)).expect("add 1");
    store.add_block(test_block(0x02, 11)).expect("add 2");
    store.add_block(test_block(0x03, 12)).expect("add 3");

    let prefix = store
        .prefix_up_to(&Point::BlockPoint(SlotNo(11), HeaderHash([0x02; 32])))
        .expect("prefix up to block 2");
    assert_eq!(prefix.len(), 2);
    assert_eq!(prefix[0].header.hash, HeaderHash([0x01; 32]));
    assert_eq!(prefix[1].header.hash, HeaderHash([0x02; 32]));

    store
        .prune_up_to(&Point::BlockPoint(SlotNo(11), HeaderHash([0x02; 32])))
        .expect("prune through block 2");
    assert_eq!(store.tip(), Point::BlockPoint(SlotNo(12), HeaderHash([0x03; 32])));
    assert!(store.get_block(&HeaderHash([0x01; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x02; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x03; 32])).is_some());
}

// ===========================================================================
// File-backed ledger snapshot store
// ===========================================================================

#[test]
fn file_ledger_store_round_trip() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let mut store = FileLedgerStore::open(dir.path().join("ledger")).expect("open");
    assert_eq!(store.count(), 0);
    assert!(store.latest_snapshot().is_none());

    store
        .save_snapshot(SlotNo(100), vec![1, 2, 3])
        .expect("save");
    assert_eq!(store.count(), 1);

    let (slot, data) = store.latest_snapshot().expect("snapshot");
    assert_eq!(slot, SlotNo(100));
    assert_eq!(data, &[1, 2, 3]);
}

#[test]
fn file_ledger_store_persists_across_reopens() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let path = dir.path().join("ledger");

    {
        let mut store = FileLedgerStore::open(&path).expect("open");
        store
            .save_snapshot(SlotNo(50), vec![0xAA])
            .expect("save 1");
        store
            .save_snapshot(SlotNo(200), vec![0xBB, 0xCC])
            .expect("save 2");
    }

    let store = FileLedgerStore::open(&path).expect("reopen");
    assert_eq!(store.count(), 2);
    let (slot, data) = store.latest_snapshot().expect("snapshot");
    assert_eq!(slot, SlotNo(200));
    assert_eq!(data, &[0xBB, 0xCC]);
}

#[test]
fn file_ledger_store_can_lookup_and_truncate_snapshots() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let path = dir.path().join("ledger");

    {
        let mut store = FileLedgerStore::open(&path).expect("open");
        store.save_snapshot(SlotNo(10), vec![0x0A]).expect("save 10");
        store.save_snapshot(SlotNo(20), vec![0x14]).expect("save 20");
        store.save_snapshot(SlotNo(30), vec![0x1E]).expect("save 30");
        store.truncate_after(Some(SlotNo(20))).expect("truncate");
    }

    let store = FileLedgerStore::open(&path).expect("reopen");
    assert_eq!(store.count(), 2);
    let (slot, data) = store
        .latest_snapshot_before_or_at(SlotNo(25))
        .expect("snapshot before or at slot");
    assert_eq!(slot, SlotNo(20));
    assert_eq!(data, &[0x14]);
    assert!(store.latest_snapshot_before_or_at(SlotNo(30)).is_some());
}

#[test]
fn file_ledger_store_replaces_same_slot_and_retains_latest_snapshots() {
    let dir = tempfile::tempdir().expect("tmp dir");
    let path = dir.path().join("ledger");

    {
        let mut store = FileLedgerStore::open(&path).expect("open");
        store.save_snapshot(SlotNo(10), vec![0x0A]).expect("save 10");
        store.save_snapshot(SlotNo(20), vec![0x14]).expect("save 20");
        store.save_snapshot(SlotNo(20), vec![0x2A]).expect("replace 20");
        store.save_snapshot(SlotNo(30), vec![0x1E]).expect("save 30");
        store.retain_latest(2).expect("retain latest");
    }

    let store = FileLedgerStore::open(&path).expect("reopen");
    assert_eq!(store.count(), 2);
    assert!(store.latest_snapshot_before_or_at(SlotNo(10)).is_none());

    let (slot, data) = store
        .latest_snapshot_before_or_at(SlotNo(20))
        .expect("snapshot at 20");
    assert_eq!(slot, SlotNo(20));
    assert_eq!(data, &[0x2A]);

    let (latest_slot, latest_data) = store.latest_snapshot().expect("latest snapshot");
    assert_eq!(latest_slot, SlotNo(30));
    assert_eq!(latest_data, &[0x1E]);
}

// ---------------------------------------------------------------------------
// ChainDb coordination
// ---------------------------------------------------------------------------

#[test]
fn chaindb_promotes_volatile_prefix_and_prunes_snapshots_on_rollback() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    chain_db
        .add_volatile_block(test_block(0x01, 10))
        .expect("add 1");
    chain_db
        .add_volatile_block(test_block(0x02, 20))
        .expect("add 2");
    chain_db
        .add_volatile_block(test_block(0x03, 30))
        .expect("add 3");
    chain_db
        .save_ledger_snapshot(SlotNo(10), vec![0x0A])
        .expect("save 10");
    chain_db
        .save_ledger_snapshot(SlotNo(30), vec![0x1E])
        .expect("save 30");

    let promoted = chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32])))
        .expect("promote prefix");
    assert_eq!(promoted, 2);
    assert_eq!(chain_db.immutable().len(), 2);
    assert_eq!(chain_db.volatile().tip(), Point::BlockPoint(SlotNo(30), HeaderHash([0x03; 32])));
    assert_eq!(chain_db.tip(), Point::BlockPoint(SlotNo(30), HeaderHash([0x03; 32])));

    chain_db
        .rollback_to(&Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32])))
        .expect("rollback to promoted point");

    let recovery = chain_db.recovery();
    assert_eq!(recovery.tip, Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32])));
    assert_eq!(recovery.ledger_snapshot_slot, Some(SlotNo(10)));
}

// ===========================================================================
// Cross-store file-backed integration
// ===========================================================================

#[test]
fn file_cross_store_block_flow() {
    let dir = tempfile::tempdir().expect("tmp dir");

    let block = Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([0xEE; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(42),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions: vec![Tx {
            id: TxId([0xFF; 32]),
            body: vec![0xCA, 0xFE],
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        }],
        raw_cbor: None,
    };

    let mut volatile = FileVolatile::open(dir.path().join("vol")).expect("open vol");
    volatile.add_block(block.clone()).expect("volatile add");

    let mut immutable = FileImmutable::open(dir.path().join("imm")).expect("open imm");
    immutable.append_block(block).expect("immutable append");

    assert_eq!(immutable.get_tip(), volatile.tip());
}

#[test]
fn chaindb_typed_ledger_checkpoint_round_trip() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let mut state = LedgerState::new(Era::Shelley);
    state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0xAB; 32]));
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: vec![0x01],
            amount: 99,
        }),
    );

    let checkpoint = state.checkpoint();
    chain_db
        .save_ledger_checkpoint(SlotNo(10), &checkpoint)
        .expect("save typed checkpoint");

    let (slot, restored) = chain_db
        .latest_ledger_checkpoint_before_or_at(SlotNo(10))
        .expect("decode typed checkpoint")
        .expect("checkpoint present");

    assert_eq!(slot, SlotNo(10));
    assert_eq!(restored, checkpoint);
    assert_eq!(restored.to_cbor_bytes(), checkpoint.to_cbor_bytes());
}

#[test]
fn chaindb_recover_ledger_state_replays_volatile_suffix_after_checkpoint() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let mut checkpoint_state = LedgerState::new(Era::Shelley);
    checkpoint_state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x0A; 32]));

    chain_db
        .save_ledger_checkpoint(SlotNo(10), &checkpoint_state.checkpoint())
        .expect("save checkpoint");
    chain_db
        .add_volatile_block(test_block(0x14, 20))
        .expect("add volatile 20");
    chain_db
        .add_volatile_block(test_block(0x1E, 30))
        .expect("add volatile 30");

    let recovered = chain_db
        .recover_ledger_state(LedgerState::new(Era::Shelley))
        .expect("recover ledger state from chaindb");

    assert_eq!(recovered.checkpoint_slot, Some(SlotNo(10)));
    assert_eq!(recovered.replayed_volatile_blocks, 2);
    assert_eq!(
        recovered.point,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
    assert_eq!(
        recovered.ledger_state.tip,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
}

#[test]
fn chaindb_recover_ledger_state_replays_immutable_suffix_after_checkpoint() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let mut checkpoint_state = LedgerState::new(Era::Shelley);
    checkpoint_state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x0A; 32]));

    chain_db
        .save_ledger_checkpoint(SlotNo(10), &checkpoint_state.checkpoint())
        .expect("save checkpoint");
    chain_db
        .immutable_mut()
        .append_block(test_block(0x14, 20))
        .expect("append immutable 20");
    chain_db
        .immutable_mut()
        .append_block(test_block(0x1E, 30))
        .expect("append immutable 30");

    let recovered = chain_db
        .recover_ledger_state(LedgerState::new(Era::Shelley))
        .expect("recover ledger state across immutable replay");

    assert_eq!(recovered.checkpoint_slot, Some(SlotNo(10)));
    assert_eq!(recovered.replayed_volatile_blocks, 0);
    assert_eq!(
        recovered.point,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
}

#[test]
fn chaindb_persist_ledger_checkpoint_prunes_to_retention_limit() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let mut state = LedgerState::new(Era::Shelley);
    state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x0A; 32]));
    let first = chain_db
        .persist_ledger_checkpoint(&state.tip, &state.checkpoint(), 2)
        .expect("persist first checkpoint");
    assert_eq!(first.retained_snapshots, 1);
    assert_eq!(first.pruned_snapshots, 0);

    state.tip = Point::BlockPoint(SlotNo(20), HeaderHash([0x14; 32]));
    let second = chain_db
        .persist_ledger_checkpoint(&state.tip, &state.checkpoint(), 2)
        .expect("persist second checkpoint");
    assert_eq!(second.retained_snapshots, 2);
    assert_eq!(second.pruned_snapshots, 0);

    state.tip = Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]));
    let third = chain_db
        .persist_ledger_checkpoint(&state.tip, &state.checkpoint(), 2)
        .expect("persist third checkpoint");
    assert_eq!(third.retained_snapshots, 2);
    assert_eq!(third.pruned_snapshots, 1);
    assert!(chain_db
        .latest_ledger_checkpoint_before_or_at(SlotNo(10))
        .expect("lookup checkpoint")
        .is_none());
    assert!(chain_db
        .latest_ledger_checkpoint_before_or_at(SlotNo(20))
        .expect("lookup checkpoint")
        .is_some());
}

#[test]
fn chaindb_checkpoint_truncation_and_clear_follow_points() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    for (slot, hash_byte) in [(10, 0x0A), (20, 0x14), (30, 0x1E)] {
        let mut state = LedgerState::new(Era::Shelley);
        state.tip = Point::BlockPoint(SlotNo(slot), HeaderHash([hash_byte; 32]));
        chain_db
            .persist_ledger_checkpoint(&state.tip, &state.checkpoint(), 4)
            .expect("persist checkpoint");
    }

    chain_db
        .truncate_ledger_checkpoints_after_point(&Point::BlockPoint(
            SlotNo(20),
            HeaderHash([0x14; 32]),
        ))
        .expect("truncate after point");
    assert!(chain_db
        .latest_ledger_checkpoint_before_or_at(SlotNo(30))
        .expect("lookup checkpoint")
        .is_some());
    let latest = chain_db.latest_ledger_checkpoint().expect("latest checkpoint");
    assert_eq!(latest.expect("checkpoint present").0, SlotNo(20));

    chain_db
        .clear_ledger_checkpoints()
        .expect("clear checkpoints");
    assert!(chain_db.latest_ledger_checkpoint().expect("latest checkpoint").is_none());
}

// ---------------------------------------------------------------------------
// Checkpoint fallback recovery
// ---------------------------------------------------------------------------

#[test]
fn chaindb_recover_ledger_state_skips_corrupt_checkpoint() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let mut ledger = InMemoryLedgerStore::default();

    // Save a valid checkpoint at slot 10.
    let mut good_state = LedgerState::new(Era::Shelley);
    good_state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x0A; 32]));
    let good_cbor = good_state.checkpoint().to_cbor_bytes();
    ledger
        .save_snapshot(SlotNo(10), good_cbor)
        .expect("save good checkpoint");

    // Save a corrupt checkpoint at slot 20 (invalid CBOR bytes).
    ledger
        .save_snapshot(SlotNo(20), vec![0xFF, 0xFE, 0xFD])
        .expect("save corrupt checkpoint");

    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    // Add a volatile block at slot 30 so the tip is slot 30.
    chain_db
        .add_volatile_block(test_block(0x1E, 30))
        .expect("add volatile 30");

    // Recovery should skip the corrupt slot-20 checkpoint and fall back to
    // the valid slot-10 checkpoint, then replay the volatile suffix.
    let recovered = chain_db
        .recover_ledger_state(LedgerState::new(Era::Shelley))
        .expect("recover should succeed via fallback");

    assert_eq!(recovered.checkpoint_slot, Some(SlotNo(10)));
    assert_eq!(recovered.replayed_volatile_blocks, 1);
    assert_eq!(
        recovered.point,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
}

#[test]
fn chaindb_recover_ledger_state_falls_through_when_all_checkpoints_corrupt() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let mut ledger = InMemoryLedgerStore::default();

    // Save only corrupt checkpoints.
    ledger
        .save_snapshot(SlotNo(5), vec![0xFF])
        .expect("save corrupt 5");
    ledger
        .save_snapshot(SlotNo(10), vec![0xFE])
        .expect("save corrupt 10");

    let mut chain_db = ChainDb::new(immutable, volatile, ledger);
    chain_db
        .add_volatile_block(test_block(0x14, 20))
        .expect("add volatile 20");

    // With all checkpoints corrupt, recovery falls through to the base state
    // and replays from scratch.
    let recovered = chain_db
        .recover_ledger_state(LedgerState::new(Era::Shelley))
        .expect("recover falls through to base state");

    assert_eq!(recovered.checkpoint_slot, None);
    assert_eq!(recovered.replayed_volatile_blocks, 1);
    assert_eq!(
        recovered.point,
        Point::BlockPoint(SlotNo(20), HeaderHash([0x14; 32]))
    );
}

// ---------------------------------------------------------------------------
// Atomic file writes — verify temp file is cleaned up
// ---------------------------------------------------------------------------

#[test]
fn file_ledger_store_does_not_leave_temp_files() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut store = FileLedgerStore::open(dir.path()).expect("open ledger store");

    store
        .save_snapshot(SlotNo(100), vec![1, 2, 3])
        .expect("save snapshot");

    // The actual file should exist.
    let expected = dir.path().join("snapshot_100.dat");
    assert!(expected.exists(), "snapshot file should exist");

    // No .tmp files should remain.
    let tmp_files: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "tmp")
        })
        .collect();
    assert!(tmp_files.is_empty(), "no temp files should remain after atomic write");
}

// ---------------------------------------------------------------------------
// Volatile → Immutable promotion
// ---------------------------------------------------------------------------

#[test]
fn promote_volatile_prefix_moves_blocks_to_immutable() {
    let chain_db = &mut ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db.add_volatile_block(test_block(0x01, 10)).unwrap();
    chain_db.add_volatile_block(test_block(0x02, 20)).unwrap();
    chain_db.add_volatile_block(test_block(0x03, 30)).unwrap();

    let promoted = chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32])))
        .unwrap();
    assert_eq!(promoted, 2);

    // First two blocks are now immutable.
    assert_eq!(chain_db.immutable().len(), 2);
    assert!(chain_db.immutable().get_block(&HeaderHash([0x01; 32])).is_some());
    assert!(chain_db.immutable().get_block(&HeaderHash([0x02; 32])).is_some());

    // Third block remains volatile.
    assert!(chain_db.volatile().get_block(&HeaderHash([0x03; 32])).is_some());
    assert!(chain_db.volatile().get_block(&HeaderHash([0x01; 32])).is_none());
}

#[test]
fn promote_all_volatile_blocks_leaves_volatile_empty() {
    let chain_db = &mut ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db.add_volatile_block(test_block(0xAA, 5)).unwrap();
    chain_db.add_volatile_block(test_block(0xBB, 10)).unwrap();

    let promoted = chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(10), HeaderHash([0xBB; 32])))
        .unwrap();
    assert_eq!(promoted, 2);
    assert_eq!(chain_db.immutable().len(), 2);
    assert_eq!(chain_db.volatile().tip(), Point::Origin);
}

#[test]
fn promote_single_block_from_volatile() {
    let chain_db = &mut ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db.add_volatile_block(test_block(0x01, 1)).unwrap();
    chain_db.add_volatile_block(test_block(0x02, 2)).unwrap();

    let promoted = chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(1), HeaderHash([0x01; 32])))
        .unwrap();
    assert_eq!(promoted, 1);
    assert_eq!(chain_db.immutable().len(), 1);
    assert_eq!(
        chain_db.volatile().tip(),
        Point::BlockPoint(SlotNo(2), HeaderHash([0x02; 32]))
    );
}

#[test]
fn promote_then_rollback_volatile_preserves_immutable() {
    let chain_db = &mut ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db.add_volatile_block(test_block(0x01, 10)).unwrap();
    chain_db.add_volatile_block(test_block(0x02, 20)).unwrap();
    chain_db.add_volatile_block(test_block(0x03, 30)).unwrap();

    chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])))
        .unwrap();
    assert_eq!(chain_db.immutable().len(), 1);

    // Rollback volatile to the middle block.
    chain_db.volatile_mut().rollback_to(&Point::BlockPoint(
        SlotNo(20),
        HeaderHash([0x02; 32]),
    ));

    // Immutable is untouched, volatile only has block at slot 20.
    assert_eq!(chain_db.immutable().len(), 1);
    assert_eq!(
        chain_db.volatile().tip(),
        Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32]))
    );
    assert!(chain_db.volatile().get_block(&HeaderHash([0x03; 32])).is_none());
}

#[test]
fn promote_volatile_prefix_point_not_found() {
    let chain_db = &mut ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db.add_volatile_block(test_block(0x01, 10)).unwrap();

    let result = chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(99), HeaderHash([0xFF; 32])));
    assert!(result.is_err(), "promoting a non-existent point should fail");
}

// ---------------------------------------------------------------------------
// ChainDb::ledger_mut / into_inner
// ---------------------------------------------------------------------------

#[test]
fn chaindb_ledger_mut_allows_direct_mutation() {
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    // Use ledger_mut() to save a snapshot directly through the underlying store.
    chain_db
        .ledger_mut()
        .save_snapshot(SlotNo(42), vec![0xCA, 0xFE])
        .expect("save via ledger_mut");

    // Verify the snapshot is visible through the immutable ledger() accessor.
    let (slot, data) = chain_db
        .ledger()
        .latest_snapshot()
        .expect("snapshot should be present");
    assert_eq!(slot, SlotNo(42));
    assert_eq!(data, &[0xCA, 0xFE]);
}

#[test]
fn chaindb_into_inner_yields_components() {
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    // Populate each store so we can verify state after decomposition.
    chain_db
        .add_volatile_block(test_block(0xA1, 10))
        .expect("add volatile block");
    chain_db
        .save_ledger_snapshot(SlotNo(10), vec![0xDD])
        .expect("save snapshot");

    let (immutable, volatile, ledger) = chain_db.into_inner();

    // Immutable was never populated.
    assert!(immutable.is_empty());

    // Volatile should contain the block we added.
    assert_eq!(
        volatile.tip(),
        Point::BlockPoint(SlotNo(10), HeaderHash([0xA1; 32]))
    );

    // Ledger should contain the snapshot we saved.
    let (slot, data) = ledger
        .latest_snapshot()
        .expect("snapshot should be present after into_inner");
    assert_eq!(slot, SlotNo(10));
    assert_eq!(data, &[0xDD]);
}

// ---------------------------------------------------------------------------
// Edge cases: volatile rollback to Origin
// ---------------------------------------------------------------------------

#[test]
fn volatile_rollback_to_origin_clears_all() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_block(0x01, 1)).expect("add block 1");
    store.add_block(test_block(0x02, 2)).expect("add block 2");
    store.add_block(test_block(0x03, 3)).expect("add block 3");

    // Rolling back to Origin should discard every block.
    store.rollback_to(&Point::Origin);

    assert_eq!(store.tip(), Point::Origin);
    assert!(store.get_block(&HeaderHash([0x01; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x02; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x03; 32])).is_none());
}

// ---------------------------------------------------------------------------
// Edge cases: ledger truncate_after(None)
// ---------------------------------------------------------------------------

#[test]
fn ledger_truncate_after_none_clears_all() {
    let mut store = InMemoryLedgerStore::default();
    store
        .save_snapshot(SlotNo(10), vec![0x0A])
        .expect("save snapshot 10");
    store
        .save_snapshot(SlotNo(20), vec![0x14])
        .expect("save snapshot 20");
    store
        .save_snapshot(SlotNo(30), vec![0x1E])
        .expect("save snapshot 30");

    // Passing None should clear all snapshots.
    store
        .truncate_after(None)
        .expect("truncate_after(None) should succeed");

    assert!(
        store.latest_snapshot().is_none(),
        "all snapshots should be cleared"
    );
}

// ---------------------------------------------------------------------------
// Edge cases: immutable suffix_after Origin
// ---------------------------------------------------------------------------

#[test]
fn immutable_suffix_after_origin_returns_all() {
    let mut store = InMemoryImmutable::default();
    store
        .append_block(test_block(0x01, 1))
        .expect("append block 1");
    store
        .append_block(test_block(0x02, 2))
        .expect("append block 2");
    store
        .append_block(test_block(0x03, 3))
        .expect("append block 3");

    // suffix_after(Origin) should return the full chain.
    let blocks = store
        .suffix_after(&Point::Origin)
        .expect("suffix_after Origin should succeed");

    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].header.slot_no, SlotNo(1));
    assert_eq!(blocks[1].header.slot_no, SlotNo(2));
    assert_eq!(blocks[2].header.slot_no, SlotNo(3));
}

// ---------------------------------------------------------------------------
// Edge cases: ChainDb recovery with empty stores
// ---------------------------------------------------------------------------

#[test]
fn chaindb_recovery_empty_stores() {
    let chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    let recovery = chain_db.recovery();

    assert_eq!(recovery.tip, Point::Origin);
    assert_eq!(recovery.ledger_snapshot_slot, None);
}

// ---------------------------------------------------------------------------
// ImmutableStore::trim_before_slot — InMemory
// ---------------------------------------------------------------------------

#[test]
fn immutable_trim_before_slot_removes_older_blocks() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 10)).unwrap();
    store.append_block(test_block(0x02, 20)).unwrap();
    store.append_block(test_block(0x03, 30)).unwrap();
    store.append_block(test_block(0x04, 40)).unwrap();

    let removed = store.trim_before_slot(SlotNo(25)).unwrap();
    assert_eq!(removed, 2, "slots 10 and 20 should be removed");
    assert_eq!(store.len(), 2);
    assert!(store.get_block(&HeaderHash([0x01; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x02; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x03; 32])).is_some());
    assert!(store.get_block(&HeaderHash([0x04; 32])).is_some());
}

#[test]
fn immutable_trim_before_slot_zero_is_noop() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 5)).unwrap();
    store.append_block(test_block(0x02, 10)).unwrap();

    let removed = store.trim_before_slot(SlotNo(0)).unwrap();
    assert_eq!(removed, 0);
    assert_eq!(store.len(), 2);
}

#[test]
fn immutable_trim_before_slot_beyond_tip_clears_all() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 5)).unwrap();
    store.append_block(test_block(0x02, 10)).unwrap();

    let removed = store.trim_before_slot(SlotNo(999)).unwrap();
    assert_eq!(removed, 2);
    assert!(store.is_empty());
    assert_eq!(store.get_tip(), Point::Origin);
}

#[test]
fn immutable_trim_before_slot_on_empty_store() {
    let mut store = InMemoryImmutable::default();
    let removed = store.trim_before_slot(SlotNo(100)).unwrap();
    assert_eq!(removed, 0);
}

#[test]
fn immutable_trim_before_slot_exact_boundary() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 10)).unwrap();
    store.append_block(test_block(0x02, 20)).unwrap();
    store.append_block(test_block(0x03, 30)).unwrap();

    // trim_before_slot(20) keeps slot 20 and later
    let removed = store.trim_before_slot(SlotNo(20)).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(store.len(), 2);
    assert!(store.get_block(&HeaderHash([0x01; 32])).is_none());
    assert!(store.get_block(&HeaderHash([0x02; 32])).is_some());
    assert!(store.get_block(&HeaderHash([0x03; 32])).is_some());
}

#[test]
fn immutable_trim_preserves_tip() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 10)).unwrap();
    store.append_block(test_block(0x02, 20)).unwrap();

    store.trim_before_slot(SlotNo(15)).unwrap();
    assert_eq!(
        store.get_tip(),
        Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32]))
    );
}

// ---------------------------------------------------------------------------
// ImmutableStore::trim_before_slot — File-backed
// ---------------------------------------------------------------------------

#[test]
fn file_immutable_trim_before_slot_removes_older_blocks() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mut store = FileImmutable::open(dir.path()).expect("open store");
    store.append_block(test_block(0x01, 10)).unwrap();
    store.append_block(test_block(0x02, 20)).unwrap();
    store.append_block(test_block(0x03, 30)).unwrap();

    let removed = store.trim_before_slot(SlotNo(25)).unwrap();
    assert_eq!(removed, 2);
    assert_eq!(store.len(), 1);
    assert!(store.get_block(&HeaderHash([0x03; 32])).is_some());

    // Verify files are actually deleted — re-open and confirm
    let store2 = FileImmutable::open(dir.path()).expect("reopen store");
    assert_eq!(store2.len(), 1);
    assert!(store2.get_block(&HeaderHash([0x01; 32])).is_none());
    assert!(store2.get_block(&HeaderHash([0x03; 32])).is_some());
}

#[test]
fn file_immutable_trim_before_slot_on_empty() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mut store = FileImmutable::open(dir.path()).expect("open store");
    let removed = store.trim_before_slot(SlotNo(100)).unwrap();
    assert_eq!(removed, 0);
}

// ---------------------------------------------------------------------------
// ChainDb::gc_immutable_before_slot
// ---------------------------------------------------------------------------

#[test]
fn chaindb_gc_immutable_before_slot() {
    let mut immutable = InMemoryImmutable::default();
    immutable.append_block(test_block(0x01, 10)).unwrap();
    immutable.append_block(test_block(0x02, 20)).unwrap();
    immutable.append_block(test_block(0x03, 30)).unwrap();
    immutable.append_block(test_block(0x04, 40)).unwrap();

    let mut chain_db = ChainDb::new(
        immutable,
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    let removed = chain_db.gc_immutable_before_slot(SlotNo(25)).unwrap();
    assert_eq!(removed, 2);
    assert_eq!(chain_db.immutable().len(), 2);
}

#[test]
fn chaindb_gc_preserves_volatile_and_ledger() {
    let mut immutable = InMemoryImmutable::default();
    immutable.append_block(test_block(0x01, 10)).unwrap();
    immutable.append_block(test_block(0x02, 20)).unwrap();

    let mut volatile = InMemoryVolatile::default();
    volatile.add_block(test_block(0x03, 30)).unwrap();

    let mut ledger = InMemoryLedgerStore::default();
    ledger.save_snapshot(SlotNo(20), vec![1, 2, 3]).unwrap();

    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    // GC immutable blocks before slot 15 — should remove only slot 10
    let removed = chain_db.gc_immutable_before_slot(SlotNo(15)).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(chain_db.immutable().len(), 1);

    // Volatile and ledger stores are untouched
    assert!(chain_db.volatile().get_block(&HeaderHash([0x03; 32])).is_some());
    assert!(chain_db.ledger().latest_snapshot().is_some());
}

// ---------------------------------------------------------------------------
// get_block_by_slot — InMemory + File-backed
// ---------------------------------------------------------------------------

#[test]
fn immutable_get_block_by_slot_found() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 10)).unwrap();
    store.append_block(test_block(0x02, 20)).unwrap();

    let block = store.get_block_by_slot(SlotNo(20)).expect("should find slot 20");
    assert_eq!(block.header.hash, HeaderHash([0x02; 32]));
}

#[test]
fn immutable_get_block_by_slot_not_found() {
    let mut store = InMemoryImmutable::default();
    store.append_block(test_block(0x01, 10)).unwrap();
    assert!(store.get_block_by_slot(SlotNo(99)).is_none());
}

#[test]
fn file_immutable_get_block_by_slot() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mut store = FileImmutable::open(dir.path()).expect("open store");
    store.append_block(test_block(0x01, 10)).unwrap();
    store.append_block(test_block(0x02, 20)).unwrap();
    store.append_block(test_block(0x03, 30)).unwrap();

    let block = store.get_block_by_slot(SlotNo(20)).expect("should find slot 20");
    assert_eq!(block.header.hash, HeaderHash([0x02; 32]));

    assert!(store.get_block_by_slot(SlotNo(15)).is_none());
}

// ---------------------------------------------------------------------------
// Crash-tolerant open — corrupted files skipped
// ---------------------------------------------------------------------------

#[test]
fn file_immutable_open_skips_corrupted_files() {
    let dir = tempfile::tempdir().expect("create tempdir");
    {
        let mut store = FileImmutable::open(dir.path()).expect("open store");
        store.append_block(test_block(0x01, 10)).unwrap();
        store.append_block(test_block(0x02, 20)).unwrap();
    }

    // Write a corrupted .json file
    std::fs::write(dir.path().join("deadbeef.json"), b"NOT VALID JSON").unwrap();

    // Reopen — should skip the bad file and load the 2 good blocks
    let store = FileImmutable::open(dir.path()).expect("reopen should succeed");
    assert_eq!(store.len(), 2);
    assert_eq!(store.skipped_on_open(), 1);
}

#[test]
fn file_volatile_open_skips_corrupted_files() {
    let dir = tempfile::tempdir().expect("create tempdir");
    {
        let mut store = FileVolatile::open(dir.path()).expect("open store");
        store.add_block(test_block(0x01, 10)).unwrap();
    }

    // Write a corrupted .json file
    std::fs::write(dir.path().join("badbad.json"), b"{{{{").unwrap();

    let store = FileVolatile::open(dir.path()).expect("reopen should succeed");
    assert_eq!(store.tip(), Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])));
    assert_eq!(store.skipped_on_open(), 1);
}

#[test]
fn file_immutable_open_handles_empty_json_file() {
    let dir = tempfile::tempdir().expect("create tempdir");
    // Write an empty .json file (simulates truncated write)
    std::fs::write(dir.path().join("empty.json"), b"").unwrap();

    let store = FileImmutable::open(dir.path()).expect("open should succeed");
    assert_eq!(store.len(), 0);
    assert_eq!(store.skipped_on_open(), 1);
}
