//! Integration tests for ExtraRedeemer predicate failure.
//!
//! Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`

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

/// An Alonzo block with a spending redeemer targeting a native-script-locked
/// input must be rejected with ExtraRedeemer because no Plutus script backs
/// the redeemer's purpose.
///
/// Setup: script-locked input (native ScriptAll []) + collateral (VKey-locked).
/// The native script satisfies the required-script-witnesses check but the
/// spending redeemer has no matching Plutus script → ExtraRedeemer.
#[test]
fn alonzo_block_rejects_extra_redeemer_for_native_script_input() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    // Native script: ScriptAll [] (always true)
    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Seed a script-locked spending UTxO.
    let input = ShelleyTxIn {
        transaction_id: [0xB4; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

    // Seed a VKey-locked collateral UTxO (required because the tx has redeemers).
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xC0; 32],
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
        script_data_hash: None,
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
    };

    // Witness set: native script (satisfies script-witness check) + one
    // spending redeemer that targets the native-script input.
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 0,   // Spending
        index: 0, // first (only) input after sorting
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

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
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 0, index: 0 })),
        "expected ExtraRedeemer for native-script-backed input, got: {result:?}",
    );
}

/// An Alonzo block with a native-script-locked input but no redeemers should
/// apply successfully (no extra redeemers to reject).
#[test]
fn alonzo_block_accepts_native_script_tx_without_redeemers() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn {
        transaction_id: [0xB5; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );

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
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    // Witness set: native script only, no redeemers.
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
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        result.is_ok(),
        "expected Ok for native-script tx without redeemers, got: {result:?}",
    );
}

/// An Alonzo block with a minting redeemer targeting a native minting policy
/// should be rejected with ExtraRedeemer (native scripts don't use redeemers).
#[test]
fn alonzo_block_rejects_extra_minting_redeemer_for_native_policy() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    // Script-locked spending input (same native script).
    let input = ShelleyTxIn {
        transaction_id: [0xB6; 32],
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

    // Collateral UTxO (VKey-locked, required because redeemer present).
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xC1; 32],
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

    let policy_id = script_hash; // same hash, used as minting policy
    let mut mint = std::collections::BTreeMap::new();
    let mut assets = std::collections::BTreeMap::new();
    assets.insert(vec![0xAA], 1i64);
    mint.insert(policy_id, assets);

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: script_addr(&script_hash),
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
        mint: Some(mint),
        script_data_hash: None,
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
    };

    // Witness set: native script for minting policy + minting redeemer.
    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 1,   // Minting
        index: 0, // first (only) policy after sorting
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

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
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 1, index: 0 })),
        "expected ExtraRedeemer for native minting policy, got: {result:?}",
    );
}

/// An Alonzo block with `is_valid = false` and a spending redeemer targeting a
/// native-script-locked input must still be rejected with ExtraRedeemer.
///
/// Upstream, `hasExactSetOfRedeemers` is a Phase-1 UTXOW check that runs
/// unconditionally before the UTXOS `is_valid` dispatching.  So even when the
/// block producer claims scripts failed, malformed redeemer sets are rejected.
#[test]
fn alonzo_block_rejects_extra_redeemer_even_when_is_valid_false() {
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_alonzo_params());

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn {
        transaction_id: [0xB7; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }),
    );
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xC2; 32],
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

    let body = AlonzoTxBody {
        inputs: vec![input],
        outputs: vec![AlonzoTxOut {
            address: vkey_addr(),
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
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
    };

    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let body_bytes = body.to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws.to_cbor_bytes()),
        auxiliary_data: None,
        is_valid: Some(false), // block says scripts failed
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
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 0, index: 0 })),
        "expected ExtraRedeemer even with is_valid=false, got: {result:?}",
    );
}

/// A Babbage block with a spending redeemer targeting a native-script-locked
/// input must be rejected with ExtraRedeemer.
#[test]
fn babbage_block_rejects_extra_redeemer_for_native_script_input() {
    let mut state = LedgerState::new(Era::Babbage);
    let mut params = permissive_alonzo_params();
    params.max_val_size = Some(5000);
    state.set_protocol_params(params);

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn {
        transaction_id: [0xB8; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xC3; 32],
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
        script_data_hash: None,
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 0,
        index: 0,
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let body_bytes = body.to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws.to_cbor_bytes()),
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
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 0, index: 0 })),
        "expected ExtraRedeemer for Babbage block, got: {result:?}",
    );
}

/// A Conway block with a minting redeemer targeting a native minting policy
/// must be rejected with ExtraRedeemer.
#[test]
fn conway_block_rejects_extra_minting_redeemer_for_native_policy() {
    let mut state = LedgerState::new(Era::Conway);
    let mut params = permissive_alonzo_params();
    params.max_val_size = Some(5000);
    params.protocol_version = Some((10, 0));
    state.set_protocol_params(params);

    let native = NativeScript::ScriptAll(vec![]);
    let script_hash = native_script_hash(&native);

    let input = ShelleyTxIn {
        transaction_id: [0xB9; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: script_addr(&script_hash),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    let collateral_input = ShelleyTxIn {
        transaction_id: [0xC4; 32],
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

    let policy_id = script_hash;
    let mut mint = std::collections::BTreeMap::new();
    let mut assets = std::collections::BTreeMap::new();
    assets.insert(vec![0xBB], 1i64);
    mint.insert(policy_id, assets);

    let body = ConwayTxBody {
        inputs: vec![input],
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
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
        script_data_hash: None,
        collateral: Some(vec![collateral_input]),
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

    let mut ws = empty_witness_set();
    ws.native_scripts.push(native);
    ws.redeemers.push(Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::Integer(0.into()),
        ex_units: ExUnits { mem: 100, steps: 100 },
    });

    let body_bytes = body.to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws.to_cbor_bytes()),
        auxiliary_data: None,
        is_valid: Some(true),
    };

    let block = Block {
        era: Era::Conway,
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
        matches!(result, Err(LedgerError::ExtraRedeemer { tag: 1, index: 0 })),
        "expected ExtraRedeemer for Conway block, got: {result:?}",
    );
}
