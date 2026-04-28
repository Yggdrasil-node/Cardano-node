//! Integration tests for block-level ExUnits validation (BBODY rule).
//!
//! Upstream reference: `Cardano.Ledger.Alonzo.Rules.Bbody` —
//!   `totalExUnits(txs) <= maxBlockExUnits(pp)`.
//!
//! Tests cover Alonzo, Babbage, and Conway eras.

use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn enterprise_addr(keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x61]; // enterprise keyhash, network 1
    addr.extend_from_slice(keyhash);
    addr
}

fn make_witness_set_with_redeemer(mem: u64, steps: u64) -> Vec<u8> {
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(0),
            ex_units: ExUnits { mem, steps },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let mut enc = Encoder::new();
    ws.encode_cbor(&mut enc);
    enc.into_bytes()
}

fn make_alonzo_tx(
    input_hash: [u8; 32],
    input_idx: u16,
    output_addr: &[u8],
    output_coin: u64,
    fee: u64,
    witness_mem: u64,
    witness_steps: u64,
) -> yggdrasil_ledger::Tx {
    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: input_idx,
        }],
        outputs: vec![AlonzoTxOut {
            address: output_addr.to_vec(),
            amount: Value::Coin(output_coin),
            datum_hash: None,
        }],
        fee,
        ttl: Some(100_000),
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
    };
    let mut enc = Encoder::new();
    body.encode_cbor(&mut enc);
    let body_bytes = enc.into_bytes();
    let id = compute_tx_id(&body_bytes);
    let wb = if witness_mem > 0 || witness_steps > 0 {
        Some(make_witness_set_with_redeemer(witness_mem, witness_steps))
    } else {
        None
    };
    yggdrasil_ledger::Tx {
        id,
        body: body_bytes,
        witnesses: wb,
        auxiliary_data: None,
        is_valid: None,
    }
}

fn make_alonzo_block_helper(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<yggdrasil_ledger::Tx>,
) -> Block {
    Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn make_babbage_tx(
    input_hash: [u8; 32],
    input_idx: u16,
    output_addr: &[u8],
    output_coin: u64,
    fee: u64,
    witness_mem: u64,
    witness_steps: u64,
) -> yggdrasil_ledger::Tx {
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: input_idx,
        }],
        outputs: vec![BabbageTxOut {
            address: output_addr.to_vec(),
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(100_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        script_data_hash: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };
    let mut enc = Encoder::new();
    body.encode_cbor(&mut enc);
    let body_bytes = enc.into_bytes();
    let id = compute_tx_id(&body_bytes);
    let wb = if witness_mem > 0 || witness_steps > 0 {
        Some(make_witness_set_with_redeemer(witness_mem, witness_steps))
    } else {
        None
    };
    yggdrasil_ledger::Tx {
        id,
        body: body_bytes,
        witnesses: wb,
        auxiliary_data: None,
        is_valid: None,
    }
}

fn make_babbage_block_helper(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<yggdrasil_ledger::Tx>,
) -> Block {
    Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn make_conway_tx(
    input_hash: [u8; 32],
    input_idx: u16,
    output_addr: &[u8],
    output_coin: u64,
    fee: u64,
    witness_mem: u64,
    witness_steps: u64,
) -> yggdrasil_ledger::Tx {
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: input_idx,
        }],
        outputs: vec![BabbageTxOut {
            address: output_addr.to_vec(),
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(100_000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        script_data_hash: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };
    let mut enc = Encoder::new();
    body.encode_cbor(&mut enc);
    let body_bytes = enc.into_bytes();
    let id = compute_tx_id(&body_bytes);
    let wb = if witness_mem > 0 || witness_steps > 0 {
        Some(make_witness_set_with_redeemer(witness_mem, witness_steps))
    } else {
        None
    };
    yggdrasil_ledger::Tx {
        id,
        body: body_bytes,
        witnesses: wb,
        auxiliary_data: None,
        is_valid: None,
    }
}

fn make_conway_block_helper(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<yggdrasil_ledger::Tx>,
) -> Block {
    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn params_with_block_ex_units(max_mem: u64, max_steps: u64) -> ProtocolParameters {
    let mut p = ProtocolParameters::alonzo_defaults();
    p.max_block_ex_units = Some(ExUnits {
        mem: max_mem,
        steps: max_steps,
    });
    p.min_fee_a = 0;
    p.min_fee_b = 0;
    // Zero out execution-unit prices so script fees don't interfere.
    p.price_mem = Some(UnitInterval {
        numerator: 0,
        denominator: 1,
    });
    p.price_step = Some(UnitInterval {
        numerator: 0,
        denominator: 1,
    });
    // Disable min-utxo enforcement.
    p.coins_per_utxo_byte = Some(0);
    p.min_utxo_value = None;
    p
}

fn seed_utxo_alonzo(
    state: &mut LedgerState,
    tx_hash: [u8; 32],
    index: u16,
    addr: &[u8],
    coin: u64,
) {
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: tx_hash,
            index,
        },
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(coin),
            datum_hash: None,
        }),
    );
}

