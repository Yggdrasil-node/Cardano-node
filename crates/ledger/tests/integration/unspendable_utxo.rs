//! Tests for B6: UnspendableUTxONoDatumHash validation.
//!
//! Validates that Plutus-script-locked spending inputs must have datum information.
//! Uses a Plutus V1 script (not a native script) so the address is NOT
//! in the `native_satisfied` set and the datum check actually fires.

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

fn witness_datum() -> PlutusData {
    PlutusData::integer(0)
}

fn witness_datum_hash() -> [u8; 32] {
    yggdrasil_crypto::blake2b::hash_bytes_256(&witness_datum().to_cbor_bytes()).0
}

/// Fake Plutus V1 script bytes used by all tests in this module.
const FAKE_PLUTUS_V1_SCRIPT: &[u8] = &[0x01];

/// Pre-computed `Blake2b-224(0x01 || 0x01)` — the Plutus V1 script hash
/// for `FAKE_PLUTUS_V1_SCRIPT`.
const FAKE_PLUTUS_SCRIPT_HASH: [u8; 28] = [
    0x66, 0xdd, 0x6f, 0xfa, 0x0c, 0x08, 0x44, 0xc7, 0x05, 0xc9, 0xf4, 0x2a, 0x60, 0x85, 0x8f, 0x79,
    0x24, 0xf4, 0x7b, 0x71, 0x66, 0x01, 0x56, 0xe9, 0x6e, 0xf5, 0xcf, 0xbe,
];

/// Alonzo block: Plutus-script-locked input with no datum hash should fail.
#[test]
fn alonzo_block_rejects_plutus_script_locked_input_without_datum_hash() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    // Spending UTxO: Plutus-script-locked but NO datum hash (unspendable).
    let spending_input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&FAKE_PLUTUS_SCRIPT_HASH),
            amount: Value::Coin(10_000_000),
            datum_hash: None, // ← NO DATUM HASH
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

    // Witness set with the Plutus V1 script whose hash matches the address.
    let mut ws = empty_witness_set();
    ws.plutus_v1_scripts.push(FAKE_PLUTUS_V1_SCRIPT.to_vec());

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
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    // Upstream fires the script integrity check (UTXOW) before the
    // UnspendableUTxONoDatumHash check (UTXOS).  This transaction has a Plutus
    // V1 script in the witness set (langViews non-empty) but no script_data_hash,
    // so the integrity check fires first.
    assert!(
        matches!(result, Err(LedgerError::MissingRequiredScriptIntegrityHash)),
        "expected MissingRequiredScriptIntegrityHash, got: {result:?}",
    );
}

/// Alonzo block: Plutus-script-locked input WITH datum hash should pass.
#[test]
fn alonzo_block_accepts_plutus_script_locked_input_with_datum_hash() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    // Spending UTxO: Plutus-script-locked WITH datum hash (spendable).
    let spending_input = ShelleyTxIn {
        transaction_id: [0xBB; 32],
        index: 0,
    };
    let datum_hash = witness_datum_hash();
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&FAKE_PLUTUS_SCRIPT_HASH),
            amount: Value::Coin(10_000_000),
            datum_hash: Some(datum_hash), // ← HAS DATUM HASH
        }),
    );
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xBC; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        collateral_input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    // Witness set with the Plutus V1 script.
    let mut ws = empty_witness_set();
    ws.plutus_v1_scripts.push(FAKE_PLUTUS_V1_SCRIPT.to_vec());
    ws.plutus_data.push(witness_datum());
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 100,
        },
    });

    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), false);
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
        script_data_hash: Some(sdh),
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
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
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
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

    // Spending UTxO: Plutus-script-locked WITH inline datum (spendable).
    let spending_input = ShelleyTxIn {
        transaction_id: [0xDD; 32],
        index: 0,
    };
    let inline_datum = PlutusData::Bytes(vec![0xAA, 0xBB]);
    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&FAKE_PLUTUS_SCRIPT_HASH),
            amount: Value::Coin(10_000_000),
            datum_option: Some(DatumOption::Inline(inline_datum)),
            script_ref: None,
        }),
    );
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xDE; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        collateral_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    // Witness set with the Plutus V1 script.
    let mut ws = empty_witness_set();
    ws.plutus_v1_scripts.push(FAKE_PLUTUS_V1_SCRIPT.to_vec());
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 100,
        },
    });

    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), false);
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
        script_data_hash: Some(sdh),
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
        reference_inputs: None,
        total_collateral: None,
        collateral_return: None,
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
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        result.is_ok(),
        "expected Ok for script-locked input with inline datum, got: {result:?}",
    );
}

