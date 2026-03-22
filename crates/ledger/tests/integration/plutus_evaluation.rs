//! Integration tests for Plutus script evaluation wired through
//! `apply_block_validated()` with mock `PlutusEvaluator` implementations.

use super::*;
use std::collections::BTreeMap;
use yggdrasil_ledger::plutus_validation::{PlutusEvaluator, PlutusScriptEval, PlutusVersion, plutus_script_hash};

// ---------------------------------------------------------------------------
// Mock evaluators
// ---------------------------------------------------------------------------

/// An evaluator that always succeeds.
struct AlwaysSucceeds;

impl PlutusEvaluator for AlwaysSucceeds {
    fn evaluate(&self, _eval: &PlutusScriptEval) -> Result<(), LedgerError> {
        Ok(())
    }
}

/// An evaluator that always fails with a script error.
struct AlwaysFails;

impl PlutusEvaluator for AlwaysFails {
    fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError> {
        Err(LedgerError::PlutusScriptFailed {
            hash: eval.script_hash,
            reason: "mock: script always fails".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Dummy signing seed for Ed25519.
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

fn encode_witness_set(ws: &ShelleyWitnessSet) -> Vec<u8> {
    ws.to_cbor_bytes()
}

fn make_block(era: Era, slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
        },
        transactions: txs,
        raw_cbor: None,
    }
}

/// Dummy Plutus script bytes (arbitrary, doesn't need to be valid Flat).
const DUMMY_SCRIPT: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04];

/// Build a minimal minting redeemer (tag=1, index=0) with empty PlutusData.
fn minting_redeemer(steps: u64, mem: u64) -> Redeemer {
    Redeemer {
        tag: 1,
        index: 0,
        data: PlutusData::Integer(0),
        ex_units: ExUnits { mem, steps },
    }
}

/// Build an Alonzo transaction that mints under a Plutus V1 policy.
///
/// The tx spends a pre-seeded key-hash input and mints 1 token under
/// the given Plutus V1 script's policy ID (= script hash).
fn build_alonzo_minting_tx(
    prev_tx_id: [u8; 32],
    script_bytes: &[u8],
    version: PlutusVersion,
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
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
    };

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    (body, script_hash, tx_id)
}

/// Build the witness set for a minting tx with a Plutus script and a VKey witness.
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

/// Seed a LedgerState with a key-hash input at the given tx_id:0.
fn seed_alonzo_state(prev_tx_id: [u8; 32]) -> LedgerState {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);
    let txin = ShelleyTxIn { transaction_id: prev_tx_id, index: 0 };
    let txout = MultiEraTxOut::Alonzo(AlonzoTxOut {
        address: addr,
        amount: Value::Coin(5_000_000),
        datum_hash: None,
    });
    let mut state = LedgerState::new(Era::Alonzo);
    state.multi_era_utxo_mut().insert(txin, txout);
    state
}

// ===========================================================================
// Alonzo Plutus V1 minting — evaluator succeeds
// ===========================================================================

#[test]
fn alonzo_plutus_v1_minting_evaluator_succeeds() {
    let prev_tx_id = [0xAA; 32];
    let (body, _script_hash, tx_id) = build_alonzo_minting_tx(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1);
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id),
        body: body.to_cbor_bytes(),
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
    };
    let block = make_block(Era::Alonzo, 100, 1, 0x01, vec![tx]);

    let mut state = seed_alonzo_state(prev_tx_id);
    let evaluator = AlwaysSucceeds;
    let result = state.apply_block_validated(&block, Some(&evaluator));
    assert!(result.is_ok(), "apply_block_validated should succeed: {:?}", result);
}

// ===========================================================================
// Alonzo Plutus V1 minting — evaluator fails → LedgerError
// ===========================================================================

#[test]
fn alonzo_plutus_v1_minting_evaluator_fails() {
    let prev_tx_id = [0xBB; 32];
    let (body, script_hash, tx_id) = build_alonzo_minting_tx(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1);
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id),
        body: body.to_cbor_bytes(),
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
    };
    let block = make_block(Era::Alonzo, 100, 1, 0x02, vec![tx]);

    let mut state = seed_alonzo_state(prev_tx_id);
    let evaluator = AlwaysFails;
    let result = state.apply_block_validated(&block, Some(&evaluator));
    assert!(result.is_err(), "apply_block_validated should fail");

    match result.unwrap_err() {
        LedgerError::PlutusScriptFailed { hash, reason } => {
            assert_eq!(hash, script_hash);
            assert!(reason.contains("always fails"));
        }
        other => panic!("unexpected error: {:?}", other),
    }
}

// ===========================================================================
// Alonzo Plutus V1 minting — no evaluator (soft skip)
// ===========================================================================

