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
        },
        transactions: vec![yggdrasil_ledger::Tx {
            id: tx_id,
            body: body_bytes,
            witnesses: None,
            auxiliary_data: None,
            is_valid: Some(false),
        }],
        raw_cbor: None,
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
