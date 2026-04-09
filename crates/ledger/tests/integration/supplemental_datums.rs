//! Integration tests for NotAllowedSupplementalDatums predicate failure.
//!
//! Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.validateRequiredDatums`
//! (`NotAllowedSupplementalDatums`)

use super::*;

fn permissive_alonzo_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.max_collateral_inputs = Some(3);
    params.collateral_percentage = Some(150);
    params
}

/// Enterprise key-hash address (type 6, network 1) → VKey-locked.
fn vkey_addr() -> Vec<u8> {
    let mut addr = vec![0x61]; // 0110_0001
    addr.extend_from_slice(&[0xEE; 28]);
    addr
}

/// Enterprise script-hash address (type 7, network 1) → script-locked.
fn script_addr(script_hash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x71]; // 0111_0001
    addr.extend_from_slice(script_hash);
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

/// An Alonzo block with a supplemental datum (in witness set, not required by
/// any spending input, and not declared in any output) should be rejected with
/// NotAllowedSupplementalDatums.
///
/// Uses a native-script-locked input to avoid VKey witness requirements.
#[test]
fn alonzo_block_rejects_unreferenced_supplemental_datum() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Seed a script-locked UTxO (no datum).
    let input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_hash: None, // output has no datum hash
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

    // Witness set: native script + an orphan datum that isn't referenced anywhere.
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    let orphan_datum = PlutusData::Integer(999.into());
    ws.plutus_data.push(orphan_datum.clone());
    let orphan_hash = {
        use yggdrasil_ledger::CborEncode;
        let cbor = orphan_datum.to_cbor_bytes();
        yggdrasil_crypto::blake2b::hash_bytes_256(&cbor).0
    };

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
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::NotAllowedSupplementalDatums { hash }) if hash == orphan_hash),
        "expected NotAllowedSupplementalDatums for orphan datum, got: {result:?}",
    );
}

/// An Alonzo block with a supplemental datum that IS declared in a transaction
/// output datum hash should pass.
///
/// Uses a native-script-locked input.
#[test]
fn alonzo_block_accepts_supplemental_datum_declared_in_output() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn {
        transaction_id: [0xBB; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    // Declare a datum hash in the output
    let declared_datum = PlutusData::Map(vec![]);
    let declared_hash = {
        use yggdrasil_ledger::CborEncode;
        let cbor = declared_datum.to_cbor_bytes();
        yggdrasil_crypto::blake2b::hash_bytes_256(&cbor).0
    };

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_hash: Some(declared_hash),
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

    // Witness set includes the native script + datum that matches the output hash
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.plutus_data.push(declared_datum);

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
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        result.is_ok(),
        "expected Ok for supplemental datum matching output, got: {result:?}",
    );
}

/// A Babbage block with a supplemental datum matching a reference-input UTxO
/// datum hash should pass.
///
/// Uses a native-script-locked input. The reference-input UTxO has a datum
/// hash (not inline), so the witness datum is allowed as supplemental.
#[test]
fn babbage_block_accepts_supplemental_datum_from_reference_input() {
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Spending UTxO (script-locked)
    let spending_input = ShelleyTxIn {
        transaction_id: [0xCC; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    // Reference UTxO with a datum hash (not inline)
    let ref_input = ShelleyTxIn {
        transaction_id: [0xDD; 32],
        index: 1,
    };
    let ref_datum = PlutusData::Bytes(vec![0xAA, 0xBB]);
    let ref_hash = {
        use yggdrasil_ledger::CborEncode;
        let cbor = ref_datum.to_cbor_bytes();
        yggdrasil_crypto::blake2b::hash_bytes_256(&cbor).0
    };
    state.multi_era_utxo_mut().insert(
        ref_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: Some(DatumOption::Hash(ref_hash)),
            script_ref: None,
        }),
    );

    let body = BabbageTxBody {
        inputs: vec![spending_input],
        outputs: vec![BabbageTxOut {
            address: script_addr(&script_hash),
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
        reference_inputs: Some(vec![ref_input]),
        total_collateral: None,
        collateral_return: None,
    };

    // Witness set includes the native script + supplemental datum
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.plutus_data.push(ref_datum);

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
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        result.is_ok(),
        "expected Ok for supplemental datum from reference input, got: {result:?}",
    );
}
