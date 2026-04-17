#![allow(clippy::unwrap_used)]
use yggdrasil_ledger::PlutusData;
use yggdrasil_ledger::eras::{ExUnits, Redeemer};
use yggdrasil_ledger::{
    AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, Era, MultiEraSubmittedTx,
    ProtocolParameters, ShelleyTx, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyWitnessSet,
    SlotNo, TxId, Value, min_fee_linear,
};
use yggdrasil_mempool::{
    MEMPOOL_ZERO_IDX, Mempool, MempoolEntry, MempoolError, MempoolRelayError, SharedMempool,
};

fn make_entry(id_byte: u8, fee: u64, size: usize) -> MempoolEntry {
    MempoolEntry {
        era: Era::Shelley,
        tx_id: TxId([id_byte; 32]),
        fee,
        body: vec![0u8; size],
        raw_tx: vec![0u8; size],
        size_bytes: size,
        ttl: SlotNo(u64::MAX), // effectively no expiry
        inputs: vec![],
    }
}

fn empty_witness_set() -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }
}

fn sample_shelley_submitted_tx(seed: u8) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Shelley(ShelleyTx {
        body: ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [seed; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 28],
                amount: 1_500_000,
            }],
            fee: 123_000,
            ttl: 5_000,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        witness_set: empty_witness_set(),
        auxiliary_data: Some(vec![0x81, seed]),
    })
}

fn sample_alonzo_submitted_tx(seed: u8) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        AlonzoTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [seed; 32],
                index: 1,
            }],
            outputs: vec![AlonzoTxOut {
                address: vec![0x61; 28],
                amount: Value::Coin(2_000_000),
                datum_hash: None,
            }],
            fee: 200_000,
            ttl: Some(9_999),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
        },
        empty_witness_set(),
        true,
        Some(vec![0x81, seed.wrapping_add(1)]),
    ))
}

fn sample_alonzo_submitted_tx_with_redeemers(
    seed: u8,
    fee: u64,
    redeemers: Vec<Redeemer>,
) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        AlonzoTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [seed; 32],
                index: 1,
            }],
            outputs: vec![AlonzoTxOut {
                address: vec![0x61; 28],
                amount: Value::Coin(2_000_000),
                datum_hash: None,
            }],
            fee,
            ttl: Some(9_999),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
        },
        ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers,
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        },
        true,
        Some(vec![0x81, 0x01]),
    ))
}

#[test]
fn mempool_prioritizes_higher_fees() {
    let mut mempool = Mempool::default();
    mempool
        .insert(make_entry(0x01, 1, 100))
        .expect("insert low");
    mempool
        .insert(make_entry(0x02, 10, 100))
        .expect("insert high");

    let best = mempool
        .pop_best()
        .expect("mempool should return the highest fee entry");
    assert_eq!(best.tx_id, TxId([0x02; 32]));
}

#[test]
fn mempool_rejects_duplicates() {
    let mut mempool = Mempool::default();
    mempool
        .insert(make_entry(0x01, 5, 100))
        .expect("first insert");
    let err = mempool
        .insert(make_entry(0x01, 5, 100))
        .expect_err("duplicate");
    assert!(matches!(err, MempoolError::Duplicate(_)));
    assert_eq!(mempool.len(), 1);
}

#[test]
fn mempool_enforces_capacity() {
    let mut mempool = Mempool::with_capacity(200);
    mempool
        .insert(make_entry(0x01, 5, 150))
        .expect("first insert");
    let err = mempool
        .insert(make_entry(0x02, 3, 100))
        .expect_err("over capacity");
    assert!(matches!(err, MempoolError::CapacityExceeded { .. }));
    assert_eq!(mempool.len(), 1);
}

#[test]
fn mempool_unlimited_capacity_when_zero() {
    let mut mempool = Mempool::default(); // max_bytes = 0
    mempool
        .insert(make_entry(0x01, 1, 10_000))
        .expect("insert 1");
    mempool
        .insert(make_entry(0x02, 2, 20_000))
        .expect("insert 2");
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
    mempool
        .insert(make_entry(0x01, 5, 150))
        .expect("first insert");
    assert!(mempool.remove_by_id(&TxId([0x01; 32])));
    // Now capacity is freed, a new 150-byte tx should fit.
    mempool
        .insert(make_entry(0x02, 3, 150))
        .expect("re-insert after removal");
    assert_eq!(mempool.len(), 1);
}