#[test]
fn alonzo_plutus_v1_minting_no_evaluator_skips() {
    let prev_tx_id = [0xCC; 32];
    let (body, _script_hash, tx_id) = build_alonzo_minting_tx(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1);
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id),
        body: body.to_cbor_bytes(),
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
    };
    let block = make_block(Era::Alonzo, 100, 1, 0x03, vec![tx]);

    let mut state = seed_alonzo_state(prev_tx_id);
    // apply_block() delegates to apply_block_validated(block, None) — no evaluator.
    let result = state.apply_block(&block);
    assert!(result.is_ok(), "apply_block (no evaluator) should soft-skip Plutus: {:?}", result);
}

// ===========================================================================
// Babbage Plutus V2 minting — evaluator succeeds
// ===========================================================================

#[test]
fn babbage_plutus_v2_minting_evaluator_succeeds() {
    let prev_tx_id = [0xDD; 32];
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

    let body = BabbageTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
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
        script_data_hash: None,
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

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
    };
    let block = make_block(Era::Babbage, 200, 2, 0x04, vec![tx]);

    let txin = ShelleyTxIn { transaction_id: prev_tx_id, index: 0 };
    let txout = MultiEraTxOut::Babbage(BabbageTxOut {
        address: addr,
        amount: Value::Coin(5_000_000),
        datum_option: None,
        script_ref: None,
    });
    let mut state = LedgerState::new(Era::Babbage);
    state.multi_era_utxo_mut().insert(txin, txout);

    let evaluator = AlwaysSucceeds;
    let result = state.apply_block_validated(&block, Some(&evaluator));
    assert!(result.is_ok(), "Babbage Plutus V2 minting should succeed: {:?}", result);
}

// ===========================================================================
// Conway Plutus V3 minting — evaluator succeeds
// ===========================================================================

#[test]
fn conway_plutus_v3_minting_evaluator_succeeds() {
    let prev_tx_id = [0xEE; 32];
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let script_hash = plutus_script_hash(PlutusVersion::V3, DUMMY_SCRIPT);

    let mut mint = BTreeMap::new();
    let mut assets = BTreeMap::new();
    assets.insert(b"gov".to_vec(), 1i64);
    mint.insert(script_hash, assets);
    let mut output_assets = BTreeMap::new();
    let mut output_policy_assets = BTreeMap::new();
    output_policy_assets.insert(b"gov".to_vec(), 1u64);
    output_assets.insert(script_hash, output_policy_assets);

    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: prev_tx_id, index: 0 }],
        outputs: vec![BabbageTxOut {
            address: addr.clone(),
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

    let body_bytes = body.to_cbor_bytes();
    let tx_id = yggdrasil_crypto::hash_bytes_256(&body_bytes).0;
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V3);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
    };
    let block = make_block(Era::Conway, 300, 3, 0x05, vec![tx]);

    let txin = ShelleyTxIn { transaction_id: prev_tx_id, index: 0 };
    let txout = MultiEraTxOut::Babbage(BabbageTxOut {
        address: addr,
        amount: Value::Coin(5_000_000),
        datum_option: None,
        script_ref: None,
    });
    let mut state = LedgerState::new(Era::Conway);
    state.multi_era_utxo_mut().insert(txin, txout);

    let evaluator = AlwaysSucceeds;
    let result = state.apply_block_validated(&block, Some(&evaluator));
    assert!(result.is_ok(), "Conway Plutus V3 minting should succeed: {:?}", result);
}

// ===========================================================================
// Alonzo Plutus V1 minting — evaluator receives correct version + script hash
// ===========================================================================

/// An evaluator that captures the script hash and version for assertion.
struct AssertEvaluator {
    expected_hash: [u8; 28],
    expected_version: PlutusVersion,
}

impl PlutusEvaluator for AssertEvaluator {
    fn evaluate(&self, eval: &PlutusScriptEval) -> Result<(), LedgerError> {
        assert_eq!(eval.script_hash, self.expected_hash, "script hash mismatch");
        assert_eq!(eval.version, self.expected_version, "version mismatch");
        assert_eq!(eval.script_bytes, DUMMY_SCRIPT, "script bytes mismatch");
        assert!(eval.ex_units.steps != 0 || eval.ex_units.mem != 0,
            "ex_units should have non-zero budget");
        Ok(())
    }
}

#[test]
fn alonzo_evaluator_receives_correct_script_metadata() {
    let prev_tx_id = [0xFF; 32];
    let script_hash = plutus_script_hash(PlutusVersion::V1, DUMMY_SCRIPT);
    let (body, _, tx_id) = build_alonzo_minting_tx(prev_tx_id, DUMMY_SCRIPT, PlutusVersion::V1);
    let ws = build_minting_witness_set(DUMMY_SCRIPT, &tx_id, PlutusVersion::V1);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id),
        body: body.to_cbor_bytes(),
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
    };
    let block = make_block(Era::Alonzo, 100, 1, 0x06, vec![tx]);

    let mut state = seed_alonzo_state(prev_tx_id);
    let evaluator = AssertEvaluator {
        expected_hash: script_hash,
        expected_version: PlutusVersion::V1,
    };
    let result = state.apply_block_validated(&block, Some(&evaluator));
    assert!(result.is_ok(), "evaluator assertions should pass: {:?}", result);
}
