use yggdrasil_ledger::TxId;
use yggdrasil_mempool::{Mempool, MempoolEntry, MempoolError};

fn make_entry(id_byte: u8, fee: u64, size: usize) -> MempoolEntry {
    MempoolEntry {
        tx_id: TxId([id_byte; 32]),
        fee,
        body: vec![0u8; size],
        size_bytes: size,
    }
}

#[test]
fn mempool_prioritizes_higher_fees() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 1, 100)).expect("insert low");
    mempool.insert(make_entry(0x02, 10, 100)).expect("insert high");

    let best = mempool.pop_best().expect("mempool should return the highest fee entry");
    assert_eq!(best.tx_id, TxId([0x02; 32]));
}

#[test]
fn mempool_rejects_duplicates() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 5, 100)).expect("first insert");
    let err = mempool.insert(make_entry(0x01, 5, 100)).expect_err("duplicate");
    assert!(matches!(err, MempoolError::Duplicate(_)));
    assert_eq!(mempool.len(), 1);
}

#[test]
fn mempool_enforces_capacity() {
    let mut mempool = Mempool::with_capacity(200);
    mempool.insert(make_entry(0x01, 5, 150)).expect("first insert");
    let err = mempool.insert(make_entry(0x02, 3, 100)).expect_err("over capacity");
    assert!(matches!(err, MempoolError::CapacityExceeded { .. }));
    assert_eq!(mempool.len(), 1);
}

#[test]
fn mempool_unlimited_capacity_when_zero() {
    let mut mempool = Mempool::default(); // max_bytes = 0
    mempool.insert(make_entry(0x01, 1, 10_000)).expect("insert 1");
    mempool.insert(make_entry(0x02, 2, 20_000)).expect("insert 2");
    assert_eq!(mempool.len(), 2);
    assert_eq!(mempool.size_bytes(), 30_000);
}

#[test]
fn remove_by_id_removes_entry_and_updates_size() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 5, 100)).expect("insert 1");
    mempool.insert(make_entry(0x02, 3, 200)).expect("insert 2");
    assert_eq!(mempool.size_bytes(), 300);

    assert!(mempool.remove_by_id(&TxId([0x01; 32])));
    assert_eq!(mempool.len(), 1);
    assert_eq!(mempool.size_bytes(), 200);

    // Removing non-existent id returns false.
    assert!(!mempool.remove_by_id(&TxId([0xFF; 32])));
}

#[test]
fn contains_reports_presence() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 5, 100)).expect("insert");
    assert!(mempool.contains(&TxId([0x01; 32])));
    assert!(!mempool.contains(&TxId([0x99; 32])));
}

#[test]
fn pop_best_returns_none_on_empty() {
    let mut mempool = Mempool::default();
    assert!(mempool.pop_best().is_none());
    assert!(mempool.is_empty());
}

#[test]
fn pop_best_updates_size() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 5, 100)).expect("insert");
    assert_eq!(mempool.size_bytes(), 100);
    let _ = mempool.pop_best();
    assert_eq!(mempool.size_bytes(), 0);
}

#[test]
fn remove_confirmed_evicts_matching_entries() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 5, 100)).expect("insert 1");
    mempool.insert(make_entry(0x02, 3, 200)).expect("insert 2");
    mempool.insert(make_entry(0x03, 1, 50)).expect("insert 3");

    let confirmed = vec![TxId([0x01; 32]), TxId([0x03; 32])];
    let removed = mempool.remove_confirmed(&confirmed);
    assert_eq!(removed, 2);
    assert_eq!(mempool.len(), 1);
    assert_eq!(mempool.size_bytes(), 200);
    assert!(mempool.contains(&TxId([0x02; 32])));
}

#[test]
fn iter_yields_entries_in_fee_order() {
    let mut mempool = Mempool::default();
    mempool.insert(make_entry(0x01, 1, 50)).expect("insert 1");
    mempool.insert(make_entry(0x02, 10, 50)).expect("insert 2");
    mempool.insert(make_entry(0x03, 5, 50)).expect("insert 3");

    let fees: Vec<u64> = mempool.iter().map(|e| e.fee).collect();
    assert_eq!(fees, vec![10, 5, 1]);
}

#[test]
fn insert_after_removal_frees_capacity() {
    let mut mempool = Mempool::with_capacity(200);
    mempool.insert(make_entry(0x01, 5, 150)).expect("first insert");
    assert!(mempool.remove_by_id(&TxId([0x01; 32])));
    // Now capacity is freed, a new 150-byte tx should fit.
    mempool.insert(make_entry(0x02, 3, 150)).expect("re-insert after removal");
    assert_eq!(mempool.len(), 1);
}
