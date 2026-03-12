use yggdrasil_ledger::{Block, BlockHeader, BlockNo, Era, HeaderHash, Point, SlotNo, Tx, TxId};
use yggdrasil_storage::{
    ImmutableStore, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile, LedgerStore,
    VolatileStore,
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
        }],
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
