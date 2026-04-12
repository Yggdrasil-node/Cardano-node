//! Integration tests for Alonzo+ `script_data_hash` validation.

use super::*;

fn required_scripts(hashes: impl IntoIterator<Item = [u8; 28]>) -> std::collections::HashSet<[u8; 28]> {
    hashes.into_iter().collect()
}

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

fn permissive_params() -> ProtocolParameters {
    let mut p = ProtocolParameters::default();
    p.min_fee_a = 0;
    p.min_fee_b = 0;
    p
}

fn seed_utxo(state: &mut LedgerState, txin: ShelleyTxIn, addr: &[u8], amount: u64) {
    state.multi_era_utxo_mut().insert(
        txin,
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(amount),
            datum_hash: None,
        }),
    );
}

fn seed_babbage_utxo(state: &mut LedgerState, txin: ShelleyTxIn, addr: &[u8], amount: u64) {
    state.multi_era_utxo_mut().insert(
        txin,
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(amount),
            datum_option: None,
            script_ref: None,
        }),
    );
}

#[test]
fn alonzo_submitted_tx_accepts_matching_script_data_hash() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let mut ws = empty_witness_set();
    // A redeemer is required for script_data_hash to be valid.
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });
    let ws_bytes = ws.to_cbor_bytes();
    let computed_hash = yggdrasil_ledger::plutus_validation::compute_script_data_hash(
        Some(&ws_bytes),
        state.protocol_params(),
        false,
        None,
        None,
        None,
        None,
    )
    .expect("compute script_data_hash");

    // Validate directly: matching declared hash should be accepted.
    let result = yggdrasil_ledger::plutus_validation::validate_script_data_hash(
        Some(computed_hash),
        Some(&ws_bytes),
        state.protocol_params(),
        false,
        None,
        None,
        None,
        None,
        None, // protocol_version
    );
    assert!(result.is_ok(), "expected success with matching hash: {:?}", result);
}

#[test]
fn alonzo_submitted_tx_rejects_mismatched_script_data_hash() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x22; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let mut ws = empty_witness_set();
    // Need redeemers so the hash path is exercised (not UnexpectedScriptIntegrityHash).
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(99),
        ex_units: ExUnits { mem: 50, steps: 50 },
    });
    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: Some([0xEE; 32]),
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body,
        ws,
        true,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::PPViewHashesDontMatch { .. })),
        "expected PPViewHashesDontMatch, got: {:?}",
        result,
    );
}

#[test]
fn babbage_submitted_tx_accepts_matching_script_data_hash_with_reference_script() {
    let signer = TestSigner::new([0xB1; 32]);
    let addr = signer.enterprise_addr();

    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x31; 32],
        index: 0,
    };
    let collateral = ShelleyTxIn {
        transaction_id: [0x32; 32],
        index: 0,
    };
    let reference_input = ShelleyTxIn {
        transaction_id: [0x33; 32],
        index: 0,
    };
    seed_babbage_utxo(&mut state, input.clone(), &addr, 5_000_000);
    seed_babbage_utxo(&mut state, collateral.clone(), &addr, 2_000_000);

    let script_bytes = vec![0x44, 0x55, 0x66];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V2,
        &script_bytes,
    );
    state.multi_era_utxo_mut().insert(
        reference_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV2(script_bytes.clone()))),
        }),
    );

    let mut ws = empty_witness_set();
    ws.redeemers.push(Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });
    let ws_bytes = ws.to_cbor_bytes();
    let computed_hash = yggdrasil_ledger::plutus_validation::compute_script_data_hash(
        Some(&ws_bytes),
        state.protocol_params(),
        false,
        Some(state.multi_era_utxo()),
        Some(&[reference_input.clone()]),
        None,
        Some(&required_scripts([policy_hash])),
    )
    .expect("compute script_data_hash with reference script");

    let mut output_assets = std::collections::BTreeMap::new();
    let mut minted_assets = std::collections::BTreeMap::new();
    minted_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(policy_hash, minted_assets);
    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(std::collections::BTreeMap::from([(
            policy_hash,
            std::collections::BTreeMap::from([(b"nft".to_vec(), 1i64)]),
        )])),
        script_data_hash: Some(computed_hash),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: Some(vec![reference_input]),
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body,
        ws,
        true,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(result.is_ok(), "expected success with matching Babbage reference-script hash: {:?}", result);
}

