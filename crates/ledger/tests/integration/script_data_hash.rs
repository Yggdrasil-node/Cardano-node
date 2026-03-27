//! Integration tests for Alonzo+ `script_data_hash` validation.

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
        MultiEraTxOut::Alonzo(AlonzoTxOut {
            address: addr.to_vec(),
            amount: Value::Coin(amount),
            datum_hash: None,
        }),
    );
}

#[test]
fn alonzo_submitted_tx_accepts_matching_script_data_hash() {
    let keyhash = [0xAA; 28];
    let addr = enterprise_addr(1, &keyhash);

    let mut state = LedgerState::new(Era::Alonzo);
    state.set_protocol_params(permissive_params());

    let input = ShelleyTxIn {
        transaction_id: [0x11; 32],
        index: 0,
    };
    seed_utxo(&mut state, input.clone(), &addr, 5_000_000);

    let ws = empty_witness_set();
    let ws_bytes = ws.to_cbor_bytes();
    let computed_hash = yggdrasil_ledger::plutus_validation::compute_script_data_hash(
        Some(&ws_bytes),
        state.protocol_params(),
        false,
    )
    .expect("compute script_data_hash");

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
        script_data_hash: Some(computed_hash),
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

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
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

    let ws = empty_witness_set();
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

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::PPViewHashesDontMatch { .. })),
        "expected PPViewHashesDontMatch, got: {:?}",
        result,
    );
}