fn seed_utxo_babbage(
    state: &mut LedgerState,
    tx_hash: [u8; 32],
    index: u16,
    addr: &[u8],
    coin: u64,
) {
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: tx_hash,
            index,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(coin),
            datum_option: None,
            script_ref: None,
        }),
    );
}

// ---------------------------------------------------------------------------
// Alonzo block-level ExUnits
// ---------------------------------------------------------------------------

/// Block with no redeemers (ExUnits = 0) always accepted even with tiny limit.
#[test]
fn alonzo_block_zero_ex_units_accepted() {
    let addr = enterprise_addr(&[0xAA; 28]);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(params_with_block_ex_units(1, 1));
    seed_utxo_alonzo(&mut state, [0x01; 32], 0, &addr, 5_000_000);

    // No redeemers → block ExUnits = 0
    let tx = make_alonzo_tx([0x01; 32], 0, &addr, 5_000_000, 0, 0, 0);
    let block = make_alonzo_block_helper(10, 1, 0xA1, vec![tx]);
    assert!(state.apply_block(&block).is_ok());
}

#[test]
fn alonzo_block_ex_units_exceed_mem_rejected() {
    let addr = enterprise_addr(&[0xBB; 28]);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(params_with_block_ex_units(10_000, 100_000));
    seed_utxo_alonzo(&mut state, [0x02; 32], 0, &addr, 5_000_000);
    seed_utxo_alonzo(&mut state, [0x03; 32], 0, &addr, 5_000_000);

    // Two transactions: 6000 + 6000 = 12000 mem > 10000 limit
    let tx1 = make_alonzo_tx([0x02; 32], 0, &addr, 5_000_000, 0, 6_000, 10_000);
    let tx2 = make_alonzo_tx([0x03; 32], 0, &addr, 5_000_000, 0, 6_000, 10_000);
    let block = make_alonzo_block_helper(10, 1, 0xA2, vec![tx1, tx2]);
    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::BlockExUnitsExceeded {
            block_mem, max_mem, ..
        } => {
            assert_eq!(block_mem, 12_000);
            assert_eq!(max_mem, 10_000);
        }
        other => panic!("expected BlockExUnitsExceeded, got: {other}"),
    }
}

#[test]
fn alonzo_block_ex_units_exceed_steps_rejected() {
    let addr = enterprise_addr(&[0xCC; 28]);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(params_with_block_ex_units(100_000, 10_000));
    seed_utxo_alonzo(&mut state, [0x04; 32], 0, &addr, 5_000_000);
    seed_utxo_alonzo(&mut state, [0x05; 32], 0, &addr, 5_000_000);

    // Two transactions: 8000 + 8000 = 16000 steps > 10000 limit
    let tx1 = make_alonzo_tx([0x04; 32], 0, &addr, 5_000_000, 0, 1_000, 8_000);
    let tx2 = make_alonzo_tx([0x05; 32], 0, &addr, 5_000_000, 0, 1_000, 8_000);
    let block = make_alonzo_block_helper(10, 1, 0xA3, vec![tx1, tx2]);
    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::BlockExUnitsExceeded {
            block_steps,
            max_steps,
            ..
        } => {
            assert_eq!(block_steps, 16_000);
            assert_eq!(max_steps, 10_000);
        }
        other => panic!("expected BlockExUnitsExceeded, got: {other}"),
    }
}

#[test]
fn alonzo_block_no_redeemers_skips_check() {
    let addr = enterprise_addr(&[0xDD; 28]);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(params_with_block_ex_units(100, 100));
    seed_utxo_alonzo(&mut state, [0x06; 32], 0, &addr, 5_000_000);

    // Tx with no witness bytes — block ExUnits sum is 0
    let tx = make_alonzo_tx([0x06; 32], 0, &addr, 5_000_000, 0, 0, 0);
    let block = make_alonzo_block_helper(10, 1, 0xA4, vec![tx]);
    assert!(state.apply_block(&block).is_ok());
}

// ---------------------------------------------------------------------------
// Babbage block-level ExUnits
// ---------------------------------------------------------------------------

/// Block with no redeemers (ExUnits = 0) always accepted even with tiny limit.
#[test]
fn babbage_block_zero_ex_units_accepted() {
    let addr = enterprise_addr(&[0xEE; 28]);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(params_with_block_ex_units(1, 1));
    seed_utxo_babbage(&mut state, [0x07; 32], 0, &addr, 5_000_000);

    let tx = make_babbage_tx([0x07; 32], 0, &addr, 5_000_000, 0, 0, 0);
    let block = make_babbage_block_helper(10, 1, 0xB1, vec![tx]);
    assert!(state.apply_block(&block).is_ok());
}