// ---------------------------------------------------------------------------
// Output-side: validate_outputs_missing_datum_hash_alonzo
// (upstream: validateOutputMissingDatumHashForScriptOutputs)
// ---------------------------------------------------------------------------

/// Alonzo block with a script-locked output that is missing `datum_hash`
/// should be rejected with `MissingDatumHashOnScriptOutput`.
#[test]
fn alonzo_output_to_script_without_datum_hash_rejected() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let script_hash = [0xCC; 28];
    let out_addr = script_addr(&script_hash);
    let in_addr = vkey_addr();

    let input = ShelleyTxIn {
        transaction_id: [0xA1; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: in_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: out_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None, // missing — should trigger error
        }],
        fee: 0,
        ttl: None,
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
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);
    let tx = Tx {
        id: tx_id,
        body: body_bytes,
        witnesses: None,
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
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let err = state
        .apply_block_validated(&block, None)
        .expect_err("Alonzo output to script addr without datum_hash should fail");
    assert!(
        matches!(err, LedgerError::MissingDatumHashOnScriptOutput { .. }),
        "expected MissingDatumHashOnScriptOutput, got: {:?}",
        err,
    );
}

/// Alonzo output to a script address WITH datum_hash should pass.
#[test]
fn alonzo_output_to_script_with_datum_hash_accepted() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let script_hash = [0xCC; 28];
    let out_addr = script_addr(&script_hash);
    let in_addr = vkey_addr();

    let input = ShelleyTxIn {
        transaction_id: [0xA2; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: in_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: out_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: Some(witness_datum_hash()),
        }],
        fee: 0,
        ttl: None,
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
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);
    let tx = Tx {
        id: tx_id,
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: Some(true),
    };
    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(11),
            block_no: BlockNo(2),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("Alonzo output to script addr with datum_hash should succeed");
}

/// Alonzo output to a VKey address without datum_hash — should be fine.
#[test]
fn alonzo_output_to_vkey_without_datum_hash_accepted() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let in_addr = vkey_addr();

    let input = ShelleyTxIn {
        transaction_id: [0xA3; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: in_addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: in_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: None,
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
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);
    let tx = Tx {
        id: tx_id,
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: Some(true),
    };
    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(12),
            block_no: BlockNo(3),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("VKey address output without datum_hash is fine");
}

// ── CIP-0069 PlutusV3 datum exemption ─────────────────────────────────

/// Fake Plutus V3 script bytes.
const FAKE_PLUTUS_V3_SCRIPT: &[u8] = &[0x01];

/// Compute the Plutus V3 script hash for `FAKE_PLUTUS_V3_SCRIPT`.
fn fake_v3_script_hash() -> [u8; 28] {
    yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V3,
        FAKE_PLUTUS_V3_SCRIPT,
    )
}

/// CIP-0069: V3-locked input WITHOUT datum is accepted when v3_script_hashes
/// is provided.
#[test]
fn cip0069_v3_script_locked_input_without_datum_accepted() {
    let v3_hash = fake_v3_script_hash();
    let addr = script_addr(&v3_hash);

    let spending_input = ShelleyTxIn {
        transaction_id: [0xCC; 32],
        index: 0,
    };
    let mut utxo = MultiEraUtxo::default();
    utxo.insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr,
            amount: Value::Coin(10_000_000),
            datum_option: None, // ← no datum (CIP-0069 allows this for V3)
            script_ref: None,
        }),
    );

    let mut v3_set = std::collections::HashSet::new();
    v3_set.insert(v3_hash);

    // With V3 hashes provided, this should pass despite missing datum.
    yggdrasil_ledger::plutus_validation::validate_unspendable_utxo_no_datum_hash(
        &utxo,
        &[spending_input],
        &std::collections::HashSet::new(), // native_satisfied
        Some(&v3_set),
    )
    .expect("CIP-0069: V3 script-locked input without datum should be accepted");
}

