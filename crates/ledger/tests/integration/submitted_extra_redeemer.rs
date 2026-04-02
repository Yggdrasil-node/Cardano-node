//! Integration tests for ExtraRedeemer validation in submitted-tx paths.
//!
//! Verifies that `apply_submitted_tx` for Alonzo/Babbage/Conway eras rejects
//! transactions with redeemers that target purposes not backed by a Plutus
//! script, matching the block-path behavior.
//!
//! Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`

use super::*;

fn permissive_params() -> ProtocolParameters {
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
    addr.extend_from_slice(&[0xCC; 28]);
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

// ---------------------------------------------------------------------------
// Alonzo submitted-tx
// ---------------------------------------------------------------------------

/// Alonzo submitted tx with a spending redeemer targeting a native-script-locked
/// input must be rejected with ExtraRedeemer.
#[test]
fn alonzo_submitted_tx_rejects_extra_redeemer_for_native_script_input() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    // Native script: ScriptAll [] (always true)
    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Spending UTxO (script-locked by native script)
    let input = ShelleyTxIn { transaction_id: [0xA1; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    // Collateral UTxO (VKey-locked)
    let coll_input = ShelleyTxIn { transaction_id: [0xC1; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        coll_input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 0,   // Spending
        index: 0,
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), false);
    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: script_addr(&script_hash),
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
        script_data_hash: Some(sdh),
        collateral: Some(vec![coll_input]),
        required_signers: None,
        network_id: None,
    };

    let raw_cbor = body.to_cbor_bytes();
    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx {
        body,
        witness_set: ws,
        is_valid: true,
        auxiliary_data: None,
        raw_cbor,
    });

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 0, index: 0 })),
        "expected ExtraRedeemer for Alonzo submitted tx, got: {result:?}",
    );
}

/// Alonzo submitted tx without redeemers should pass the ExtraRedeemer check.
#[test]
fn alonzo_submitted_tx_no_redeemers_passes() {
    let signer = TestSigner::new([0xA2; 32]);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xA2; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: signer.enterprise_addr(),
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

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        ..empty_witness_set()
    };
    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(result.is_ok(), "expected Ok for redeemer-free Alonzo submitted tx, got: {result:?}");
}

// ---------------------------------------------------------------------------
// Babbage submitted-tx
// ---------------------------------------------------------------------------

/// Babbage submitted tx with a spending redeemer targeting a native-script-locked
/// input must be rejected with ExtraRedeemer.
#[test]
fn babbage_submitted_tx_rejects_extra_redeemer_for_native_script_input() {
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn { transaction_id: [0xB1; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let coll_input = ShelleyTxIn { transaction_id: [0xC2; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        coll_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), false);
    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: script_addr(&script_hash),
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
        collateral: Some(vec![coll_input]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let raw_cbor = body.to_cbor_bytes();
    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx {
        body,
        witness_set: ws,
        is_valid: true,
        auxiliary_data: None,
        raw_cbor,
    });

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 0, index: 0 })),
        "expected ExtraRedeemer for Babbage submitted tx, got: {result:?}",
    );
}

// ---------------------------------------------------------------------------
// Conway submitted-tx
// ---------------------------------------------------------------------------

/// Conway submitted tx with a spending redeemer targeting a native-script-locked
/// input must be rejected with ExtraRedeemer.
#[test]
fn conway_submitted_tx_rejects_extra_redeemer_for_native_script_input() {
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn { transaction_id: [0xD1; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let coll_input = ShelleyTxIn { transaction_id: [0xC3; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        coll_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), true);
    let body = ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: script_addr(&script_hash),
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
        script_data_hash: Some(sdh),
        collateral: Some(vec![coll_input]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let raw_cbor = body.to_cbor_bytes();
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx {
        body,
        witness_set: ws,
        is_valid: true,
        auxiliary_data: None,
        raw_cbor,
    });

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 0, index: 0 })),
        "expected ExtraRedeemer for Conway submitted tx, got: {result:?}",
    );
}

/// Conway submitted tx with a minting redeemer but no corresponding Plutus
/// script must be rejected with ExtraRedeemer (tag=1).
#[test]
fn conway_submitted_tx_rejects_extra_minting_redeemer() {
    let signer = TestSigner::new([0xD2; 32]);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xD2; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let coll_input = ShelleyTxIn { transaction_id: [0xC4; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        coll_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: vkey_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    // Mint with the native script's hash.  The native script satisfies the
    // script-witness check, but the minting redeemer still targets a
    // native-script policy → ExtraRedeemer.
    let native = NativeScript::ScriptAll(vec![]);
    let native_hash = native_script_hash(&native);
    let mut mint2: std::collections::BTreeMap<[u8; 28], std::collections::BTreeMap<Vec<u8>, i64>> =
        std::collections::BTreeMap::new();
    mint2.insert(native_hash, std::collections::BTreeMap::new());

    // Build redeemer-bearing ws parts first to derive script_data_hash.
    let redeemer = Redeemer {
        tag: 1,   // Minting
        index: 0, // first sorted policy
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    };
    let sdh_ws = ShelleyWitnessSet {
        redeemers: vec![redeemer.clone()],
        native_scripts: vec![native.clone()],
        ..empty_witness_set()
    };
    let sdh = compute_test_script_data_hash(&sdh_ws, state.protocol_params(), true);

    let body = ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: signer.enterprise_addr(),
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
        mint: Some(mint2),
        script_data_hash: Some(sdh),
        collateral: Some(vec![coll_input]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    ws.native_scripts.push(native);
    ws.redeemers.push(redeemer);

    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 1, index: 0 })),
        "expected ExtraRedeemer for minting redeemer with native script, got: {result:?}",
    );
}
