//! Integration tests for Phase-2 Plutus script evaluation wired through
//! `apply_submitted_tx()` with mock `PlutusEvaluator` implementations.
//!
//! These tests mirror the block-path tests in `plutus_evaluation.rs` but
//! exercise the **submitted-transaction** path where `is_valid` is always
//! `true` and any Phase-2 failure is a hard rejection (no
//! ValidationTagMismatch bifurcation).

use super::*;
use std::collections::BTreeMap;
use yggdrasil_ledger::plutus_validation::{
    PlutusEvaluator, PlutusScriptEval, PlutusVersion, TxContext, plutus_script_hash,
};

// ---------------------------------------------------------------------------
// Mock evaluators (same as plutus_evaluation.rs)
// ---------------------------------------------------------------------------

struct AlwaysSucceeds;

impl PlutusEvaluator for AlwaysSucceeds {
    fn evaluate(&self, _eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
        Ok(())
    }
}

struct AlwaysFails;

impl PlutusEvaluator for AlwaysFails {
    fn evaluate(&self, eval: &PlutusScriptEval, _tx_ctx: &TxContext) -> Result<(), LedgerError> {
        Err(LedgerError::PlutusScriptFailed {
            hash: eval.script_hash,
            reason: "mock: script always fails".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TEST_SEED: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
    0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
];

fn test_vkey(seed: &[u8; 32]) -> [u8; 32] {
    let sk = yggdrasil_crypto::ed25519::SigningKey::from_bytes(*seed);
    sk.verification_key().unwrap().0
}

fn test_sign(seed: &[u8; 32], message: &[u8; 32]) -> [u8; 64] {
    let sk = yggdrasil_crypto::ed25519::SigningKey::from_bytes(*seed);
    sk.sign(message).unwrap().0
}

fn make_witness(seed: &[u8; 32], tx_body_hash: &[u8; 32]) -> ShelleyVkeyWitness {
    ShelleyVkeyWitness {
        vkey: test_vkey(seed),
        signature: test_sign(seed, tx_body_hash),
    }
}

fn enterprise_keyhash_address(keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60u8];
    addr.extend_from_slice(keyhash);
    addr
}

const DUMMY_SCRIPT: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];

fn minting_redeemer(steps: u64, mem: u64) -> Redeemer {
    Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem, steps },
    }
}

/// Compute the script_data_hash for a minting tx with the given Plutus script.
fn compute_minting_sdh(
    script_bytes: &[u8],
    version: PlutusVersion,
    pp: Option<&ProtocolParameters>,
    conway: bool,
) -> [u8; 32] {
    let redeemer = minting_redeemer(1_000_000, 500_000);
    let (v1, v2, v3) = match version {
        PlutusVersion::V1 => (vec![script_bytes.to_vec()], vec![], vec![]),
        PlutusVersion::V2 => (vec![], vec![script_bytes.to_vec()], vec![]),
        PlutusVersion::V3 => (vec![], vec![], vec![script_bytes.to_vec()]),
    };
    let ws = ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: v1,
        plutus_data: vec![],
        redeemers: vec![redeemer],
        plutus_v2_scripts: v2,
        plutus_v3_scripts: v3,
    };
    compute_test_script_data_hash(&ws, pp, conway)
}

/// Build a minting `AlonzoTxBody` + script_hash + tx_id.
fn build_alonzo_minting_body(
    prev_tx_id: [u8; 32],
    script_bytes: &[u8],
    version: PlutusVersion,
    script_data_hash: Option<[u8; 32]>,
) -> (AlonzoTxBody, [u8; 28], [u8; 32]) {
    let script_hash = plutus_script_hash(version, script_bytes);
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"token".to_vec(), 1i64);
    mint.insert(script_hash, assets);
    let mut output_assets = BTreeMap::new();
    let mut output_policy_assets = BTreeMap::new();
    output_policy_assets.insert(b"token".to_vec(), 1u64);
    output_assets.insert(script_hash, output_policy_assets);

    let body = AlonzoTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![AlonzoTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_hash: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
        script_data_hash,
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    (body, script_hash, tx_id)
}

fn build_minting_witness_set(
    script_bytes: &[u8],
    tx_body_hash: &[u8; 32],
    version: PlutusVersion,
) -> ShelleyWitnessSet {
    let vkey_witness = make_witness(&TEST_SEED, tx_body_hash);
    let redeemer = minting_redeemer(1_000_000, 500_000);

    let (v1, v2, v3) = match version {
        PlutusVersion::V1 => (vec![script_bytes.to_vec()], vec![], vec![]),
        PlutusVersion::V2 => (vec![], vec![script_bytes.to_vec()], vec![]),
        PlutusVersion::V3 => (vec![], vec![], vec![script_bytes.to_vec()]),
    };

    ShelleyWitnessSet {
        vkey_witnesses: vec![vkey_witness],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: v1,
        plutus_data: vec![],
        redeemers: vec![redeemer],
        plutus_v2_scripts: v2,
        plutus_v3_scripts: v3,
    }
}