#[test]
fn babbage_block_ex_units_exceed_rejected() {
    let addr = enterprise_addr(&[0xFF; 28]);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(params_with_block_ex_units(10_000, 10_000));
    seed_utxo_babbage(&mut state, [0x08; 32], 0, &addr, 5_000_000);
    seed_utxo_babbage(&mut state, [0x09; 32], 0, &addr, 5_000_000);

    let tx1 = make_babbage_tx([0x08; 32], 0, &addr, 5_000_000, 0, 7_000, 7_000);
    let tx2 = make_babbage_tx([0x09; 32], 0, &addr, 5_000_000, 0, 7_000, 7_000);
    let block = make_babbage_block_helper(10, 1, 0xB2, vec![tx1, tx2]);
    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::BlockExUnitsExceeded {
            block_mem,
            block_steps,
            max_mem,
            max_steps,
        } => {
            assert_eq!(block_mem, 14_000);
            assert_eq!(block_steps, 14_000);
            assert_eq!(max_mem, 10_000);
            assert_eq!(max_steps, 10_000);
        }
        other => panic!("expected BlockExUnitsExceeded, got: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Conway block-level ExUnits
// ---------------------------------------------------------------------------

/// Block with no redeemers (ExUnits = 0) always accepted even with tiny limit.
#[test]
fn conway_block_zero_ex_units_accepted() {
    let addr = enterprise_addr(&[0x11; 28]);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(params_with_block_ex_units(1, 1));
    seed_utxo_babbage(&mut state, [0x0A; 32], 0, &addr, 5_000_000);

    let tx = make_conway_tx([0x0A; 32], 0, &addr, 5_000_000, 0, 0, 0);
    let block = make_conway_block_helper(10, 1, 0xC1, vec![tx]);
    assert!(state.apply_block(&block).is_ok());
}

#[test]
fn conway_block_ex_units_exceed_rejected() {
    let addr = enterprise_addr(&[0x22; 28]);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(params_with_block_ex_units(10_000, 10_000));
    seed_utxo_babbage(&mut state, [0x0B; 32], 0, &addr, 5_000_000);
    seed_utxo_babbage(&mut state, [0x0C; 32], 0, &addr, 5_000_000);

    let tx1 = make_conway_tx([0x0B; 32], 0, &addr, 5_000_000, 0, 6_000, 6_000);
    let tx2 = make_conway_tx([0x0C; 32], 0, &addr, 5_000_000, 0, 6_000, 6_000);
    let block = make_conway_block_helper(10, 1, 0xC2, vec![tx1, tx2]);
    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::BlockExUnitsExceeded {
            block_mem,
            block_steps,
            max_mem,
            max_steps,
        } => {
            assert_eq!(block_mem, 12_000);
            assert_eq!(block_steps, 12_000);
            assert_eq!(max_mem, 10_000);
            assert_eq!(max_steps, 10_000);
        }
        other => panic!("expected BlockExUnitsExceeded, got: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

/// Block ExUnits exactly at limit (mem or steps exceeds by 1) is rejected.
#[test]
fn block_ex_units_one_over_limit_rejected() {
    let addr = enterprise_addr(&[0x33; 28]);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(params_with_block_ex_units(10_000, 10_000));
    seed_utxo_alonzo(&mut state, [0x0D; 32], 0, &addr, 5_000_000);
    seed_utxo_alonzo(&mut state, [0x0E; 32], 0, &addr, 5_000_000);

    // 5001 + 5000 = 10001 mem > 10000 limit
    let tx1 = make_alonzo_tx([0x0D; 32], 0, &addr, 5_000_000, 0, 5_001, 5_000);
    let tx2 = make_alonzo_tx([0x0E; 32], 0, &addr, 5_000_000, 0, 5_000, 5_000);
    let block = make_alonzo_block_helper(10, 1, 0xE1, vec![tx1, tx2]);
    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::BlockExUnitsExceeded {
            block_mem, max_mem, ..
        } => {
            assert_eq!(block_mem, 10_001);
            assert_eq!(max_mem, 10_000);
        }
        other => panic!("expected BlockExUnitsExceeded, got: {other}"),
    }
}

#[test]
/// When protocol_params is not set, the block ExUnits check is skipped
/// (no BlockExUnitsExceeded), even with huge redeemers. Other validation
/// may still reject the block.
fn block_ex_units_no_params_skips_check() {
    let addr = enterprise_addr(&[0x44; 28]);
    let mut state = LedgerState::new(Era::Alonzo);
    // No protocol_params set → block ExUnits check skipped
    seed_utxo_alonzo(&mut state, [0x0F; 32], 0, &addr, 5_000_000);

    let tx = make_alonzo_tx([0x0F; 32], 0, &addr, 5_000_000, 0, 999_999, 999_999);
    let block = make_alonzo_block_helper(10, 1, 0xE2, vec![tx]);
    let res = state.apply_block(&block);
    // May fail for other reasons (witness validation), but NOT BlockExUnitsExceeded.
    match res {
        Ok(_) => {} // fine
        Err(LedgerError::BlockExUnitsExceeded { .. }) => {
            panic!("block ExUnits check should be skipped when no params are set");
        }
        Err(_) => {} // other error is acceptable
    }
}
