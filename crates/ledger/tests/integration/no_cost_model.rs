//! NoCostModel validation tests.
//!
//! Upstream reference: `Cardano.Ledger.Alonzo.Plutus.Evaluate`
//! — `collectPlutusScriptsWithContext` / `CollectError` / `NoCostModel`.
//!
//! When a transaction includes a Plutus script whose language version
//! has no cost model in the protocol parameters, the transaction must be
//! rejected (Phase-1) before any CEK evaluation takes place.

use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an Alonzo-era ledger state with cost_models set to the given map.
fn alonzo_state_with_cost_models(cost_models: std::collections::BTreeMap<u8, Vec<i64>>) -> LedgerState {
    let mut state = LedgerState::new(Era::Alonzo);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((6, 0)); // Alonzo
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    pp.cost_models = Some(cost_models);
    state.set_protocol_params(pp);
    state
}

/// Build a Babbage-era ledger state with cost_models set to the given map.
fn babbage_state_with_cost_models(cost_models: std::collections::BTreeMap<u8, Vec<i64>>) -> LedgerState {
    let mut state = LedgerState::new(Era::Babbage);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((8, 0)); // Babbage
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    pp.cost_models = Some(cost_models);
    state.set_protocol_params(pp);
    state
}

/// Build a Conway-era ledger state with cost_models set to the given map.
fn conway_state_with_cost_models(cost_models: std::collections::BTreeMap<u8, Vec<i64>>) -> LedgerState {
    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((10, 0)); // Conway
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    pp.cost_models = Some(cost_models);
    state.set_protocol_params(pp);
    state
}

/// Seed a MultiEra UTxO with a script-locked Alonzo output.
fn seed_script_utxo(
    state: &mut LedgerState,
    tx_id: [u8; 32],
    script_hash: [u8; 28],
    coin: u64,
) {
    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::ScriptHash(script_hash),
    })
    .to_bytes();
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: tx_id, index: 0 },
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address,
            amount: Value::Coin(coin),
            datum_hash: Some(yggdrasil_crypto::blake2b::hash_bytes_256(&spending_datum().to_cbor_bytes()).0),
        }),
    );
}

/// Seed a VKey-locked collateral UTxO suitable for Alonzo-family script txs.
fn seed_collateral_utxo(state: &mut LedgerState, tx_id: [u8; 32], coin: u64) -> ShelleyTxIn {
    let input = ShelleyTxIn { transaction_id: tx_id, index: 1 };
    let address = Address::Enterprise(EnterpriseAddress {
        network: 1,
        payment: StakeCredential::AddrKeyHash([0xAB; 28]),
    })
    .to_bytes();
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Shelley(ShelleyTxOut { address, amount: coin }),
    );
    input
}

/// Compute the Blake2b-224 hash of a plutus script with the given version prefix.
fn test_plutus_script_hash(version_tag: u8, script_bytes: &[u8]) -> [u8; 28] {
    let mut prefixed = vec![version_tag];
    prefixed.extend_from_slice(script_bytes);
    yggdrasil_crypto::blake2b::hash_bytes_224(&prefixed).0
}

fn spending_datum() -> PlutusData {
    PlutusData::Integer(9)
}

/// Build an Alonzo block with a single transaction that includes a Plutus
/// script in the witness set (minting policy pattern — simpler than spending).
fn alonzo_block_with_plutus_v1_mint(
    script_bytes: Vec<u8>,
    policy_hash: [u8; 28],
    input_txid: [u8; 32],
    collateral_input: ShelleyTxIn,
) -> Block {
    // Build witness set with the V1 script + spending/minting redeemers.
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![spending_datum()],
        redeemers: vec![Redeemer {
            tag: 0, // spending
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }, Redeemer {
            tag: 1, // minting
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };
    let witness_bytes = ws.to_cbor_bytes();

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: input_txid, index: 0 }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01],
            amount: Value::Coin(1_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some({
            let mut m = std::collections::BTreeMap::new();
            let mut assets = std::collections::BTreeMap::new();
            assets.insert(vec![0x01], 1i64);
            m.insert(policy_hash, assets);
            m
        }),
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
        script_data_hash: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0xAA; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions: vec![Tx {
            id: TxId(tx_hash.0),
            body: body_bytes,
            witnesses: Some(witness_bytes),
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    }
}

/// Build a Babbage block with a single transaction that includes a Plutus
/// V2 minting script.
fn babbage_block_with_plutus_v2_mint(
    script_bytes: Vec<u8>,
    policy_hash: [u8; 28],
    input_txid: [u8; 32],
    collateral_input: ShelleyTxIn,
) -> Block {
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![spending_datum()],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }, Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };
    let witness_bytes = ws.to_cbor_bytes();

    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: input_txid, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some({
            let mut m = std::collections::BTreeMap::new();
            let mut assets = std::collections::BTreeMap::new();
            assets.insert(vec![0x01], 1i64);
            m.insert(policy_hash, assets);
            m
        }),
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
        script_data_hash: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([0xBA; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions: vec![Tx {
            id: TxId(tx_hash.0),
            body: body_bytes,
            witnesses: Some(witness_bytes),
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    }
}

/// Build a Conway submitted transaction with a single Plutus V3 minting script.
fn conway_submitted_tx_with_plutus_v3_mint(
    script_bytes: Vec<u8>,
    policy_hash: [u8; 28],
    input_txid: [u8; 32],
    collateral_input: ShelleyTxIn,
) -> MultiEraSubmittedTx {
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![spending_datum()],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }, Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![script_bytes],
    };

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: input_txid, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some({
            let mut m = std::collections::BTreeMap::new();
            let mut assets = std::collections::BTreeMap::new();
            assets.insert(vec![0x01], 1i64);
            m.insert(policy_hash, assets);
            m
        }),
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
        script_data_hash: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
    };

    MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body,
        ws,
        true,
        None,
    ))
}

