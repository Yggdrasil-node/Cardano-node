//! Integration tests for MissingRedeemer predicate failures.
//!
//! Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`

use std::collections::BTreeMap;

use super::*;

fn permissive_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.max_collateral_inputs = Some(3);
    params.collateral_percentage = Some(150);
    params
}

fn mint_one(policy_hash: [u8; 28]) -> BTreeMap<[u8; 28], BTreeMap<Vec<u8>, i64>> {
    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"nft".to_vec(), 1i64);
    mint.insert(policy_hash, assets);
    mint
}

fn output_assets(policy_hash: [u8; 28]) -> BTreeMap<[u8; 28], BTreeMap<Vec<u8>, u64>> {
    let mut assets = BTreeMap::new();
    let mut policy_assets = BTreeMap::new();
    policy_assets.insert(b"nft".to_vec(), 1u64);
    assets.insert(policy_hash, policy_assets);
    assets
}

#[test]
fn alonzo_block_rejects_missing_minting_redeemer_for_plutus_policy() {
    let signer = TestSigner::new([0x71; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x11; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x12; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    let script_bytes = vec![0x51, 0x52, 0x53];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V1,
        &script_bytes,
    );
    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
            datum_hash: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint_one(policy_hash)),
        script_data_hash: None,
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };

    let body_bytes = body.to_cbor_bytes();
    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0x21; 32]),
            prev_hash: HeaderHash([0x00; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x31; 32],
        },
        transactions: vec![Tx {
            id: compute_tx_id(&body_bytes),
            body: body_bytes,
            witnesses: Some(ws.to_cbor_bytes()),
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    // Upstream fires the script integrity check (UTXOW) before the missing
    // redeemer check (UTXOS).  A transaction with Plutus scripts but no
    // script_data_hash is rejected at the integrity level.
    // Reference: `Cardano.Ledger.Alonzo.Tx.mkScriptIntegrity` — langViews
    //   is non-empty when scripts are needed, so a hash is required.
    assert!(
        matches!(result, Err(LedgerError::MissingRequiredScriptIntegrityHash)),
        "expected MissingRequiredScriptIntegrityHash for Alonzo block with scripts but no hash, got: {result:?}",
    );
}

#[test]
fn babbage_submitted_tx_rejects_missing_minting_redeemer_for_plutus_policy() {
    let signer = TestSigner::new([0x72; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x41; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x42; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let script_bytes = vec![0x61, 0x62, 0x63];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V2,
        &script_bytes,
    );
    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
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
        mint: Some(mint_one(policy_hash)),
        script_data_hash: None,
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    // Same as the Alonzo block case: upstream fires the script integrity
    // check before the missing redeemer check.  A Babbage transaction with
    // Plutus scripts but no script_data_hash is rejected at the integrity
    // level.
    assert!(
        matches!(result, Err(LedgerError::MissingRequiredScriptIntegrityHash)),
        "expected MissingRequiredScriptIntegrityHash for Babbage tx with scripts but no hash, got: {result:?}",
    );
}

/// Alonzo block-apply: Plutus minting policy with correct script_data_hash
/// but no redeemer. The Phase-1 MissingRedeemers check must reject this
/// unconditionally, before the `is_valid` dispatch.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`
#[test]
fn alonzo_block_rejects_missing_redeemer_phase1_with_valid_hash() {
    let signer = TestSigner::new([0x73; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x51; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x52; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_hash: None,
        }),
    );

    let script_bytes = vec![0x51, 0x52, 0x53];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V1,
        &script_bytes,
    );

    // Build witness set with the Plutus script but NO redeemer.
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![], // placeholder, filled after body hash
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    // Compute a correct script_data_hash from the (empty-redeemer) witness set.
    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), false);

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
            datum_hash: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint_one(policy_hash)),
        script_data_hash: Some(sdh),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        ..ws
    };

    let body_bytes = body.to_cbor_bytes();
    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0x53; 32]),
            prev_hash: HeaderHash([0x00; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x31; 32],
        },
        transactions: vec![Tx {
            id: compute_tx_id(&body_bytes),
            body: body_bytes,
            witnesses: Some(ws.to_cbor_bytes()),
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::MissingRedeemer { .. })),
        "expected MissingRedeemer for Alonzo block with Plutus policy but no redeemer, got: {result:?}",
    );
}

/// Babbage submitted-tx: Plutus V2 minting policy with correct script_data_hash
/// but no redeemer. Phase-1 MissingRedeemers check must reject.
#[test]
fn babbage_submitted_tx_rejects_missing_redeemer_phase1_with_valid_hash() {
    let signer = TestSigner::new([0x74; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x61; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x62; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let script_bytes = vec![0x61, 0x62, 0x63];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V2,
        &script_bytes,
    );

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), false);

    let body = BabbageTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
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
        mint: Some(mint_one(policy_hash)),
        script_data_hash: Some(sdh),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        ..ws
    };

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingRedeemer { .. })),
        "expected MissingRedeemer for Babbage submitted-tx with Plutus policy but no redeemer, got: {result:?}",
    );
}

/// Conway block-apply: is_valid=false transaction with Plutus policy and
/// correct script_data_hash but no redeemer. The Phase-1 check must reject
/// even though Phase-2 evaluation is skipped for is_valid=false.
///
/// This is the scenario that was previously missed — `is_valid=false` would
/// skip the Phase-2 path where MissingRedeemer was checked.
#[test]
fn conway_block_rejects_missing_redeemer_even_when_is_valid_false() {
    let signer = TestSigner::new([0x75; 32]);
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn { transaction_id: [0x71; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    let collateral = ShelleyTxIn { transaction_id: [0x72; 32], index: 0 };
    state.multi_era_utxo_mut().insert(
        collateral.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let script_bytes = vec![0x71, 0x72, 0x73];
    let policy_hash = yggdrasil_ledger::plutus_validation::plutus_script_hash(
        yggdrasil_ledger::plutus_validation::PlutusVersion::V3,
        &script_bytes,
    );

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![script_bytes],
    };
    let sdh = compute_test_script_data_hash(&ws, state.protocol_params(), true);

    let body = yggdrasil_ledger::eras::conway::ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
            amount: Value::CoinAndAssets(4_800_000, output_assets(policy_hash)),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint_one(policy_hash)),
        script_data_hash: Some(sdh),
        collateral: Some(vec![collateral.clone()]),
        required_signers: None,
        network_id: None,
        collateral_return: Some(BabbageTxOut {
            address: addr,
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
        total_collateral: Some(0),
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
    };

    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![signer.witness(&tx_body_hash)],
        ..ws
    };

    let body_bytes = body.to_cbor_bytes();
    let block = Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([0x76; 32]),
            prev_hash: HeaderHash([0x00; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x31; 32],
        },
        transactions: vec![Tx {
            id: compute_tx_id(&body_bytes),
            body: body_bytes,
            witnesses: Some(ws.to_cbor_bytes()),
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::MissingRedeemer { .. })),
        "expected MissingRedeemer for Conway is_valid=false block with Plutus policy but no redeemer, got: {result:?}",
    );
}