fn seed_alonzo_utxo(state: &mut LedgerState, prev_tx_id: [u8; 32]) {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let txin = ShelleyTxIn { transaction_id: prev_tx_id, index: 0 };
    let txout = MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: addr,
        amount: Value::Coin(5_000_000),
        datum_hash: None,
    });
    state.multi_era_utxo_mut().insert(txin, txout);
}

fn seed_babbage_utxo(state: &mut LedgerState, prev_tx_id: [u8; 32]) {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let txin = ShelleyTxIn { transaction_id: prev_tx_id, index: 0 };
    let txout = MultiEraTxOut::Babbage(BabbageTxOut {
        address: addr,
        amount: Value::Coin(5_000_000),
        datum_option: None,
        script_ref: None,
    });
    state.multi_era_utxo_mut().insert(txin, txout);
}

// ===========================================================================
// Alonzo submitted — evaluator succeeds
// ===========================================================================

#[test]
fn alonzo_submitted_plutus_v1_evaluator_succeeds() {
    let prev_tx_id = [0xE1; 32];
    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V1, None, false);
    let (body, _script_hash, tx_id) = build_alonzo_minting_body(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1, Some(sdh));
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Alonzo);
    seed_alonzo_utxo(&mut state, prev_tx_id);

    let evaluator = AlwaysSucceeds;
    let result = state.apply_submitted_tx(&submitted, SlotNo(100), Some(&evaluator));
    assert!(result.is_ok(), "Alonzo submitted Plutus V1 minting should succeed: {:?}", result);
}

// ===========================================================================
// Alonzo submitted — evaluator fails → hard reject (PlutusScriptFailed)
// ===========================================================================

#[test]
fn alonzo_submitted_plutus_v1_evaluator_fails() {
    let prev_tx_id = [0xE2; 32];
    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V1, None, false);
    let (body, script_hash, tx_id) = build_alonzo_minting_body(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1, Some(sdh));
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Alonzo);
    seed_alonzo_utxo(&mut state, prev_tx_id);

    let evaluator = AlwaysFails;
    let result = state.apply_submitted_tx(&submitted, SlotNo(100), Some(&evaluator));
    assert!(
        matches!(
            result,
            Err(LedgerError::PlutusScriptFailed { hash, .. }) if hash == script_hash
        ),
        "expected PlutusScriptFailed for submitted tx, got: {:?}",
        result,
    );
}

// ===========================================================================
// Alonzo submitted — no evaluator (None) → soft-skip (backward compat)
// ===========================================================================

#[test]
fn alonzo_submitted_plutus_no_evaluator_soft_skip() {
    let prev_tx_id = [0xE3; 32];
    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V1, None, false);
    let (body, _script_hash, tx_id) = build_alonzo_minting_body(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1, Some(sdh));
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Alonzo);
    seed_alonzo_utxo(&mut state, prev_tx_id);

    // No evaluator — Phase-2 validation is skipped.
    let result = state.apply_submitted_tx(&submitted, SlotNo(100), None);
    assert!(result.is_ok(), "Alonzo submitted with no evaluator should soft-skip: {:?}", result);
}

// ===========================================================================
// Babbage submitted — evaluator succeeds (V2 minting)
// ===========================================================================

#[test]
fn babbage_submitted_plutus_v2_evaluator_succeeds() {
    let prev_tx_id = [0xE4; 32];
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let script_hash = plutus_script_hash(PlutusVersion::V2, DUMMY_SCRIPT);

    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"nft".to_vec(), 1i64);
    mint.insert(script_hash, assets);
    let mut output_assets = BTreeMap::new();
    let mut output_policy_assets = BTreeMap::new();
    output_policy_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(script_hash, output_policy_assets);

    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V2, None, false);
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
        script_data_hash: Some(sdh),
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V2);

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Babbage);
    seed_babbage_utxo(&mut state, prev_tx_id);

    let evaluator = AlwaysSucceeds;
    let result = state.apply_submitted_tx(&submitted, SlotNo(200), Some(&evaluator));
    assert!(result.is_ok(), "Babbage submitted Plutus V2 minting should succeed: {:?}", result);
}

