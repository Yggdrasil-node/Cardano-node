//! Integration tests for MissingTxBodyMetadataHash predicate failure.
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.Utxow.validateMissingTxBodyMetadataHash`

use super::*;

fn permissive_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params
}

fn sample_addr() -> Vec<u8> {
    let mut addr = vec![0x61]; // enterprise key-hash, network 1
    addr.extend_from_slice(&[0x11; 28]);
    addr
}

fn sample_shelley_tx_body() -> ShelleyTxBody {
    ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x61; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    }
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

fn seed_shelley_utxo(state: &mut LedgerState) {
    let input = ShelleyTxIn {
        transaction_id: [0x61; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input,
        ShelleyTxOut {
            address: sample_addr(),
            amount: 5_000_000,
        },
    );
}

/// aux data present but no hash declared → MissingTxBodyMetadataHash (block path)
#[test]
fn shelley_block_rejects_aux_data_without_hash() {
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());
    seed_shelley_utxo(&mut state);

    let body = sample_shelley_tx_body();
    let body_bytes = body.to_cbor_bytes();
    let ws = empty_witness_set();
    let ws_bytes = ws.to_cbor_bytes();
    let aux_data = vec![0xA1, 0x01, 0x63, 0x66, 0x6F, 0x6F]; // {1: "foo"}
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: Some(aux_data),
        is_valid: None,
    };

    let block = Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([0; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };

    let result = state.apply_block_validated(&block, None);
    assert!(
        matches!(result, Err(LedgerError::MissingTxBodyMetadataHash)),
        "expected MissingTxBodyMetadataHash, got: {:?}",
        result,
    );
}

/// aux data present but no hash → reject on submitted path too
#[test]
fn shelley_submitted_tx_rejects_aux_data_without_hash() {
    let signer = TestSigner::new([0x61; 32]);
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());
    let addr = signer.enterprise_addr();
    let input = ShelleyTxIn {
        transaction_id: [0x61; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input.clone(),
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let body = ShelleyTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let aux_data = vec![0xA1, 0x01, 0x63, 0x66, 0x6F, 0x6F]; // {1: "foo"}
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));

    let submitted =
        MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux_data)));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(
        matches!(result, Err(LedgerError::MissingTxBodyMetadataHash)),
        "expected MissingTxBodyMetadataHash, got: {:?}",
        result,
    );
}

/// no aux data, no hash → OK
#[test]
fn shelley_submitted_tx_accepts_no_aux_no_hash() {
    let signer = TestSigner::new([0x62; 32]);
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params());
    let addr = signer.enterprise_addr();
    let input = ShelleyTxIn {
        transaction_id: [0x61; 32],
        index: 0,
    };
    state.utxo_mut().insert(
        input.clone(),
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let body = ShelleyTxBody {
        inputs: vec![input],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted = MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(body, ws, None));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10), None);
    assert!(result.is_ok(), "expected OK, got: {:?}", result);
}
