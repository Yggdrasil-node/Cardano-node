//! Integration tests for submitted-transaction required script witness checks.

use super::*;

fn enterprise_addr(network: u8, keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60 | (network & 0x0f)];
    addr.extend_from_slice(keyhash);
    addr
}

fn enterprise_script_addr(network: u8, script_hash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x70 | (network & 0x0f)];
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

fn spending_datum() -> PlutusData {
    PlutusData::Bytes(vec![0xCA, 0xFE])
}

fn spending_datum_hash() -> [u8; 32] {
    yggdrasil_crypto::blake2b::hash_bytes_256(&spending_datum().to_cbor_bytes()).0
}

fn permissive_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params
}

#[test]
fn alonzo_submitted_tx_rejects_missing_required_script_witness() {
    let script_hash = [0xAB; 28];
    let input_addr = enterprise_script_addr(1, &script_hash);
    let output_addr = enterprise_addr(1, &[0x11; 28]);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x21; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: input_addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: output_addr,
            amount: Value::Coin(5_000_000),
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

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        true,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingScriptWitness { hash }) if hash == script_hash),
        "expected MissingScriptWitness, got: {:?}",
        result,
    );
}

#[test]
fn babbage_submitted_tx_accepts_required_script_from_reference_input() {
    let plutus_bytes = vec![0x42, 0x24, 0x99];
    let script_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V2,
        &plutus_bytes,
    );
    let input_addr = enterprise_script_addr(1, &script_hash);
    let output_addr = enterprise_addr(1, &[0x44; 28]);

    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let spending_input = ShelleyTxIn {
        transaction_id: [0x31; 32],
        index: 0,
    };
    let reference_input = ShelleyTxIn {
        transaction_id: [0x32; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x33; 32],
        index: 0,
    };

    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: input_addr,
            amount: Value::Coin(5_000_000),
            datum_option: Some(DatumOption::Hash(spending_datum_hash())),
            script_ref: None,
        }),
    );
    state.multi_era_utxo_mut().insert(
        reference_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: output_addr.clone(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV2(plutus_bytes))),
        }),
    );
    state.multi_era_utxo_mut().insert(
        collateral_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: output_addr.clone(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let mut ws = empty_witness_set();
    ws.plutus_data.push(spending_datum());
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::integer(0),
        ex_units: ExUnits {
            mem: 100,
            steps: 100,
        },
    });

    // Compute sdh including reference-input scripts so language_views contain V2 cost model.
    let ws_bytes = ws.to_cbor_bytes();
    let sdh = yggdrasil_ledger::plutus_validation::compute_script_data_hash(
        Some(&ws_bytes),
        state.protocol_params(),
        false,
        Some(state.multi_era_utxo()),
        Some(std::slice::from_ref(&reference_input)),
        None,
        None,
    )
    .expect("compute sdh with reference inputs");
    let body = BabbageTxBody {
        inputs: vec![spending_input],
        outputs: vec![BabbageTxOut {
            address: output_addr,
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
        script_data_hash: Some(sdh),
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: Some(vec![reference_input]),
    };

    let submitted =
        MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(result.is_ok(), "expected success, got: {:?}", result);
}

#[test]
fn allegra_submitted_tx_rejects_missing_native_script_witness() {
    let script_hash = [0xCC; 28];
    let input_addr = enterprise_script_addr(1, &script_hash);
    let output_addr = enterprise_addr(1, &[0x11; 28]);

    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x41; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: input_addr,
            amount: 5_000_000,
        }),
    );

    let body = AllegraTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: output_addr,
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let submitted = MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingScriptWitness { hash }) if hash == script_hash),
        "expected MissingScriptWitness for Allegra, got: {:?}",
        result,
    );
}

#[test]
fn allegra_submitted_tx_accepts_native_script_witness() {
    // Use ScriptAll([]) — always evaluates to true without needing VKey witnesses.
    let always_true_script = NativeScript::ScriptAll(vec![]);
    let always_true_hash = native_script_hash(&always_true_script);
    let input_addr = enterprise_script_addr(1, &always_true_hash);
    let output_addr = enterprise_addr(1, &[0x11; 28]);

    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x43; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: input_addr,
            amount: 5_000_000,
        }),
    );

    let body = AllegraTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: output_addr,
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let mut ws = empty_witness_set();
    ws.native_scripts = vec![always_true_script];

    let submitted = MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(result.is_ok(), "expected Allegra accept, got: {:?}", result);
}

#[test]
fn mary_submitted_tx_rejects_missing_native_script_for_mint() {
    let signer = TestSigner::new([0x51; 32]);
    let script_hash = [0xEE; 28];
    let output_addr = enterprise_addr(1, &[0x11; 28]);

    let mut state = LedgerState::new(Era::Mary);
    state.set_protocol_params(permissive_params());

    // Fund a plain key-hash input so UTxO resolution succeeds.
    let input = ShelleyTxIn {
        transaction_id: [0x51; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Mary(MaryTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
        }),
    );

    let mut mint_map = std::collections::BTreeMap::new();
    let mut asset_map = std::collections::BTreeMap::new();
    asset_map.insert(vec![0x01], 1i64);
    mint_map.insert(script_hash, asset_map);

    let body = MaryTxBody {
        inputs: vec![input],
        outputs: vec![MaryTxOut {
            address: output_addr,
            amount: Value::Coin(5_000_000),
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint_map),
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));

    let submitted = MultiEraSubmittedTx::Mary(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingScriptWitness { hash }) if hash == script_hash),
        "expected MissingScriptWitness for Mary mint, got: {:?}",
        result,
    );
}
