//! Integration tests for ExtraneousScriptWitness predicate failure.
//!
//! Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.extraneousScriptWitnessesUTXOW`

use super::*;

fn permissive_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params
}

fn sample_addr() -> Vec<u8> {
    // Enterprise key-hash address (type 6, network 1) → VKey-locked, no script
    let mut addr = vec![0x61]; // 0110_0001
    addr.extend_from_slice(&[0x11; 28]);
    addr
}

/// Seed a VKey-locked UTxO so the transaction's input resolves.
fn seed_utxo(state: &mut LedgerState, seed: u8) {
    let input = ShelleyTxIn {
        transaction_id: [seed; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input,
        ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        },
    );
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

// ── Allegra submitted path: extraneous native script ───────────────────

/// A native script that is NOT required by any input, cert, or withdrawal
/// but IS present in the witness set should be rejected.
#[test]
fn allegra_submitted_tx_rejects_extraneous_native_script() {
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params());
    seed_utxo(&mut state, 0xA1);

    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xA1; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        validity_interval_start: None,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    // An unrequired ScriptAll([]) native script in witness set.
    let unrequired_script = NativeScript::ScriptAll(vec![]);
    let unrequired_hash = native_script_hash(&unrequired_script);

    let mut ws = empty_witness_set();
    ws.native_scripts.push(unrequired_script);

    let submitted = MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(
            result,
            Err(LedgerError::ExtraneousScriptWitness { hash }) if hash == unrequired_hash
        ),
        "expected ExtraneousScriptWitness, got: {:?}",
        result,
    );
}

// ── Allegra: required native script is NOT extraneous ──────────────────

/// When a native script IS required (input at a script address), it should
/// be accepted (no extraneous error).
#[test]
fn allegra_submitted_tx_accepts_required_native_script() {
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params());

    // Create a script-locked UTxO: address type 0x71 = enterprise script-hash
    let required_script = NativeScript::ScriptAll(vec![]); // always true
    let script_hash = native_script_hash(&required_script);
    let mut script_addr = vec![0x71]; // enterprise script-hash, network 1
    script_addr.extend_from_slice(&script_hash);

    let input = ShelleyTxIn {
        transaction_id: [0xA2; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: script_addr.clone(),
            amount: 5_000_000,
        }),
    );

    let body = AllegraTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        validity_interval_start: None,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let mut ws = empty_witness_set();
    ws.native_scripts.push(required_script);

    let submitted = MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        result.is_ok(),
        "expected Ok for required native script, got: {:?}",
        result,
    );
}

// ── Block path: extraneous native script in Allegra block ──────────────

/// Block path should also reject extraneous script witnesses.
#[test]
fn allegra_block_rejects_extraneous_native_script() {
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params());
    seed_utxo(&mut state, 0xA3);

    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xA3; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        validity_interval_start: None,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let unrequired_script = NativeScript::ScriptAll(vec![]);
    let unrequired_hash = native_script_hash(&unrequired_script);

    let mut ws = empty_witness_set();
    ws.native_scripts.push(unrequired_script);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = ws.to_cbor_bytes();

    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: None,
        is_valid: None,
    };

    let block = Block {
        era: Era::Allegra,
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
        matches!(
            result,
            Err(LedgerError::ExtraneousScriptWitness { hash }) if hash == unrequired_hash
        ),
        "expected ExtraneousScriptWitness, got: {:?}",
        result,
    );
}

// ── Shelley submitted path: extraneous native script ───────────────────

/// The Shelley submitted-tx path must reject native scripts that are not
/// required by any input, cert, or withdrawal — parity with upstream
/// `extraneousScriptWitnessesUTXOW` which applies from Shelley onward.
#[test]
fn shelley_submitted_tx_rejects_extraneous_native_script() {
    let signer = TestSigner::new([0xB1; 32]);
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());

    // Seed a VKey-locked UTxO whose address matches the signer.
    let input = ShelleyTxIn {
        transaction_id: [0xB1; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input.clone(),
        ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        },
    );

    let body = ShelleyTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let unrequired_script = NativeScript::ScriptAll(vec![]);
    let unrequired_hash = native_script_hash(&unrequired_script);

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.native_scripts.push(unrequired_script);
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));

    let submitted = MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(
            result,
            Err(LedgerError::ExtraneousScriptWitness { hash }) if hash == unrequired_hash
        ),
        "expected ExtraneousScriptWitness for Shelley, got: {:?}",
        result,
    );
}

// ── Shelley submitted path: required multisig script accepted ──────────

/// When a native script IS required (script-locked UTxO input), the
/// Shelley submitted path should accept it.
#[test]
fn shelley_submitted_tx_accepts_required_native_script() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());

    // Script-locked UTxO: enterprise script-hash address (type 0x71)
    let required_script = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&required_script);
    let mut script_addr = vec![0x71]; // enterprise script-hash, network 1
    script_addr.extend_from_slice(&script_hash);

    let input = ShelleyTxIn {
        transaction_id: [0xB2; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input.clone(),
        ShelleyTxOut {
            address: script_addr,
            amount: 5_000_000,
        },
    );

    let body = ShelleyTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let mut ws = empty_witness_set();
    ws.native_scripts.push(required_script);

    let submitted = MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        result.is_ok(),
        "expected Ok for required native script in Shelley, got: {:?}",
        result,
    );
}

// ── Shelley block path: extraneous native script ───────────────────────

/// Block path (apply_block_validated) should reject extraneous script
/// witnesses in Shelley era.
#[test]
fn shelley_block_rejects_extraneous_native_script() {
    let signer = TestSigner::new([0xB3; 32]);
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0xB3; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input.clone(),
        ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        },
    );

    let body = ShelleyTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let unrequired_script = NativeScript::ScriptAll(vec![]);
    let unrequired_hash = native_script_hash(&unrequired_script);

    let body_bytes = body.to_cbor_bytes();
    let tx_body_hash = compute_tx_id(&body_bytes).0;
    let mut ws = empty_witness_set();
    ws.native_scripts.push(unrequired_script);
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let ws_bytes = ws.to_cbor_bytes();

    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: None,
        is_valid: None,
    };

    let block = Block {
        era: Era::Shelley,
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
        matches!(
            result,
            Err(LedgerError::ExtraneousScriptWitness { hash }) if hash == unrequired_hash
        ),
        "expected ExtraneousScriptWitness for Shelley block, got: {:?}",
        result,
    );
}

// ── Shelley block path: missing script witness ─────────────────────────

/// When a Shelley-era UTxO is script-locked but no script is provided in
/// the witness set, the block path should reject with MissingScriptWitness.
#[test]
fn shelley_block_rejects_missing_script_witness() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());

    let required_script = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&required_script);
    let mut script_addr = vec![0x71];
    script_addr.extend_from_slice(&script_hash);

    let input = ShelleyTxIn {
        transaction_id: [0xB4; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input.clone(),
        ShelleyTxOut {
            address: script_addr,
            amount: 5_000_000,
        },
    );

    let body = ShelleyTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    // Witness set with NO scripts.
    let ws = empty_witness_set();

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = ws.to_cbor_bytes();

    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: None,
        is_valid: None,
    };

    let block = Block {
        era: Era::Shelley,
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
        matches!(result, Err(LedgerError::MissingScriptWitness { .. })),
        "expected MissingScriptWitness for missing multisig in Shelley block, got: {:?}",
        result,
    );
}
