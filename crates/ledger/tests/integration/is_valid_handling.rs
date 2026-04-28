//! Integration tests for Alonzo+ `is_valid` handling.
//!
//! Verifies submitted-transaction rejection for `is_valid = false` and
//! block-path collateral-only application for `is_valid = false`.

use super::*;

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
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(amount),
            datum_option: None,
            script_ref: None,
        }),
    );
}

#[test]
fn submitted_alonzo_tx_rejects_is_valid_false() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    };
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
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        false,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::SubmittedTxIsInvalid)),
        "expected SubmittedTxIsInvalid, got: {:?}",
        result,
    );
}

#[test]
fn submitted_babbage_tx_rejects_is_valid_false() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x02; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

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
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        false,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::SubmittedTxIsInvalid)),
        "expected SubmittedTxIsInvalid, got: {:?}",
        result,
    );
}

#[test]
fn submitted_conway_tx_rejects_is_valid_false() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x03; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

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
        script_data_hash: None,
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

    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        false,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::SubmittedTxIsInvalid)),
        "expected SubmittedTxIsInvalid, got: {:?}",
        result,
    );
}

#[test]
fn alonzo_block_is_valid_false_applies_collateral_only() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let spend_input = ShelleyTxIn {
        transaction_id: [0x10; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x20; 32],
        index: 0,
    };
    seed_utxo(&mut state, spend_input.clone(), &addr, 5_000_000);
    seed_utxo(&mut state, collateral_input.clone(), &addr, 3_000_000);

    let body = AlonzoTxBody {
        inputs: vec![spend_input.clone()],
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
        script_data_hash: None,
        collateral: Some(vec![collateral_input.clone()]),
        required_signers: None,
        network_id: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0x01; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: vec![yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("block with is_valid=false should apply collateral-only transition");

    assert!(
        state.multi_era_utxo().get(&spend_input).is_some(),
        "regular spending input must remain unspent when is_valid=false"
    );
    assert!(
        state.multi_era_utxo().get(&collateral_input).is_none(),
        "collateral input must be consumed when is_valid=false"
    );

    let produced = ShelleyTxIn {
        transaction_id: tx_id.0,
        index: 0,
    };
    assert!(
        state.multi_era_utxo().get(&produced).is_none(),
        "normal tx outputs must not be produced when is_valid=false"
    );
}

/// Conway block with `is_valid = false` and an **incorrect**
/// `current_treasury_value` must be accepted — upstream places
/// `validateTreasuryValue` inside the `IsValid True` branch of
/// `conwayLedgerTransitionTRC`, so it is skipped for invalid txs.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ledger` — `conwayLedgerTransitionTRC`.
#[test]
fn conway_block_is_valid_false_skips_treasury_value_check() {
    let keyhash = [0xBB; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());
    // Set treasury to a known value.
    state.accounting_mut().treasury = 100;

    let spend_input = ShelleyTxIn {
        transaction_id: [0x30; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x31; 32],
        index: 0,
    };
    seed_utxo(&mut state, spend_input.clone(), &addr, 5_000_000);
    seed_utxo(&mut state, collateral_input.clone(), &addr, 3_000_000);

    let body = ConwayTxBody {
        inputs: vec![spend_input.clone()],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
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
        script_data_hash: None,
        collateral: Some(vec![collateral_input.clone()]),
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        // Intentionally wrong — treasury is 100 but we claim 999.
        current_treasury_value: Some(999),
        treasury_donation: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([0x02; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: vec![Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("is_valid=false tx with wrong treasury value should be accepted");

    assert!(
        state.multi_era_utxo().get(&spend_input).is_some(),
        "regular spending input must remain unspent when is_valid=false"
    );
    assert!(
        state.multi_era_utxo().get(&collateral_input).is_none(),
        "collateral input must be consumed when is_valid=false"
    );
}

/// Verify that a Conway `is_valid = true` tx with wrong treasury value
/// **is** rejected — the check must be active for valid transactions.
#[test]
fn conway_block_is_valid_true_rejects_wrong_treasury_value() {
    let keyhash = [0xCC; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());
    state.accounting_mut().treasury = 100;

    let input = ShelleyTxIn {
        transaction_id: [0x40; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let body = ConwayTxBody {
        inputs: vec![input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        // Wrong treasury value — should trigger rejection for valid tx.
        current_treasury_value: Some(999),
        treasury_donation: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([0x03; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: vec![Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(true),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(
            result,
            Err(LedgerError::CurrentTreasuryValueIncorrect { .. })
        ),
        "is_valid=true tx with wrong treasury value must be rejected, got: {:?}",
        result,
    );
}

/// Alonzo block with `is_valid = false` tx carrying a PPUP update must NOT
/// collect the proposal — upstream `alonzoEvalScriptsTxInvalid` returns
/// `pure pup` (unchanged proposals) and does not run the DELEGS sub-rule.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxos` — `alonzoEvalScriptsTxInvalid`.
#[test]
fn alonzo_block_is_valid_false_skips_ppup_collection() {
    let keyhash = [0xDD; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    let mut params = ProtocolParameters::alonzo_defaults();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    state.set_protocol_params(params);
    // Seed a genesis delegate so PPUP would be valid if collected.
    let genesis_hash = [0x01; 28];
    state.gen_delegs_mut().insert(
        genesis_hash,
        yggdrasil_ledger::GenesisDelegationState {
            delegate: [0x02; 28],
            vrf: [0x03; 32],
        },
    );

    let spend_input = ShelleyTxIn {
        transaction_id: [0x50; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x51; 32],
        index: 0,
    };
    seed_utxo(&mut state, spend_input.clone(), &addr, 5_000_000);
    seed_utxo(&mut state, collateral_input.clone(), &addr, 3_000_000);

    let update = ShelleyUpdate {
        proposed_protocol_parameter_updates: {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                genesis_hash,
                ProtocolParameterUpdate {
                    min_fee_a: Some(999),
                    ..Default::default()
                },
            );
            m
        },
        epoch: 1,
    };

    let body = AlonzoTxBody {
        inputs: vec![spend_input],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_hash: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: Some(update),
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: Some(vec![collateral_input]),
        required_signers: None,
        network_id: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = Block {
        era: Era::Alonzo,
        header: BlockHeader {
            hash: HeaderHash([0x04; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: vec![Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("is_valid=false block should succeed");

    assert!(
        state.pending_pparam_updates().is_empty(),
        "PPUP proposals from is_valid=false tx must NOT be collected, \
         got: {:?}",
        state.pending_pparam_updates(),
    );
}

/// Babbage block with `is_valid = false` tx carrying MIR certificates must
/// NOT accumulate the MIR entries — upstream `alonzoEvalScriptsTxInvalid`
/// does not run the DELEGS sub-rule for invalid transactions.
///
/// Reference: `Cardano.Ledger.Alonzo.Rules.Utxos` — `alonzoEvalScriptsTxInvalid`.
#[test]
fn babbage_block_is_valid_false_skips_mir_collection() {
    let keyhash = [0xEE; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Babbage);
    let mut params = ProtocolParameters::alonzo_defaults();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    state.set_protocol_params(params);

    let cred = StakeCredential::AddrKeyHash([0xF1; 28]);
    let acct = RewardAccount {
        network: 1,
        credential: cred,
    };
    state
        .reward_accounts_mut()
        .insert(acct, RewardAccountState::new(0, None));

    let spend_input = ShelleyTxIn {
        transaction_id: [0x60; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x61; 32],
        index: 0,
    };
    seed_utxo(&mut state, spend_input.clone(), &addr, 5_000_000);
    seed_utxo(&mut state, collateral_input.clone(), &addr, 3_000_000);

    let body = BabbageTxBody {
        inputs: vec![spend_input],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: Some(1_000),
        certificates: Some(vec![DCert::MoveInstantaneousReward(
            yggdrasil_ledger::MirPot::Reserves,
            yggdrasil_ledger::MirTarget::StakeCredentials({
                let mut m = std::collections::BTreeMap::new();
                m.insert(cred, 1_000_000i64);
                m
            }),
        )]),
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
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([0x05; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: vec![Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("is_valid=false block should succeed");

    assert!(
        state.instantaneous_rewards().ir_reserves.is_empty(),
        "MIR from is_valid=false tx must NOT be accumulated, \
         got ir_reserves: {:?}",
        state.instantaneous_rewards().ir_reserves,
    );
}

/// Babbage block with `is_valid = false` tx with collateral return must place
/// the collateral return output at index `body.outputs.len()`, not `u16::MAX`.
///
/// Upstream `mkCollateralTxIn`: `TxIn txId (mkTxIxFor body)` where
/// `mkTxIxFor body = fromIntegral $ length (body ^. outputsTxBodyL)`.
///
/// Reference: `Cardano.Ledger.Babbage.TxBody` — `mkCollateralTxIn`.
#[test]
fn babbage_collateral_return_index_equals_outputs_len() {
    let keyhash = [0xEE; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Babbage);
    state.set_protocol_params(permissive_params());

    let spend_input = ShelleyTxIn {
        transaction_id: [0x60; 32],
        index: 0,
    };
    let collateral_input = ShelleyTxIn {
        transaction_id: [0x61; 32],
        index: 0,
    };
    seed_utxo(&mut state, spend_input.clone(), &addr, 5_000_000);
    seed_utxo(&mut state, collateral_input.clone(), &addr, 3_000_000);

    // Transaction with 2 regular outputs + collateral return.
    let body = BabbageTxBody {
        inputs: vec![spend_input.clone()],
        outputs: vec![
            BabbageTxOut {
                address: addr.clone(),
                amount: Value::Coin(2_500_000),
                datum_option: None,
                script_ref: None,
            },
            BabbageTxOut {
                address: addr.clone(),
                amount: Value::Coin(2_500_000),
                datum_option: None,
                script_ref: None,
            },
        ],
        fee: 0,
        ttl: Some(1_000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: Some(vec![collateral_input.clone()]),
        required_signers: None,
        network_id: None,
        collateral_return: Some(BabbageTxOut {
            address: addr,
            amount: Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }),
        total_collateral: Some(2_000_000),
        reference_inputs: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_id = compute_tx_id(&body_bytes);

    let block = Block {
        era: Era::Babbage,
        header: BlockHeader {
            hash: HeaderHash([0x04; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: vec![Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
        header_cbor_size: None,
    };

    state
        .apply_block_validated(&block, None)
        .expect("block with is_valid=false and collateral return should apply");

    // Collateral input must be consumed.
    assert!(
        state.multi_era_utxo().get(&collateral_input).is_none(),
        "collateral input must be consumed"
    );

    // Collateral return output must be at index = outputs.len() = 2.
    let correct_return_txin = ShelleyTxIn {
        transaction_id: tx_id.0,
        index: 2, // body.outputs.len()
    };
    assert!(
        state.multi_era_utxo().get(&correct_return_txin).is_some(),
        "collateral return must be at index {} (= outputs.len()), \
         but not found in UTxO",
        2,
    );

    // No output should exist at the old sentinel (u16::MAX).
    let old_sentinel_txin = ShelleyTxIn {
        transaction_id: tx_id.0,
        index: u16::MAX,
    };
    assert!(
        state.multi_era_utxo().get(&old_sentinel_txin).is_none(),
        "collateral return must NOT be at u16::MAX index"
    );

    // Normal outputs must NOT be produced for is_valid=false tx.
    let output_0 = ShelleyTxIn {
        transaction_id: tx_id.0,
        index: 0,
    };
    assert!(
        state.multi_era_utxo().get(&output_0).is_none(),
        "regular output at index 0 must not be produced for is_valid=false"
    );
}