// ===========================================================================
// Phase 40: TTL-aware admission + expiry purge
// ===========================================================================

fn make_entry_with_ttl(id_byte: u8, fee: u64, size: usize, ttl: u64) -> MempoolEntry {
    MempoolEntry {
        era: Era::Shelley,
        tx_id: TxId([id_byte; 32]),
        fee,
        body: vec![0u8; size],
        raw_tx: vec![0u8; size],
        size_bytes: size,
        ttl: SlotNo(ttl),
        inputs: vec![],
    }
}

#[test]
fn mempool_entry_round_trips_shelley_submitted_tx() {
    let tx = sample_shelley_submitted_tx(0x10);
    let entry = MempoolEntry::from_multi_era_submitted_tx(tx.clone(), 999, SlotNo(5_000));

    assert_eq!(entry.era, Era::Shelley);
    assert_eq!(entry.tx_id, tx.tx_id());
    assert_eq!(entry.body, tx.body_cbor());
    assert_eq!(entry.raw_tx, tx.raw_cbor());
    assert_eq!(entry.size_bytes, entry.raw_tx.len());
    assert_eq!(entry.to_multi_era_submitted_tx().expect("decode entry"), tx);
}

#[test]
fn mempool_entry_round_trips_alonzo_submitted_tx() {
    let tx = sample_alonzo_submitted_tx(0x20);
    let entry = MempoolEntry::from_multi_era_submitted_tx(tx.clone(), 1_234, SlotNo(9_999));

    assert_eq!(entry.era, Era::Alonzo);
    assert_eq!(entry.tx_id, tx.tx_id());
    assert_eq!(entry.body, tx.body_cbor());
    assert_eq!(entry.raw_tx, tx.raw_cbor());
    assert_eq!(entry.size_bytes, entry.raw_tx.len());
    assert_eq!(entry.to_multi_era_submitted_tx().expect("decode entry"), tx);
}

#[test]
fn mempool_entry_rejects_txid_mismatch_when_decoding() {
    let tx = sample_shelley_submitted_tx(0x30);
    let mut entry = MempoolEntry::from_multi_era_submitted_tx(tx.clone(), 10, SlotNo(5_000));
    entry.tx_id = TxId([0xFF; 32]);

    let err = entry
        .to_multi_era_submitted_tx()
        .expect_err("mismatched txid should fail");
    assert!(matches!(
        err,
        MempoolRelayError::TxIdMismatch {
            expected,
            actual,
        } if expected == TxId([0xFF; 32]) && actual == tx.tx_id()
    ));
}

#[test]
fn insert_checked_accepts_valid_ttl() {
    let mut mempool = Mempool::default();
    let entry = make_entry_with_ttl(0x01, 5, 100, 1000);
    mempool
        .insert_checked(entry, SlotNo(500), None)
        .expect("valid TTL should be accepted");
    assert_eq!(mempool.len(), 1);
}

#[test]
fn insert_checked_accepts_at_exact_ttl() {
    let mut mempool = Mempool::default();
    let entry = make_entry_with_ttl(0x01, 5, 100, 500);
    mempool
        .insert_checked(entry, SlotNo(500), None)
        .expect("TTL == current_slot should be accepted");
    assert_eq!(mempool.len(), 1);
}