/// CIP-0069: V1-locked input WITHOUT datum is still rejected even when V3
/// hashes are provided (V1 is not exempt).
#[test]
fn cip0069_v1_script_locked_input_without_datum_rejected() {
    let v3_hash = fake_v3_script_hash();
    let addr = script_addr(&FAKE_PLUTUS_SCRIPT_HASH);

    let spending_input = ShelleyTxIn {
        transaction_id: [0xDD; 32],
        index: 0,
    };
    let mut utxo = MultiEraUtxo::default();
    utxo.insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr,
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    // V3 set includes the V3 hash, but the input is locked by V1 — not exempt.
    let mut v3_set = std::collections::HashSet::new();
    v3_set.insert(v3_hash);

    let result = yggdrasil_ledger::plutus_validation::validate_unspendable_utxo_no_datum_hash(
        &utxo,
        &[spending_input],
        &std::collections::HashSet::new(),
        Some(&v3_set),
    );
    assert!(
        matches!(result, Err(LedgerError::UnspendableUTxONoDatumHash { .. })),
        "expected UnspendableUTxONoDatumHash for V1, got: {result:?}",
    );
}

/// CIP-0069: Without V3 hashes (None), V3-locked input without datum is
/// rejected (pre-CIP-0069 / Alonzo/Babbage behavior).
#[test]
fn cip0069_v3_input_without_v3_set_rejected() {
    let v3_hash = fake_v3_script_hash();
    let addr = script_addr(&v3_hash);

    let spending_input = ShelleyTxIn {
        transaction_id: [0xEE; 32],
        index: 0,
    };
    let mut utxo = MultiEraUtxo::default();
    utxo.insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr,
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let result = yggdrasil_ledger::plutus_validation::validate_unspendable_utxo_no_datum_hash(
        &utxo,
        &[spending_input],
        &std::collections::HashSet::new(),
        None, // no V3 hash set → pre-CIP-0069 behavior
    );
    assert!(
        matches!(result, Err(LedgerError::UnspendableUTxONoDatumHash { .. })),
        "expected rejection without V3 set, got: {result:?}",
    );
}

/// CIP-0069: collect_v3_script_hashes correctly gathers V3 scripts from
/// witness set and reference inputs.
#[test]
fn collect_v3_script_hashes_from_witnesses_and_refs() {
    let v3_hash = fake_v3_script_hash();

    // From witness set
    let mut ws = empty_witness_set();
    ws.plutus_v3_scripts.push(FAKE_PLUTUS_V3_SCRIPT.to_vec());

    let hashes_from_ws =
        yggdrasil_ledger::plutus_validation::collect_v3_script_hashes(Some(&ws), None, None);
    assert!(
        hashes_from_ws.contains(&v3_hash),
        "V3 hash from witness set"
    );

    // From reference input
    let ref_input = ShelleyTxIn {
        transaction_id: [0xFF; 32],
        index: 0,
    };
    let mut utxo = MultiEraUtxo::default();
    utxo.insert(
        ref_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV3(FAKE_PLUTUS_V3_SCRIPT.to_vec()))),
        }),
    );

    let empty_ws = empty_witness_set();
    let hashes_from_refs = yggdrasil_ledger::plutus_validation::collect_v3_script_hashes(
        Some(&empty_ws),
        Some(&utxo),
        Some(&[ref_input]),
    );
    assert!(
        hashes_from_refs.contains(&v3_hash),
        "V3 hash from reference input"
    );

    // V1 reference script should NOT be included
    let ref_input2 = ShelleyTxIn {
        transaction_id: [0xFE; 32],
        index: 0,
    };
    let mut utxo2 = MultiEraUtxo::default();
    utxo2.insert(
        ref_input2.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV1(FAKE_PLUTUS_V1_SCRIPT.to_vec()))),
        }),
    );

    let hashes_v1_only = yggdrasil_ledger::plutus_validation::collect_v3_script_hashes(
        Some(&empty_ws),
        Some(&utxo2),
        Some(&[ref_input2]),
    );
    assert!(
        hashes_v1_only.is_empty(),
        "V1 ref script should not be in V3 set"
    );
}
