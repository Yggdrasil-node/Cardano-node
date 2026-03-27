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

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
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

    state.multi_era_utxo_mut().insert(
        spending_input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: input_addr,
            amount: Value::Coin(5_000_000),
            datum_option: None,
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
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: Some(vec![reference_input]),
    };

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        true,
        None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "expected success, got: {:?}", result);
}