#[test]
fn insert_checked_rejects_expired_ttl() {
    let mut mempool = Mempool::default();
    let entry = make_entry_with_ttl(0x01, 5, 100, 499);
    let err = mempool
        .insert_checked(entry, SlotNo(500), None)
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

#[test]
fn insert_checked_rejects_below_min_fee_with_protocol_params() {
    let mut mempool = Mempool::default();
    let params = ProtocolParameters::default();
    let entry = make_entry_with_ttl(0x10, 1, 200, 1000);

    let err = mempool
        .insert_checked(entry, SlotNo(10), Some(&params))
        .expect_err("fee should be too small");
    assert!(matches!(err, MempoolError::FeeTooSmall { .. }));
}

#[test]
fn insert_checked_rejects_oversized_tx_with_protocol_params() {
    let mut mempool = Mempool::default();
    let params = ProtocolParameters::default();
    let entry = make_entry_with_ttl(0x11, 9_999_999, 20_000, 1000);

    let err = mempool
        .insert_checked(entry, SlotNo(10), Some(&params))
        .expect_err("tx size should exceed max");
    assert!(matches!(err, MempoolError::TxTooLarge { .. }));
}

#[test]
fn insert_checked_rejects_ex_units_above_protocol_limit_for_decodable_tx() {
    let mut mempool = Mempool::default();
    let mut params = ProtocolParameters::alonzo_defaults();
    params.max_tx_ex_units = Some(ExUnits { mem: 1, steps: 1 });

    let tx = sample_alonzo_submitted_tx_with_redeemers(
        0x70,
        5_000_000,
        vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(1),
            ex_units: ExUnits { mem: 10, steps: 10 },
        }],
    );
    let entry = MempoolEntry::from_multi_era_submitted_tx(tx, 5_000_000, SlotNo(20_000));
    let decoded = entry
        .to_multi_era_submitted_tx()
        .expect("entry should decode as submitted tx");
    assert_eq!(
        decoded.total_ex_units(),
        Some(ExUnits { mem: 10, steps: 10 })
    );

    let err = mempool
        .insert_checked(entry, SlotNo(10), Some(&params))
        .expect_err("ex units should exceed protocol tx limit");
    assert!(matches!(err, MempoolError::ExUnitsExceedTxLimit { .. }));
}

#[test]
fn insert_checked_uses_script_fee_when_redeemers_present() {
    let mut mempool = Mempool::default();
    let params = ProtocolParameters::alonzo_defaults();

    let tx = sample_alonzo_submitted_tx_with_redeemers(
        0x71,
        1,
        vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(2),
            ex_units: ExUnits {
                mem: 1_000_000,
                steps: 1_000_000_000,
            },
        }],
    );
    let entry = MempoolEntry::from_multi_era_submitted_tx(tx, 1, SlotNo(20_000));
    let decoded = entry
        .to_multi_era_submitted_tx()
        .expect("entry should decode as submitted tx");
    assert!(decoded.total_ex_units().is_some());
    let linear_min = min_fee_linear(&params, entry.body.len());

    let err = mempool
        .insert_checked(entry, SlotNo(10), Some(&params))
        .expect_err("fee should be too small once script fee is included");

    match err {
        MempoolError::FeeTooSmall { minimum, declared } => {
            assert_eq!(declared, 1);
            assert!(
                minimum > linear_min,
                "minimum fee should include script fee"
            );
        }
        other => panic!("expected FeeTooSmall, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Conflict detection tests (double-spend prevention)
// ---------------------------------------------------------------------------

/// Build a `MempoolEntry` with a specific set of UTxO inputs.
fn make_entry_with_inputs(
    id_byte: u8,
    fee: u64,
    size: usize,
    inputs: Vec<ShelleyTxIn>,
) -> MempoolEntry {
    MempoolEntry {
        era: Era::Shelley,
        tx_id: TxId([id_byte; 32]),
        fee,
        body: vec![0u8; size],
        raw_tx: vec![0u8; size],
        size_bytes: size,
        ttl: SlotNo(u64::MAX),
        inputs,
    }
}

fn sample_input(tx_seed: u8, index: u16) -> ShelleyTxIn {
    ShelleyTxIn {
        transaction_id: [tx_seed; 32],
        index,
    }
}

#[test]
fn conflict_detection_rejects_double_spend() {
    let input_a = sample_input(0xAA, 0);

    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input_a.clone()]);
    let entry2 = make_entry_with_inputs(0x02, 200, 50, vec![input_a.clone()]);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool.insert(entry1).expect("first tx should be accepted");

    let result = mempool.insert(entry2);
    assert!(
        matches!(result, Err(MempoolError::ConflictingInputs(_))),
        "expected ConflictingInputs error, got {:?}",
        result
    );
    assert_eq!(
        mempool.len(),
        1,
        "mempool should still have only the first tx"
    );
}

