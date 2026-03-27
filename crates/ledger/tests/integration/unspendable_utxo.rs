//! Tests for B6: UnspendableUTxONoDatumHash validation.
//!
//! Validates that Plutus-script-locked spending inputs must have datum information.

use super::*;

fn permissive_alonzo_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.max_collateral_inputs = Some(3);
    params.collateral_percentage = Some(150);
    params
}

/// Enterprise script-hash address (type 7, network 1) → script-locked.
fn script_addr(script_hash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x71]; // 0111_0001
    addr.extend_from_slice(script_hash);
    addr
}

/// Enterprise key-hash address (type 6, network 1) → VKey-locked.
fn vkey_addr() -> Vec<u8> {
    let mut addr = vec![0x61]; // 0110_0001
    addr.extend_from_slice(&[0xEE; 28]);
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

/// Alonzo block: Plutus-script-locked input with no datum hash should fail.
#[test]
fn alonzo_block_rejects_plutus_script_locked_input_without_datum_hash() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Spending UTxO: Plutus-script-locked but NO datum hash (unspendable).
    let spending_input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_hash: None,  // ← NO DATUM HASH
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![spending_input.clone()],
        outputs: vec![AlonzoTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
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
    };

    // Witness set with native script (required to identify the script-locked input).
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = ws.to_cbor_bytes();

    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: None,
        is_valid: Some(true),
    };

    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![tx],
        raw_cbor: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::UnspendableUTxONoDatumHash { .. })),
        "expected UnspendableUTxONoDatumHash, got: {result:?}",
    );
}

/// Alonzo block: Plutus-script-locked input WITH datum hash should pass.
#[test]
fn alonzo_block_accepts_plutus_script_locked_input_with_datum_hash() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Spending UTxO: Plutus-script-locked WITH datum hash (spendable).
    let spending_input = ShelleyTxIn {
        transaction_id: [0xBB; 32],
        index: 0,
    };
    let datum_hash = [0xCC; 32];
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_hash: Some(datum_hash),  // ← HAS DATUM HASH
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![spending_input],
        outputs: vec![AlonzoTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
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
    };

    // Witness set with native script.
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = ws.to_cbor_bytes();

    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: None,
        is_valid: Some(true),
    };

    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![tx],
        raw_cbor: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        result.is_ok(),
        "expected Ok for script-locked input with datum hash, got: {result:?}",
    );
}

/// Babbage block: Plutus-script-locked input with inline datum should pass.
#[test]
fn babbage_block_accepts_plutus_script_locked_input_with_inline_datum() {
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Spending UTxO: Plutus-script-locked WITH inline datum (spendable).
    let spending_input = ShelleyTxIn {
        transaction_id: [0xDD; 32],
        index: 0,
    };
    let inline_datum = PlutusData::Bytes(vec![0xAA, 0xBB]);
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_option: Some(DatumOption::Inline(inline_datum)),
            script_ref: None,
        }),
    );

    let body = BabbageTxBody {
        inputs: vec![spending_input],
        outputs: vec![BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
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
        reference_inputs: None,
        total_collateral: None,
        collateral_return: None,
    };

    // Witness set with native script.
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = ws.to_cbor_bytes();

    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: None,
        is_valid: Some(true),
    };

    let block = Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
        },
        transactions: vec![tx],
        raw_cbor: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        result.is_ok(),
        "expected Ok for script-locked input with inline datum, got: {result:?}",
    );
}