// ===========================================================================
// Babbage submitted — evaluator fails → hard reject
// ===========================================================================

#[test]
fn babbage_submitted_plutus_v2_evaluator_fails() {
    let prev_tx_id = [0xE5; 32];
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let script_hash = plutus_script_hash(PlutusVersion::V2, DUMMY_SCRIPT);

    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"nft".to_vec(), 1i64);
    mint.insert(script_hash, assets);
    let mut output_assets = BTreeMap::new();
    let mut output_policy_assets = BTreeMap::new();
    output_policy_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(script_hash, output_policy_assets);

    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V2, None, false);
    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
        script_data_hash: Some(sdh),
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    };

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V2);

    let submitted = MultiEraSubmittedTx::Babbage(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Babbage);
    seed_babbage_utxo(&mut state, prev_tx_id);

    let evaluator = AlwaysFails;
    let result = state.apply_submitted_tx(&submitted, SlotNo(200), Some(&evaluator));
    assert!(
        matches!(
            result,
            Err(LedgerError::PlutusScriptFailed { hash, .. }) if hash == script_hash
        ),
        "expected PlutusScriptFailed for Babbage submitted tx, got: {:?}",
        result,
    );
}

// ===========================================================================
// Conway submitted — evaluator succeeds (V3 minting)
// ===========================================================================

#[test]
fn conway_submitted_plutus_v3_evaluator_succeeds() {
    let prev_tx_id = [0xE6; 32];
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let script_hash = plutus_script_hash(PlutusVersion::V3, DUMMY_SCRIPT);

    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"nft".to_vec(), 1i64);
    mint.insert(script_hash, assets);
    let mut output_assets = BTreeMap::new();
    let mut output_policy_assets = BTreeMap::new();
    output_policy_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(script_hash, output_policy_assets);

    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V3, None, true);
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
        script_data_hash: Some(sdh),
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

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V3);

    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Conway);
    seed_babbage_utxo(&mut state, prev_tx_id); // Conway uses Babbage-shaped UTxO

    let evaluator = AlwaysSucceeds;
    let result = state.apply_submitted_tx(&submitted, SlotNo(300), Some(&evaluator));
    assert!(result.is_ok(), "Conway submitted Plutus V3 minting should succeed: {:?}", result);
}

// ===========================================================================
// Conway submitted — evaluator fails → hard reject
// ===========================================================================

#[test]
fn conway_submitted_plutus_v3_evaluator_fails() {
    let prev_tx_id = [0xE7; 32];
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let script_hash = plutus_script_hash(PlutusVersion::V3, DUMMY_SCRIPT);

    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"nft".to_vec(), 1i64);
    mint.insert(script_hash, assets);
    let mut output_assets = BTreeMap::new();
    let mut output_policy_assets = BTreeMap::new();
    output_policy_assets.insert(b"nft".to_vec(), 1u64);
    output_assets.insert(script_hash, output_policy_assets);

    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V3, None, true);
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: addr,
            amount: Value::CoinAndAssets(4_800_000, output_assets),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: Some(mint),
        script_data_hash: Some(sdh),
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

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V3);

    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let mut state = LedgerState::new(Era::Conway);
    seed_babbage_utxo(&mut state, prev_tx_id);

    let evaluator = AlwaysFails;
    let result = state.apply_submitted_tx(&submitted, SlotNo(300), Some(&evaluator));
    assert!(
        matches!(
            result,
            Err(LedgerError::PlutusScriptFailed { hash, .. }) if hash == script_hash
        ),
        "expected PlutusScriptFailed for Conway submitted tx, got: {:?}",
        result,
    );
}

// ===========================================================================
// Submitted tx with is_valid = false is rejected before Plutus evaluation
// ===========================================================================

#[test]
fn submitted_tx_is_valid_false_rejected_before_plutus() {
    let prev_tx_id = [0xE8; 32];
    let sdh = compute_minting_sdh(DUMMY_SCRIPT, PlutusVersion::V1, None, false);
    let (body, _script_hash, tx_id) = build_alonzo_minting_body(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1, Some(sdh));
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    // is_valid = false — should be rejected immediately
    let submitted = MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        body, ws, false, None,
    ));

    let mut state = LedgerState::new(Era::Alonzo);
    seed_alonzo_utxo(&mut state, prev_tx_id);

    let evaluator = AlwaysSucceeds;
    let result = state.apply_submitted_tx(&submitted, SlotNo(100), Some(&evaluator));
    assert!(
        matches!(result, Err(LedgerError::SubmittedTxIsInvalid)),
        "expected SubmittedTxIsInvalid, got: {:?}",
        result,
    );
}