#[test]
fn conflict_detection_allows_disjoint_inputs() {
    let input_a = sample_input(0xAA, 0);
    let input_b = sample_input(0xBB, 0);

    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input_a]);
    let entry2 = make_entry_with_inputs(0x02, 200, 50, vec![input_b]);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool.insert(entry1).expect("first tx");
    mempool
        .insert(entry2)
        .expect("second tx with different input should be accepted");
    assert_eq!(mempool.len(), 2);
}

#[test]
fn conflict_detection_partial_overlap_rejected() {
    let input_a = sample_input(0xAA, 0);
    let input_b = sample_input(0xBB, 0);

    // entry1 consumes [A, B]; entry2 consumes [B, C] — overlap on B.
    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input_a.clone(), input_b.clone()]);
    let entry2 = make_entry_with_inputs(0x02, 200, 50, vec![input_b, sample_input(0xCC, 0)]);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool.insert(entry1).expect("first tx");
    let result = mempool.insert(entry2);
    assert!(matches!(result, Err(MempoolError::ConflictingInputs(_))));
}

#[test]
fn conflict_detection_claims_released_on_removal() {
    let input_a = sample_input(0xAA, 0);

    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input_a.clone()]);
    let entry2 = make_entry_with_inputs(0x02, 200, 50, vec![input_a.clone()]);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool.insert(entry1.clone()).expect("insert entry1");

    // Remove entry1 — its input claim should be released.
    assert!(mempool.remove_by_id(&entry1.tx_id));

    // Now entry2 (which spends the same input) should be admitted.
    mempool
        .insert(entry2)
        .expect("entry2 should be accepted after entry1 removed");
    assert_eq!(mempool.len(), 1);
}

#[test]
fn conflict_detection_claims_released_on_pop_best() {
    let input_a = sample_input(0xAA, 0);

    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input_a.clone()]);
    let entry2 = make_entry_with_inputs(0x02, 50, 50, vec![input_a.clone()]);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool.insert(entry1).expect("insert entry1 (higher fee)");

    let popped = mempool.pop_best().expect("pop highest fee entry");
    assert_eq!(popped.fee, 100);

    // After popping, the input claim should be gone; entry2 should be admitted.
    mempool.insert(entry2).expect("entry2 admitted after pop");
    assert_eq!(mempool.len(), 1);
}

#[test]
fn conflict_detection_claims_released_on_confirmed() {
    let input_a = sample_input(0xAA, 0);

    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input_a.clone()]);
    let entry2 = make_entry_with_inputs(0x02, 50, 50, vec![input_a.clone()]);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool.insert(entry1.clone()).expect("insert entry1");

    let removed = mempool.remove_confirmed(&[entry1.tx_id]);
    assert_eq!(removed, 1);

    // Input claim released — entry2 should now be admitted.
    mempool
        .insert(entry2)
        .expect("entry2 admitted after confirmation");
    assert_eq!(mempool.len(), 1);
}

// ===========================================================================
// SharedMempool tests
// ===========================================================================

#[test]
fn shared_mempool_new_wraps_existing() {
    let mut inner = Mempool::with_capacity(5000);
    inner.insert(make_entry(0x01, 10, 100)).expect("insert 1");
    inner.insert(make_entry(0x02, 20, 200)).expect("insert 2");

    let shared = SharedMempool::new(inner);
    assert_eq!(shared.len(), 2);
    assert!(!shared.is_empty());
    assert_eq!(shared.size_bytes(), 300);
    assert_eq!(shared.capacity(), 5000);
}

#[test]
fn shared_mempool_insert_and_pop_best() {
    let shared = SharedMempool::with_capacity(0);
    shared
        .insert(make_entry(0x01, 5, 100))
        .expect("insert low fee");
    shared
        .insert(make_entry(0x02, 50, 100))
        .expect("insert high fee");
    assert_eq!(shared.len(), 2);

    let best = shared.pop_best().expect("should return highest-fee entry");
    assert_eq!(best.tx_id, TxId([0x02; 32]));
    assert_eq!(best.fee, 50);
    assert_eq!(shared.len(), 1);
}