// ---------------------------------------------------------------------------
// Tests — Block-apply path
// ---------------------------------------------------------------------------

#[test]
fn alonzo_block_rejects_v1_when_no_v1_cost_model() {
    // cost_models has V2 but not V1
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(1, vec![0i64; 175]); // V2 present

    let mut state = alonzo_state_with_cost_models(cost_models);

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = test_plutus_script_hash(0x01, &script_bytes);

    seed_script_utxo(&mut state, [0xBB; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xBC; 32], 2_000_000);

    let block = alonzo_block_with_plutus_v1_mint(script_bytes, policy_hash, [0xBB; 32], collateral);

    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 0 }),
        "expected NoCostModel for V1 (language 0), got: {err:?}",
    );
}

#[test]
fn alonzo_block_accepts_v1_when_v1_cost_model_present() {
    // cost_models has V1
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(0, vec![0i64; 166]); // V1 present

    let mut state = alonzo_state_with_cost_models(cost_models);

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = test_plutus_script_hash(0x01, &script_bytes);

    seed_script_utxo(&mut state, [0xBB; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xBC; 32], 2_000_000);

    let block = alonzo_block_with_plutus_v1_mint(script_bytes, policy_hash, [0xBB; 32], collateral);

    // This should NOT fail with NoCostModel. It may fail with other errors
    // (e.g. script_data_hash mismatch, evaluator errors) but NoCostModel
    // specifically should NOT be the error.
    let result = state.apply_block(&block);
    match &result {
        Err(LedgerError::NoCostModel { .. }) => {
            panic!("should NOT get NoCostModel when V1 cost model is present");
        }
        _ => {} // any other result is fine for this test's purpose
    }
}

#[test]
fn alonzo_block_rejects_v1_when_cost_models_empty() {
    // cost_models is an empty map — no language version has a model
    let cost_models = std::collections::BTreeMap::new();

    let mut state = alonzo_state_with_cost_models(cost_models);

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = test_plutus_script_hash(0x01, &script_bytes);

    seed_script_utxo(&mut state, [0xBB; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xBC; 32], 2_000_000);

    let block = alonzo_block_with_plutus_v1_mint(script_bytes, policy_hash, [0xBB; 32], collateral);

    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 0 }),
        "expected NoCostModel for V1, got: {err:?}",
    );
}

// ---------------------------------------------------------------------------
// Tests — Submitted-tx path
// ---------------------------------------------------------------------------

#[test]
fn alonzo_submitted_tx_rejects_v1_when_no_cost_model() {
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(1, vec![0i64; 175]); // only V2

    let mut state = alonzo_state_with_cost_models(cost_models);

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = test_plutus_script_hash(0x01, &script_bytes);

    seed_script_utxo(&mut state, [0xCC; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xCD; 32], 2_000_000);

    // Build witness set
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![script_bytes],
        plutus_data: vec![spending_datum()],
        redeemers: vec![Redeemer {
            tag: 0, // spending
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }, Redeemer {
            tag: 1, // minting
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    };

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0xCC; 32], index: 0 }],
        outputs: vec![AlonzoTxOut {
            address: vec![0x61, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01],
            amount: Value::Coin(1_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some({
            let mut m = std::collections::BTreeMap::new();
            let mut assets = std::collections::BTreeMap::new();
            assets.insert(vec![0x01], 1i64);
            m.insert(policy_hash, assets);
            m
        }),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        script_data_hash: None,
    };

    let submitted_tx = AlonzoCompatibleSubmittedTx::new(body, ws, true, None);

    let raw = submitted_tx.to_cbor_bytes();
    let tx = MultiEraSubmittedTx::from_cbor_bytes_for_era(Era::Alonzo, &raw).unwrap();

    let err = state.apply_submitted_tx(&tx, SlotNo(10), None).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 0 }),
        "expected NoCostModel for V1, got: {err:?}",
    );
}

#[test]
fn babbage_block_rejects_v2_when_only_v1_cost_model() {
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(0, vec![0i64; 166]);

    let mut state = babbage_state_with_cost_models(cost_models);

    let script_bytes = vec![0x04, 0x05, 0x06];
    let policy_hash = test_plutus_script_hash(0x02, &script_bytes);

    seed_script_utxo(&mut state, [0xB2; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xB4; 32], 2_000_000);

    let block = babbage_block_with_plutus_v2_mint(script_bytes, policy_hash, [0xB2; 32], collateral);

    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 1 }),
        "expected NoCostModel for V2 (language 1), got: {err:?}",
    );
}

