//! Integration tests for Conway `TxRefScriptsSizeTooBig` validation.
//!
//! Verifies that the total reference-script size across all UTxO entries
//! referenced by a transaction (spending + reference inputs) does not exceed
//! `MAX_REF_SCRIPT_SIZE_PER_TX` (204,800 bytes).
//!
//! Reference: `Cardano.Ledger.Conway.Rules.Ledger` —
//! `ConwayTxRefScriptsSizeTooBig`.

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

/// Creates a BabbageTxOut with an inline PlutusV2 script reference of the
/// given byte size.
fn babbage_txout_with_ref_script(addr: &[u8], coin: u64, script_size: usize) -> BabbageTxOut {
    BabbageTxOut {
        address: addr.to_vec(),
        amount: Value::Coin(coin),
        datum_option: None,
        script_ref: Some(ScriptRef(Script::PlutusV2(vec![0xDE; script_size]))),
    }
}

fn conway_body(
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
// total_ref_scripts_size — unit tests
// ===========================================================================

#[test]
fn total_ref_scripts_size_counts_spending_and_reference_inputs() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut utxo = MultiEraUtxo::new();
    // Spending input with 1000-byte script
    utxo.insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 3_000_000, 1_000)),
    );
    // Reference input with 2000-byte script
    utxo.insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 2_000_000, 2_000)),
    );

    let total = utxo.total_ref_scripts_size(
        &[ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(&[ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
    );
    assert_eq!(total, 3_000);
}

#[test]
fn total_ref_scripts_size_skips_outputs_without_scripts() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut utxo = MultiEraUtxo::new();
    // Output without script_ref
    utxo.insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    // Output with script_ref
    utxo.insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 2_000_000, 500)),
    );

    let total = utxo.total_ref_scripts_size(
        &[ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(&[ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
    );
    assert_eq!(total, 500);
}

#[test]
fn total_ref_scripts_size_no_inputs_returns_zero() {
    let utxo = MultiEraUtxo::new();
    let total = utxo.total_ref_scripts_size(&[], None);
    assert_eq!(total, 0);
}

// ===========================================================================
// Conway submitted-tx path
// ===========================================================================

#[test]
fn conway_submitted_tx_rejects_oversized_ref_scripts() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());

    // Spending input with no script
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    // Reference input with script exceeding 204,800 bytes
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 3_000_000, 204_801)),
    );

    let body = conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::TxRefScriptsSizeTooBig { actual: 204_801, max_allowed: 204_800 })),
        "expected TxRefScriptsSizeTooBig, got: {:?}",
        result,
    );
}

#[test]
fn conway_submitted_tx_accepts_ref_scripts_at_limit() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());

    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    // Exactly 204,800 bytes — should pass
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 3_000_000, 204_800)),
    );

    let body = conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "ref scripts at exact limit should succeed: {:?}", result);
}

#[test]
fn conway_submitted_tx_accepts_no_ref_scripts() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());

    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let body = conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        None,
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "no ref scripts should succeed: {:?}", result);
}

// ===========================================================================
// Conway block-application path
// ===========================================================================

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
fn conway_block_rejects_oversized_ref_scripts() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());

    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    // Over-limit reference script
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 3_000_000, 204_801)),
    );

    let body = conway_body(
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
        is_valid: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::TxRefScriptsSizeTooBig { actual: 204_801, max_allowed: 204_800 })),
        "expected TxRefScriptsSizeTooBig in block path, got: {:?}",
        result,
    );
}

#[test]
fn conway_block_accepts_ref_scripts_under_limit() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());

    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    // Small script — well under limit
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 3_000_000, 1_000)),
    );

    let body = conway_body(
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
        is_valid: None,
        },
    ]);

    let result = state.apply_block_validated(&block, None);
    assert!(result.is_ok(), "ref scripts under limit should succeed: {:?}", result);
}

// ===========================================================================
// Cumulative size across spending + reference inputs
// ===========================================================================

#[test]
fn conway_submitted_tx_cumulates_scripts_across_both_input_types() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(mainnet_params());

    // Spending input with 102,401 byte script
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 5_000_000, 102_401)),
    );
    // Reference input with 102,400 byte script → total = 204,801 (over limit)
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(babbage_txout_with_ref_script(&addr, 3_000_000, 102_400)),
    );

    let body = conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        Some(vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }]),
        addr,
    );
    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(
        AlonzoCompatibleSubmittedTx::new(body, ws, true, None),
    );

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::TxRefScriptsSizeTooBig { actual: 204_801, max_allowed: 204_800 })),
        "expected cumulative size to trigger rejection, got: {:?}",
        result,
    );
}