#[test]
fn shared_mempool_remove_by_id() {
    let shared = SharedMempool::with_capacity(0);
    shared.insert(make_entry(0x01, 10, 100)).expect("insert");
    assert!(shared.contains(&TxId([0x01; 32])));

    assert!(shared.remove_by_id(&TxId([0x01; 32])));
    assert!(!shared.contains(&TxId([0x01; 32])));
    assert!(shared.is_empty());

    // Removing non-existent id returns false.
    assert!(!shared.remove_by_id(&TxId([0xFF; 32])));
}

#[test]
fn shared_mempool_remove_confirmed() {
    let shared = SharedMempool::with_capacity(0);
    shared.insert(make_entry(0x01, 10, 100)).expect("insert 1");
    shared.insert(make_entry(0x02, 20, 200)).expect("insert 2");
    shared.insert(make_entry(0x03, 30, 300)).expect("insert 3");

    let confirmed = [TxId([0x01; 32]), TxId([0x03; 32])];
    let removed = shared.remove_confirmed(&confirmed);
    assert_eq!(removed, 2);
    assert_eq!(shared.len(), 1);
    assert!(shared.contains(&TxId([0x02; 32])));
}

#[test]
fn shared_mempool_contains() {
    let shared = SharedMempool::with_capacity(0);
    shared.insert(make_entry(0x01, 10, 100)).expect("insert");

    assert!(shared.contains(&TxId([0x01; 32])));
    assert!(!shared.contains(&TxId([0x99; 32])));
}

#[test]
fn shared_mempool_purge_expired() {
    let shared = SharedMempool::with_capacity(0);
    shared
        .insert(make_entry_with_ttl(0x01, 5, 100, 100))
        .expect("insert ttl=100");
    shared
        .insert(make_entry_with_ttl(0x02, 3, 200, 500))
        .expect("insert ttl=500");
    shared
        .insert(make_entry_with_ttl(0x03, 1, 50, 200))
        .expect("insert ttl=200");

    let removed = shared.purge_expired(SlotNo(300));
    assert_eq!(removed, 2); // TTL 100 and 200 expired
    assert_eq!(shared.len(), 1);
    assert!(shared.contains(&TxId([0x02; 32])));
    assert_eq!(shared.size_bytes(), 200);
}

#[test]
fn shared_mempool_insert_checked_ttl_expired() {
    let shared = SharedMempool::with_capacity(0);
    let entry = make_entry_with_ttl(0x01, 10, 100, 499);

    let err = shared
        .insert_checked(entry, SlotNo(500), None)
        .expect_err("expired TTL should be rejected");
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
    assert!(shared.is_empty());
}

#[test]
fn shared_mempool_insert_checked_accepts_valid() {
    let shared = SharedMempool::with_capacity(0);
    let entry = make_entry_with_ttl(0x01, 10, 100, 1000);

    shared
        .insert_checked(entry, SlotNo(500), None)
        .expect("valid TTL should be accepted");
    assert_eq!(shared.len(), 1);
    assert!(shared.contains(&TxId([0x01; 32])));
}

#[test]
fn shared_mempool_insert_checked_double_spend_through_checked_path() {
    let input = sample_input(0xDD, 0);

    let entry1 = make_entry_with_inputs(0x01, 100, 50, vec![input.clone()]);
    let entry2 = make_entry_with_inputs(0x02, 200, 50, vec![input]);
    // Give both entries a valid TTL.
    let entry1 = MempoolEntry {
        ttl: SlotNo(9999),
        ..entry1
    };
    let entry2 = MempoolEntry {
        ttl: SlotNo(9999),
        ..entry2
    };

    let shared = SharedMempool::with_capacity(1_000_000);
    shared
        .insert_checked(entry1, SlotNo(100), None)
        .expect("first tx via checked path");

    let result = shared.insert_checked(entry2, SlotNo(100), None);
    assert!(
        matches!(result, Err(MempoolError::ConflictingInputs(_))),
        "expected ConflictingInputs, got {result:?}"
    );
    assert_eq!(shared.len(), 1);
}