#[test]
fn babbage_submitted_tx_rejects_v2_when_no_cost_model() {
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(0, vec![0i64; 166]);

    let mut state = babbage_state_with_cost_models(cost_models);

    let script_bytes = vec![0x07, 0x08, 0x09];
    let policy_hash = test_plutus_script_hash(0x02, &script_bytes);

    seed_script_utxo(&mut state, [0xB3; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xB5; 32], 2_000_000);

    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![spending_datum()],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }, Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }],
        plutus_v2_scripts: vec![script_bytes],
        plutus_v3_scripts: vec![],
    };

    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0xB3; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some({
            let mut m = std::collections::BTreeMap::new();
            let mut assets = std::collections::BTreeMap::new();
            assets.insert(vec![0x01], 1i64);
            m.insert(policy_hash, assets);
            m
        }),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        script_data_hash: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body,
        ws,
        true,
        None,
    ));

    let err = state.apply_submitted_tx(&submitted, SlotNo(10), None).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 1 }),
        "expected NoCostModel for V2 (language 1), got: {err:?}",
    );
}

#[test]
fn no_cost_model_skipped_when_cost_models_field_absent() {
    // When cost_models is None (not configured in PP), the NoCostModel check
    // is soft-skipped.
    let mut state = LedgerState::new(Era::Alonzo);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((6, 0));
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    pp.cost_models = None; // explicitly None
    state.set_protocol_params(pp);

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = test_plutus_script_hash(0x01, &script_bytes);

    seed_script_utxo(&mut state, [0xDD; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xDE; 32], 2_000_000);

    let block = alonzo_block_with_plutus_v1_mint(script_bytes, policy_hash, [0xDD; 32], collateral);

    // Should NOT produce NoCostModel when PP has no cost_models at all.
    let result = state.apply_block(&block);
    match &result {
        Err(LedgerError::NoCostModel { .. }) => {
            panic!("NoCostModel should not fire when cost_models field is absent");
        }
        _ => {} // any other result is fine
    }
}

#[test]
fn conway_block_rejects_v3_when_only_v1_v2_cost_models() {
    // cost_models has V1 + V2 but no V3
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(0, vec![0i64; 166]); // V1
    cost_models.insert(1, vec![0i64; 175]); // V2

    let mut state = conway_state_with_cost_models(cost_models);

    let script_bytes = vec![0x01, 0x02, 0x03];
    let policy_hash = test_plutus_script_hash(0x03, &script_bytes); // V3 prefix

    seed_script_utxo(&mut state, [0xEE; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xEF; 32], 2_000_000);

    // Build a Conway block with a V3 minting script
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![spending_datum()],
        redeemers: vec![Redeemer {
            tag: 0,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }, Redeemer {
            tag: 1,
            index: 0,
            data: PlutusData::Integer(0.into()),
            ex_units: ExUnits { mem: 1000, steps: 2000 },
        }],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![script_bytes],
    };
    let witness_bytes = ws.to_cbor_bytes();

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0xEE; 32], index: 0 }],
        outputs: vec![BabbageTxOut {
            address: vec![0x61, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
                          0x01, 0x01, 0x01, 0x01, 0x01],
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(100),
        certificates: None,
        validity_interval_start: None,
        mint: Some({
            let mut m = std::collections::BTreeMap::new();
            let mut assets = std::collections::BTreeMap::new();
            assets.insert(vec![0x01], 1i64);
            m.insert(policy_hash, assets);
            m
        }),
        collateral: Some(vec![collateral]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: None,
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    let block = Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([0xFF; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
        },
        transactions: vec![Tx {
            id: TxId(tx_hash.0),
            body: body_bytes,
            witnesses: Some(witness_bytes),
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 2 }),
        "expected NoCostModel for V3 (language 2), got: {err:?}",
    );
}

#[test]
fn conway_submitted_tx_rejects_v3_when_only_v1_v2_cost_models() {
    let mut cost_models = std::collections::BTreeMap::new();
    cost_models.insert(0, vec![0i64; 166]);
    cost_models.insert(1, vec![0i64; 175]);

    let mut state = conway_state_with_cost_models(cost_models);

    let script_bytes = vec![0x0A, 0x0B, 0x0C];
    let policy_hash = test_plutus_script_hash(0x03, &script_bytes);

    seed_script_utxo(&mut state, [0xF0; 32], policy_hash, 2_000_000);
    let collateral = seed_collateral_utxo(&mut state, [0xF1; 32], 2_000_000);

    let submitted = conway_submitted_tx_with_plutus_v3_mint(script_bytes, policy_hash, [0xF0; 32], collateral);

    let err = state.apply_submitted_tx(&submitted, SlotNo(10), None).unwrap_err();
    assert!(
        matches!(err, LedgerError::NoCostModel { language: 2 }),
        "expected NoCostModel for V3 (language 2), got: {err:?}",
    );
}
