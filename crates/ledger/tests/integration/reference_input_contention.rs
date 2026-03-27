//! Integration tests for Babbage+ reference input contention (disjointness)
//! rule: spending inputs and reference inputs must have no overlap.
//!
//! Upstream reference: `Cardano.Ledger.Babbage.Rules.Utxo` —
//! `disjoint (inputs txb) (referenceInputs txb)`.

use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn enterprise_addr(network: u8, keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60 | (network & 0x0f)];
    addr.extend_from_slice(keyhash);
    addr
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

fn mainnet_params() -> ProtocolParameters {
    let mut p = ProtocolParameters::default();
    p.min_fee_a = 0;
    p.min_fee_b = 0;
    p
}

fn seed_babbage_utxo(state: &mut LedgerState, addr: &[u8]) {
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    // Second UTxO for reference-only inputs
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(3_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
}

fn babbage_body_with_ref_inputs(
    inputs: Vec<ShelleyTxIn>,
    ref_inputs: Option<Vec<ShelleyTxIn>>,
    addr: Vec<u8>,
) -> BabbageTxBody {
    BabbageTxBody {
        inputs,
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1000),
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
        collateral_return: None,
        total_collateral: None,
        reference_inputs: ref_inputs,
    }
}

fn conway_body_with_ref_inputs(
    inputs: Vec<ShelleyTxIn>,
    ref_inputs: Option<Vec<ShelleyTxIn>>,
    addr: Vec<u8>,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs,
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: ref_inputs,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    }
}

// ===========================================================================
// Submitted-tx path — Babbage
// ===========================================================================

#[test]
fn babbage_submitted_tx_rejects_overlapping_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    // Spend input [0x01..] and also reference it — overlap
    let overlap_input = ShelleyTxIn { transaction_id: [0x01; 32], index: 0 };
    let body = babbage_body_with_ref_inputs(
        vec![overlap_input.clone()],
        Some(vec![overlap_input]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Babbage(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::ReferenceInputContention)),
        "expected ReferenceInputContention, got: {:?}",
        result,
    );
}

#[test]
fn babbage_submitted_tx_accepts_disjoint_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    // Spend [0x01..], reference [0x02..] — disjoint, should succeed
    let body = babbage_body_with_ref_inputs(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Babbage(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "disjoint ref inputs should succeed: {:?}", result);
}

#[test]
fn babbage_submitted_tx_accepts_no_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let body = babbage_body_with_ref_inputs(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        None,
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Babbage(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "no ref inputs should succeed: {:?}", result);
}

// ===========================================================================
// Submitted-tx path — Conway
// ===========================================================================

#[test]
fn conway_submitted_tx_rejects_overlapping_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let overlap_input = ShelleyTxIn { transaction_id: [0x01; 32], index: 0 };
    let body = conway_body_with_ref_inputs(
        vec![overlap_input.clone()],
        Some(vec![overlap_input]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::ReferenceInputContention)),
        "expected ReferenceInputContention for Conway, got: {:?}",
        result,
    );
}

#[test]
fn conway_submitted_tx_accepts_disjoint_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let body = conway_body_with_ref_inputs(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "disjoint ref inputs should succeed: {:?}", result);
}

// ===========================================================================
// Block-application path — Babbage
// ===========================================================================

fn make_babbage_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
    }
}

fn make_conway_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
    }
}

#[test]
fn babbage_block_rejects_overlapping_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let overlap_input = ShelleyTxIn { transaction_id: [0x01; 32], index: 0 };
    let body = babbage_body_with_ref_inputs(
        vec![overlap_input.clone()],
        Some(vec![overlap_input]),
        addr,
    );
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_babbage_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::ReferenceInputContention)),
        "expected ReferenceInputContention in block path, got: {:?}",
        result,
    );
}

#[test]
fn babbage_block_accepts_disjoint_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let body = babbage_body_with_ref_inputs(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_babbage_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "disjoint ref inputs in block path should succeed: {:?}", result);
}

// ===========================================================================
// Block-application path — Conway
// ===========================================================================

#[test]
fn conway_block_rejects_overlapping_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let overlap_input = ShelleyTxIn { transaction_id: [0x01; 32], index: 0 };
    let body = conway_body_with_ref_inputs(
        vec![overlap_input.clone()],
        Some(vec![overlap_input]),
        addr,
    );
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_conway_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::ReferenceInputContention)),
        "expected ReferenceInputContention in Conway block path, got: {:?}",
        result,
    );
}

#[test]
fn conway_block_accepts_disjoint_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());
    seed_babbage_utxo(&mut state, &addr);

    let body = conway_body_with_ref_inputs(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = make_conway_block(10, 1, 0x01, vec![
        yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "disjoint ref inputs in Conway block path should succeed: {:?}", result);
}

// ===========================================================================
// Unit test — validate_reference_input_disjointness directly
// ===========================================================================

#[test]
fn disjointness_check_rejects_overlap_at_same_index() {
    let input = ShelleyTxIn { transaction_id: [0xAA; 32], index: 3 };
    let result = MultiEraUtxo::validate_reference_input_disjointness(
        &[input.clone()],
        &[input],
    );
    assert!(matches!(result, Err(LedgerError::ReferenceInputContention)));
}

#[test]
fn disjointness_check_accepts_same_txid_different_index() {
    let spend = ShelleyTxIn { transaction_id: [0xAA; 32], index: 0 };
    let refer = ShelleyTxIn { transaction_id: [0xAA; 32], index: 1 };
    let result = MultiEraUtxo::validate_reference_input_disjointness(
        &[spend],
        &[refer],
    );
    assert!(result.is_ok());
}

#[test]
fn disjointness_check_accepts_empty_ref_inputs() {
    let spend = ShelleyTxIn { transaction_id: [0xAA; 32], index: 0 };
    let result = MultiEraUtxo::validate_reference_input_disjointness(
        &[spend],
        &[],
    );
    assert!(result.is_ok());
}