#[test]
fn shared_mempool_purge_expired_releases_input_claims() {
    let input = sample_input(0xEE, 0);

    let entry1 = MempoolEntry {
        ttl: SlotNo(100),
        ..make_entry_with_inputs(0x01, 50, 80, vec![input.clone()])
    };
    let entry2 = MempoolEntry {
        ttl: SlotNo(u64::MAX),
        ..make_entry_with_inputs(0x02, 70, 80, vec![input])
    };

    let shared = SharedMempool::with_capacity(1_000_000);
    shared.insert(entry1).expect("insert entry1");

    // entry2 conflicts while entry1 is alive.
    assert!(shared.insert(entry2.clone()).is_err());

    // Purge entry1 (TTL 100 < current_slot 200).
    let purged = shared.purge_expired(SlotNo(200));
    assert_eq!(purged, 1);

    // Input claim released — entry2 should now be admitted.
    shared
        .insert(entry2)
        .expect("entry2 accepted after purge released claims");
    assert_eq!(shared.len(), 1);
}

#[test]
fn shared_mempool_snapshot_after_purge_is_empty() {
    let shared = SharedMempool::with_capacity(0);
    shared
        .insert(make_entry_with_ttl(0x01, 5, 100, 50))
        .expect("insert ttl=50");
    shared
        .insert(make_entry_with_ttl(0x02, 3, 200, 60))
        .expect("insert ttl=60");

    // Purge everything (current_slot > all TTLs).
    let purged = shared.purge_expired(SlotNo(1000));
    assert_eq!(purged, 2);
    assert!(shared.is_empty());

    // Snapshot should reflect the empty state.
    let snap = shared.snapshot();
    let entries = snap.mempool_txids_after(MEMPOOL_ZERO_IDX);
    assert!(
        entries.is_empty(),
        "snapshot should be empty after full purge"
    );
}

#[test]
fn shared_mempool_capacity_returns_configured_max() {
    let shared_unlimited = SharedMempool::with_capacity(0);
    assert_eq!(shared_unlimited.capacity(), 0);

    let shared_limited = SharedMempool::with_capacity(4096);
    assert_eq!(shared_limited.capacity(), 4096);

    // Wrap an inner mempool with specific capacity.
    let inner = Mempool::with_capacity(8192);
    let shared_wrapped = SharedMempool::new(inner);
    assert_eq!(shared_wrapped.capacity(), 8192);
}

// ===========================================================================
// Phase: Mempool epoch revalidation — purge_invalid_for_params
// Reference: Ouroboros.Consensus.Mempool.Impl.Update — syncWithLedger /
//            revalidateTx epoch reconciliation pass.
// ===========================================================================

/// Returns a `ProtocolParameters` instance with a very high `min_fee_a` so
/// that entries with a low declared fee will fail the fee check.
fn params_with_high_min_fee() -> ProtocolParameters {
    // min_fee_a is the coefficient; set it very high so even 500-byte txs
    // require a huge fee.
    ProtocolParameters {
        min_fee_a: 1_000_000, // lovelace per byte
        ..ProtocolParameters::default()
    }
}

/// Returns a `ProtocolParameters` instance with a very small `max_tx_size`
/// so that all but the tiniest entries will fail the size check.
fn params_with_small_max_tx_size() -> ProtocolParameters {
    ProtocolParameters {
        max_tx_size: 50, // bytes
        ..ProtocolParameters::default()
    }
}