#[test]
fn babbage_submitted_tx_accepts_matching_script_data_hash_with_unused_reference_script() {
    let signer = TestSigner::new([0xB2; 32]);
    let addr = signer.enterprise_addr();

    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x51; 32],
        index: 0,
    };
    let collateral = ShelleyTxIn {
        transaction_id: [0x52; 32],
        index: 0,
    };
    let reference_input = ShelleyTxIn {
        transaction_id: [0x53; 32],
        index: 0,
    };
    seed_babbage_utxo(&mut state, input.clone(), &addr, 5_000_000);
    seed_babbage_utxo(&mut state, collateral.clone(), &addr, 2_000_000);

    let mint_script = vec![0x21, 0x22, 0x23];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V1,
        &mint_script,
    );
    state.multi_era_utxo_mut().insert(
        reference_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV2(vec![0x99, 0x98, 0x97]))),
        }),
    );

    let mut ws = empty_witness_set();
    ws.plutus_v1_scripts.push(mint_script);
    ws.redeemers.push(Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });
    let ws_bytes = ws.to_cbor_bytes();
    let required = required_scripts([policy_hash]);
    let computed_hash = yggdrasil_ledger::plutus_validation::compute_script_data_hash(
        Some(&ws_bytes),
        state.protocol_params(),
        false,
        Some(state.multi_era_utxo()),
        Some(&[reference_input.clone()]),
        None,
        Some(&required),
    )
    .expect("compute script_data_hash with unused reference script excluded");

    let mut output_assets = std::collections::BTreeMap::new();
    let mut minted_assets = std::collections::BTreeMap::new();
    minted_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(policy_hash, minted_assets);
    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(std::collections::BTreeMap::from([(
            policy_hash,
            std::collections::BTreeMap::from([(b"nft".to_vec(), 1i64)]),
        )])),
        script_data_hash: Some(computed_hash),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: Some(vec![reference_input]),
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body,
        ws,
        true,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        result.is_ok(),
        "expected success when unused reference script is excluded from script_data_hash language views: {:?}",
        result,
    );
}

#[test]
fn conway_submitted_tx_rejects_mismatched_script_data_hash_with_reference_script() {
    let signer = TestSigner::new([0xC1; 32]);
    let addr = signer.enterprise_addr();

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x41; 32],
        index: 0,
    };
    let collateral = ShelleyTxIn {
        transaction_id: [0x42; 32],
        index: 0,
    };
    let reference_input = ShelleyTxIn {
        transaction_id: [0x43; 32],
        index: 0,
    };
    seed_babbage_utxo(&mut state, input.clone(), &addr, 5_000_000);
    seed_babbage_utxo(&mut state, collateral.clone(), &addr, 2_000_000);
    state.multi_era_utxo_mut().insert(
        reference_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(2_000_000),
            datum_option: None,
            script_ref: Some(ScriptRef(Script::PlutusV3(vec![0x77, 0x88]))),
        }),
    );

    let mut ws = empty_witness_set();
    ws.redeemers.push(Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });
    let mut output_assets = std::collections::BTreeMap::new();
    let mut minted_assets = std::collections::BTreeMap::new();
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V3,
        &[0x77, 0x88],
    );
    minted_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(policy_hash, minted_assets);
    let body = ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(std::collections::BTreeMap::from([(
            policy_hash,
            std::collections::BTreeMap::from([(b"nft".to_vec(), 1i64)]),
        )])),
        script_data_hash: Some([0xEE; 32]),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: Some(vec![reference_input]),
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));

    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body,
        ws,
        true,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::PPViewHashesDontMatch { .. })),
        "expected PPViewHashesDontMatch for Conway reference-script hash, got: {:?}",
        result,
    );
}

// ---------------------------------------------------------------------------
// Bidirectional script integrity hash tests
// Reference: upstream `hashScriptIntegrity` / `ppViewHashesDontMatch`
// ---------------------------------------------------------------------------

