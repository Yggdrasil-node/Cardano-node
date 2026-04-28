//! Integration tests for InvalidMetadata predicate failure.
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.Utxow.validateMetadata`
//!            `Cardano.Ledger.Metadata.validMetadatum`
//!            `Cardano.Ledger.Shelley.SoftForks.validMetadata` — active when
//!            protocol version > (2, 0), i.e. from Allegra (PV 3.0) onward.

use super::*;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn permissive_params_pv(major: u64, minor: u64) -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.protocol_version = Some((major, minor));
    params
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

fn seed_shelley_utxo(state: &mut LedgerState, signer: &TestSigner, tx_hash: [u8; 32], amount: u64) {
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: tx_hash,
            index: 0,
        },
        ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount,
        },
    );
}

fn seed_multi_era_utxo(
    state: &mut LedgerState,
    signer: &TestSigner,
    tx_hash: [u8; 32],
    amount: u64,
) {
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: tx_hash,
            index: 0,
        },
        ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount,
        },
    );
}

/// Builds a Shelley-era metadata map `{ uint => metadatum }` with a single
/// key-value entry.
///
/// CBOR layout: `A1 <key_uint> <value>` (1-entry map).
fn shelley_metadata_with_value(key: u64, value_cbor: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.map(1);
    enc.unsigned(key);
    let mut out = enc.into_bytes();
    out.extend_from_slice(value_cbor);
    out
}

/// Encodes a CBOR bytes item with the given length.
fn cbor_bytes(len: usize) -> Vec<u8> {
    let data = vec![0xAA; len];
    let mut enc = Encoder::new();
    enc.bytes(&data);
    enc.into_bytes()
}

/// Encodes a CBOR text item of the given UTF-8 byte length.
fn cbor_text(len: usize) -> Vec<u8> {
    let text = "x".repeat(len);
    let mut enc = Encoder::new();
    enc.text(&text);
    enc.into_bytes()
}

fn aux_hash(data: &[u8]) -> [u8; 32] {
    yggdrasil_crypto::hash_bytes_256(data).0
}

// -----------------------------------------------------------------------
// Rejection tests — block-apply path (PV 3.0+, soft fork active)
// -----------------------------------------------------------------------

/// Bytes metadatum 65 bytes long — must be rejected with InvalidMetadata.
#[test]
fn allegra_block_rejects_bytes_65() {
    let signer = TestSigner::new([0x02; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));

    let aux = shelley_metadata_with_value(1, &cbor_bytes(65));
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
    };
    seed_multi_era_utxo(&mut state, &signer, [0x02; 32], 5_000_000);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = empty_witness_set().to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: Some(aux.clone()),
        is_valid: None,
    };
    let block = Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([0xAA; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };
    let err = state.apply_block_validated(&block, None).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidMetadata),
        "65-byte bytes metadatum should trigger InvalidMetadata, got: {err:?}",
    );
}

/// Text metadatum 65 UTF-8 bytes — must be rejected.
#[test]
fn allegra_block_rejects_text_65() {
    let signer = TestSigner::new([0x04; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));

    let aux = shelley_metadata_with_value(1, &cbor_text(65));
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x04; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
    };
    seed_multi_era_utxo(&mut state, &signer, [0x04; 32], 5_000_000);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = empty_witness_set().to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: Some(aux.clone()),
        is_valid: None,
    };
    let block = Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([0xAA; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };
    let err = state.apply_block_validated(&block, None).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidMetadata),
        "65-byte text metadatum should trigger InvalidMetadata, got: {err:?}",
    );
}

/// Array with a nested oversized bytes metadatum — must be rejected.
#[test]
fn allegra_block_rejects_nested_oversized_bytes_in_array() {
    let signer = TestSigner::new([0x07; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));

    // Metadata value: [h'AA..AA' (65 bytes)]
    let oversized = cbor_bytes(65);
    let mut value_cbor = Vec::new();
    value_cbor.push(0x81); // CBOR array(1)
    value_cbor.extend_from_slice(&oversized);

    let aux = shelley_metadata_with_value(1, &value_cbor);
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x07; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
    };
    seed_multi_era_utxo(&mut state, &signer, [0x07; 32], 5_000_000);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = empty_witness_set().to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: Some(aux.clone()),
        is_valid: None,
    };
    let block = Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([0xAA; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };
    let err = state.apply_block_validated(&block, None).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidMetadata),
        "nested oversized bytes inside array should trigger InvalidMetadata, got: {err:?}",
    );
}

/// Map with an oversized text value — must be rejected.
#[test]
fn allegra_block_rejects_nested_oversized_text_in_map() {
    let signer = TestSigner::new([0x08; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));

    // Metadata value: { 1: "xxx...x" (65 chars) }
    let oversized_text = cbor_text(65);
    let mut value_cbor = Vec::new();
    value_cbor.push(0xA1); // map(1)
    value_cbor.push(0x01); // uint(1) key
    value_cbor.extend_from_slice(&oversized_text);

    let aux = shelley_metadata_with_value(42, &value_cbor);
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x08; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
    };
    seed_multi_era_utxo(&mut state, &signer, [0x08; 32], 5_000_000);

    let body_bytes = body.to_cbor_bytes();
    let ws_bytes = empty_witness_set().to_cbor_bytes();
    let tx = Tx {
        id: compute_tx_id(&body_bytes),
        body: body_bytes,
        witnesses: Some(ws_bytes),
        auxiliary_data: Some(aux.clone()),
        is_valid: None,
    };
    let block = Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([0xAA; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(10),
            block_no: BlockNo(1),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: vec![tx],
        raw_cbor: None,
        header_cbor_size: None,
    };
    let err = state.apply_block_validated(&block, None).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidMetadata),
        "nested oversized text inside map should trigger InvalidMetadata, got: {err:?}",
    );
}

// -----------------------------------------------------------------------
// Acceptance tests — submitted-tx path with TestSigner
// -----------------------------------------------------------------------

/// Bytes metadatum exactly 64 bytes — accepted.
#[test]
fn submitted_allegra_accepts_bytes_64() {
    let signer = TestSigner::new([0x01; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));
    seed_multi_era_utxo(&mut state, &signer, [0x01; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_bytes(64));
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
        validity_interval_start: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted =
        MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux)));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("64-byte bytes metadatum should be accepted");
}

/// Text metadatum exactly 64 UTF-8 bytes — accepted.
#[test]
fn submitted_allegra_accepts_text_64() {
    let signer = TestSigner::new([0x03; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));
    seed_multi_era_utxo(&mut state, &signer, [0x03; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_text(64));
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x03; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
        validity_interval_start: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted =
        MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux)));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("64-byte text metadatum should be accepted");
}

/// Large integer metadatum — integers have no size restriction.
#[test]
fn submitted_allegra_accepts_large_integer() {
    let signer = TestSigner::new([0x09; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));
    seed_multi_era_utxo(&mut state, &signer, [0x09; 32], 5_000_000);

    // CBOR unsigned integer u64::MAX
    let int_cbor = vec![0x1B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let aux = shelley_metadata_with_value(1, &int_cbor);
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x09; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
        validity_interval_start: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted =
        MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux)));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("large integer metadatum should be accepted");
}

/// Empty bytes (0 length) — accepted.
#[test]
fn submitted_allegra_accepts_empty_bytes() {
    let signer = TestSigner::new([0x0E; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));
    seed_multi_era_utxo(&mut state, &signer, [0x0E; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_bytes(0));
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x0E; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
        validity_interval_start: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted =
        MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux)));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("0-length bytes metadatum should be accepted");
}

/// No auxiliary data at all — no metadata validation triggered.
#[test]
fn submitted_allegra_accepts_no_auxiliary_data() {
    let signer = TestSigner::new([0x0D; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));
    seed_multi_era_utxo(&mut state, &signer, [0x0D; 32], 5_000_000);

    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x0D; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted = MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(body, ws, None));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("no auxiliary data means no metadata validation");
}

// -----------------------------------------------------------------------
// Rejection tests — submitted-tx path
// -----------------------------------------------------------------------

/// Submitted Allegra tx with oversized bytes metadata — rejected.
#[test]
fn submitted_allegra_rejects_oversized_bytes_metadata() {
    let signer = TestSigner::new([0x0A; 32]);
    let mut state = LedgerState::new(Era::Allegra);
    state.set_protocol_params(permissive_params_pv(3, 0));
    seed_multi_era_utxo(&mut state, &signer, [0x0A; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_bytes(65));
    let body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x0A; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
        validity_interval_start: None,
    };
    let submitted = MultiEraSubmittedTx::Allegra(ShelleyCompatibleSubmittedTx::new(
        body,
        empty_witness_set(),
        Some(aux),
    ));
    let err = state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidMetadata),
        "submitted tx with 65-byte bytes should trigger InvalidMetadata, got: {err:?}",
    );
}

/// Submitted Conway tx with oversized text metadata — rejected.
#[test]
fn submitted_conway_rejects_oversized_text_metadata() {
    let signer = TestSigner::new([0x0C; 32]);
    let mut state = LedgerState::new(Era::Conway);
    let mut pp = permissive_params_pv(10, 0);
    pp.key_deposit = 2_000_000;
    pp.coins_per_utxo_byte = Some(4310);
    state.set_protocol_params(pp);
    seed_multi_era_utxo(&mut state, &signer, [0x0C; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_text(65));
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x0C; 32],
            index: 0,
        }],
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
        auxiliary_data_hash: Some(aux_hash(&aux)),
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
        true,
        Some(aux),
    ));
    let err = state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidMetadata),
        "Conway submitted tx with 65-byte text should trigger InvalidMetadata, got: {err:?}",
    );
}

// -----------------------------------------------------------------------
// PV 2.0 (Shelley) — soft fork NOT active, oversized metadata passes
// -----------------------------------------------------------------------

/// Protocol version 2.0 — `validMetadata` returns false, oversized bytes
/// are tolerated.
#[test]
fn submitted_shelley_pv2_allows_oversized_bytes() {
    let signer = TestSigner::new([0x0B; 32]);
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params_pv(2, 0));
    seed_shelley_utxo(&mut state, &signer, [0x0B; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_bytes(100));
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x0B; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted =
        MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux)));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("PV 2.0 should skip metadata size validation");
}

/// Protocol version 2.0 — oversized text is also tolerated.
#[test]
fn submitted_shelley_pv2_allows_oversized_text() {
    let signer = TestSigner::new([0x06; 32]);
    let mut state = LedgerState::new(Era::Shelley);
    state.set_protocol_params(permissive_params_pv(2, 0));
    seed_shelley_utxo(&mut state, &signer, [0x06; 32], 5_000_000);

    let aux = shelley_metadata_with_value(1, &cbor_text(100));
    let body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x06; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: signer.enterprise_addr(),
            amount: 5_000_000,
        }],
        fee: 0,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: Some(aux_hash(&aux)),
    };
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    let mut ws = empty_witness_set();
    ws.vkey_witnesses.push(signer.witness(&tx_body_hash));
    let submitted =
        MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(body, ws, Some(aux)));
    state
        .apply_submitted_tx(&submitted, SlotNo(10), None)
        .expect("PV 2.0 should skip metadata size validation for text");
}