#[test]
fn purge_invalid_for_params_evicts_underfunded_txs() {
    let mut mempool = Mempool::default();
    // Two entries with low fees.
    mempool
        .insert(make_entry_with_ttl(0x01, 1, 100, 9_999_999))
        .expect("insert low fee 1");
    mempool
        .insert(make_entry_with_ttl(0x02, 2, 100, 9_999_999))
        .expect("insert low fee 2");
    // One entry with a laughably high fee that will survive any fee floor.
    mempool
        .insert(make_entry_with_ttl(0x03, u64::MAX / 2, 50, 9_999_999))
        .expect("insert very high fee");
    assert_eq!(mempool.len(), 3);

    let params = params_with_high_min_fee();
    // Use slot 0 — no TTL expiry; only fee / size checks trigger.
    let removed = mempool.purge_invalid_for_params(SlotNo(0), &params);

    // Both low-fee entries should be evicted; the high-fee one survives.
    assert_eq!(removed, 2, "both low-fee entries should be purged");
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&TxId([0x03; 32])));
    assert_eq!(mempool.size_bytes(), 50);
}

#[test]
fn purge_invalid_for_params_evicts_oversized_txs() {
    let mut mempool = Mempool::default();
    // Large entries that will violate a 50-byte max_tx_size.
    mempool
        .insert(make_entry_with_ttl(0x01, 9_999_999, 200, 9_999_999))
        .expect("insert large 1");
    mempool
        .insert(make_entry_with_ttl(0x02, 9_999_999, 300, 9_999_999))
        .expect("insert large 2");
    // Small entry that will pass the size check.
    mempool
        .insert(make_entry_with_ttl(0x03, 9_999_999, 10, 9_999_999))
        .expect("insert small");
    assert_eq!(mempool.len(), 3);

    let params = params_with_small_max_tx_size();
    let removed = mempool.purge_invalid_for_params(SlotNo(0), &params);

    assert_eq!(removed, 2, "the two oversized entries should be purged");
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&TxId([0x03; 32])));
}

#[test]
fn purge_invalid_for_params_keeps_valid_txs() {
    let params = ProtocolParameters::default();
    // Compute a fee that will satisfy the default fee formula for a 100-byte tx.
    use yggdrasil_ledger::min_fee_linear;
    let min_fee = min_fee_linear(&params, 100) + 1_000;

    let mut mempool = Mempool::default();
    mempool
        .insert(make_entry_with_ttl(0x01, min_fee, 100, 9_999_999))
        .expect("insert");
    mempool
        .insert(make_entry_with_ttl(0x02, min_fee, 100, 9_999_999))
        .expect("insert");
    assert_eq!(mempool.len(), 2);

    let removed = mempool.purge_invalid_for_params(SlotNo(0), &params);

    assert_eq!(removed, 0, "valid entries should not be evicted");
    assert_eq!(mempool.len(), 2);
}

#[test]
fn purge_invalid_for_params_also_evicts_expired_ttl() {
    let params = ProtocolParameters::default();
    use yggdrasil_ledger::min_fee_linear;
    let min_fee = min_fee_linear(&params, 100) + 1_000;

    let mut mempool = Mempool::default();
    // Expired TTL — should be removed even though the fee would otherwise be fine.
    mempool
        .insert(make_entry_with_ttl(0x01, min_fee, 100, 50))
        .expect("insert expired");
    // Valid TTL — should survive.
    mempool
        .insert(make_entry_with_ttl(0x02, min_fee, 100, 9_999_999))
        .expect("insert valid");
    assert_eq!(mempool.len(), 2);

    let removed = mempool.purge_invalid_for_params(SlotNo(100), &params);

    assert_eq!(removed, 1, "expired-TTL entry should be evicted");
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&TxId([0x02; 32])));
}

#[test]
fn shared_mempool_purge_invalid_for_params() {
    let shared = SharedMempool::with_capacity(0);
    // Low fee — will fail the min-fee check under high-fee params.
    shared
        .insert(make_entry_with_ttl(0x01, 1, 100, 9_999_999))
        .expect("insert low fee");
    // Very high fee — will survive.
    shared
        .insert(make_entry_with_ttl(0x02, u64::MAX / 2, 50, 9_999_999))
        .expect("insert high fee");
    assert_eq!(shared.len(), 2);

    let params = params_with_high_min_fee();
    let removed = shared.purge_invalid_for_params(SlotNo(0), &params);

    assert_eq!(removed, 1);
    assert_eq!(shared.len(), 1);
    assert!(shared.contains(&TxId([0x02; 32])));
}