/// Both directions absent → OK (no scripts, no hash).
#[test]
fn alonzo_no_redeemers_no_hash_accepted() {
    let signer = TestSigner::new([0xD1; 32]);
    let addr = signer.enterprise_addr();

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xD0; 32], index: 0 };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1_000),
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
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));
    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(result.is_ok(), "no redeemers + no hash should be accepted: {:?}", result);
}

/// Redeemers present, hash absent → MissingRequiredScriptIntegrityHash.
#[test]
fn alonzo_redeemers_without_hash_rejected() {
    let keyhash = [0xD2; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xD3; 32], index: 0 };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None, // deliberately absent
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    let mut ws = empty_witness_set();
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(42),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));
    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingRequiredScriptIntegrityHash)),
        "expected MissingRequiredScriptIntegrityHash, got: {:?}",
        result,
    );
}

/// Hash declared, no redeemers → UnexpectedScriptIntegrityHash.
#[test]
fn alonzo_hash_without_redeemers_rejected() {
    let keyhash = [0xD4; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xD5; 32], index: 0 };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: Some([0xFF; 32]), // declared but no redeemers
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));
    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::UnexpectedScriptIntegrityHash { .. })),
        "expected UnexpectedScriptIntegrityHash, got: {:?}",
        result,
    );
}

/// Babbage: redeemers present, hash absent → MissingRequiredScriptIntegrityHash.
#[test]
fn babbage_redeemers_without_hash_rejected() {
    let keyhash = [0xD6; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xD7; 32], index: 0 };
    seed_babbage_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None, // deliberately absent
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let mut ws = empty_witness_set();
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(1),
        ex_units: ExUnits { mem: 50, steps: 50 },
    });

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));
    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingRequiredScriptIntegrityHash)),
        "expected MissingRequiredScriptIntegrityHash for Babbage, got: {:?}",
        result,
    );
}

/// Conway: hash declared, no redeemers → UnexpectedScriptIntegrityHash.
#[test]
fn conway_hash_without_redeemers_rejected() {
    let keyhash = [0xD8; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0xD9; 32], index: 0 };
    seed_babbage_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: Some([0xAB; 32]), // declared but no redeemers
        collateral: None,
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

    let ws = empty_witness_set();
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(body, ws, true, None));
    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::UnexpectedScriptIntegrityHash { .. })),
        "expected UnexpectedScriptIntegrityHash for Conway, got: {:?}",
        result,
    );
}

// ── ScriptIntegrityHashMismatch PV>=11 split ─────────────────────────

/// At PV < 11, a mismatched script_data_hash returns PPViewHashesDontMatch.
#[test]
fn script_data_hash_mismatch_returns_ppview_at_pv10() {
    let result = yggdrasil_ledger::plutus_validation::validate_script_data_hash(
        Some([0xAA; 32]),        // declared (wrong)
        Some(&minimal_ws_with_redeemer()),
        None,
        false,
        None,
        None,
        None,
        None,
        Some((10, 0)),           // PV 10 < 11
    );
    assert!(
        matches!(result, Err(LedgerError::PPViewHashesDontMatch { .. })),
        "expected PPViewHashesDontMatch at PV 10, got: {result:?}",
    );
}

/// At PV >= 11, a mismatched script_data_hash returns ScriptIntegrityHashMismatch.
#[test]
fn script_data_hash_mismatch_returns_integrity_at_pv11() {
    let result = yggdrasil_ledger::plutus_validation::validate_script_data_hash(
        Some([0xAA; 32]),        // declared (wrong)
        Some(&minimal_ws_with_redeemer()),
        None,
        false,
        None,
        None,
        None,
        None,
        Some((11, 0)),           // PV 11 >= 11
    );
    assert!(
        matches!(result, Err(LedgerError::ScriptIntegrityHashMismatch { .. })),
        "expected ScriptIntegrityHashMismatch at PV 11, got: {result:?}",
    );
}

/// Helper: minimal witness set CBOR bytes with one trivial redeemer.
fn minimal_ws_with_redeemer() -> Vec<u8> {
    let mut ws = empty_witness_set();
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem: 1_000_000, steps: 1_000_000 },
    });
    ws.to_cbor_bytes()
}
