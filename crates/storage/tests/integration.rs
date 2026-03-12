use yggdrasil_storage::{ImmutableBlockStore, LedgerSnapshotStore, VolatileBlockStore};

#[test]
fn storage_stores_and_rolls_back_blocks() {
    let mut immutable = ImmutableBlockStore::default();
    immutable.append(String::from("block-1"));
    assert_eq!(immutable.len(), 1);

    let mut volatile = VolatileBlockStore::default();
    volatile.insert(String::from("block-2"));
    volatile.rollback(0);

    let mut snapshots = LedgerSnapshotStore::default();
    snapshots.persist(String::from("snapshot-1"));
    assert_eq!(snapshots.count(), 1);
}
