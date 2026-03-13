use yggdrasil_ledger::{SlotNo, TxId};
use yggdrasil_mempool::{Mempool, MempoolEntry, MempoolError};

fn make_entry(id_byte: u8, fee: u64, size: usize) -> MempoolEntry {
    MempoolEntry {
        tx_id: TxId([id_byte; 32]),
        fee,
        body: vec![0u8; size],
        size_bytes: size,
        ttl: SlotNo(u64::MAX), // effectively no expiry
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

// ===========================================================================
// Phase 40: TTL-aware admission + expiry purge
// ===========================================================================

fn make_entry_with_ttl(id_byte: u8, fee: u64, size: usize, ttl: u64) -> MempoolEntry {
    MempoolEntry {
        tx_id: TxId([id_byte; 32]),
        fee,
        body: vec![0u8; size],
        size_bytes: size,
        ttl: SlotNo(ttl),
    }
}

#[test]
fn insert_checked_accepts_valid_ttl() {
    let mut mempool = Mempool::default();
    let entry = make_entry_with_ttl(0x01, 5, 100, 1000);
    mempool
        .insert_checked(entry, SlotNo(500))
        .expect("valid TTL should be accepted");
    assert_eq!(mempool.len(), 1);
}

#[test]
fn insert_checked_accepts_at_exact_ttl() {
    let mut mempool = Mempool::default();
    let entry = make_entry_with_ttl(0x01, 5, 100, 500);
    mempool
        .insert_checked(entry, SlotNo(500))
        .expect("TTL == current_slot should be accepted");
    assert_eq!(mempool.len(), 1);
}

#[test]
fn insert_checked_rejects_expired_ttl() {
    let mut mempool = Mempool::default();
    let entry = make_entry_with_ttl(0x01, 5, 100, 499);
    let err = mempool
        .insert_checked(entry, SlotNo(500))
        .expect_err("expired TTL");
    assert!(
        matches!(
            err,
            MempoolError::TtlExpired {
                ttl: SlotNo(499),
                current_slot: SlotNo(500)
            }
        ),
        "expected TtlExpired, got {err:?}"
    );
    assert_eq!(mempool.len(), 0);
}

#[test]
fn purge_expired_removes_stale_entries() {
    let mut mempool = Mempool::default();
    mempool
        .insert(make_entry_with_ttl(0x01, 5, 100, 100))
        .expect("insert 1");
    mempool
        .insert(make_entry_with_ttl(0x02, 3, 200, 500))
        .expect("insert 2");
    mempool
        .insert(make_entry_with_ttl(0x03, 1, 50, 200))
        .expect("insert 3");

    let removed = mempool.purge_expired(SlotNo(300));
    assert_eq!(removed, 2); // entries with TTL 100 and 200 are removed
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&TxId([0x02; 32])));
    assert_eq!(mempool.size_bytes(), 200);
}

#[test]
fn purge_expired_at_exact_ttl_keeps_entry() {
    let mut mempool = Mempool::default();
    mempool
        .insert(make_entry_with_ttl(0x01, 5, 100, 300))
        .expect("insert");
    let removed = mempool.purge_expired(SlotNo(300));
    assert_eq!(removed, 0);
    assert_eq!(mempool.len(), 1);
}

#[test]
fn purge_expired_removes_nothing_when_all_valid() {
    let mut mempool = Mempool::default();
    mempool
        .insert(make_entry_with_ttl(0x01, 5, 100, 1000))
        .expect("insert 1");
    mempool
        .insert(make_entry_with_ttl(0x02, 3, 200, 2000))
        .expect("insert 2");
    let removed = mempool.purge_expired(SlotNo(500));
    assert_eq!(removed, 0);
    assert_eq!(mempool.len(), 2);
}
