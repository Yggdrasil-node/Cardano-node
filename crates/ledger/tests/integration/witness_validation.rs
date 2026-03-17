//! Integration tests for VKey witness and native script validation
//! wired into the per-era `apply_block()` pipeline.

use super::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a 29-byte enterprise key-hash address (type 0x60, network 0).
fn enterprise_keyhash_address(keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60u8]; // enterprise, keyhash, network 0
    addr.extend_from_slice(keyhash);
    addr
}

/// Build a 29-byte enterprise script-hash address (type 0x70, network 0).
fn enterprise_scripthash_address(script_hash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x70u8]; // enterprise, scripthash, network 0
    addr.extend_from_slice(script_hash);
    addr
}

/// Serialise a `ShelleyWitnessSet` to CBOR bytes.
fn encode_witness_set(ws: &ShelleyWitnessSet) -> Vec<u8> {
    ws.to_cbor_bytes()
}

/// Construct a minimal `ShelleyWitnessSet` with only VKey witnesses.
fn witness_set_with_vkeys(vkeys: Vec<ShelleyVkeyWitness>) -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: vkeys,
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }
}

/// Construct a minimal `ShelleyWitnessSet` with VKey witnesses and native scripts.
fn witness_set_with_vkeys_and_scripts(
    vkeys: Vec<ShelleyVkeyWitness>,
    scripts: Vec<NativeScript>,
) -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: vkeys,
        native_scripts: scripts,
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }
}

/// A dummy Ed25519 signing key seed (32 bytes) for testing.
const TEST_SEED: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
    0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
];

/// A different signing key seed.
const WRONG_SEED: [u8; 32] = [0xFFu8; 32];

/// Derive the 32-byte VKey from a signing key seed.
fn test_vkey(seed: &[u8; 32]) -> [u8; 32] {
    let sk = yggdrasil_crypto::ed25519::SigningKey::from_bytes(*seed);
    sk.verification_key().unwrap().0
}

/// Sign a 32-byte message (tx body hash) with a signing key seed.
fn test_sign(seed: &[u8; 32], message: &[u8; 32]) -> [u8; 64] {
    let sk = yggdrasil_crypto::ed25519::SigningKey::from_bytes(*seed);
    sk.sign(message).unwrap().0
}

/// Create a VKey witness by signing tx body hash with the given seed.
fn make_witness(seed: &[u8; 32], tx_body_hash: &[u8; 32]) -> ShelleyVkeyWitness {
    ShelleyVkeyWitness {
        vkey: test_vkey(seed),
        signature: test_sign(seed, tx_body_hash),
    }
}

fn make_shelley_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Shelley,
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

fn make_allegra_block_raw(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Allegra,
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

fn make_conway_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<yggdrasil_ledger::Tx>) -> Block {
    Block {
        era: Era::Conway,
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

// ===========================================================================
// VKey witness validation tests
// ===========================================================================

#[test]
fn shelley_block_accepts_valid_vkey_witness() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    state.apply_block(&block).expect("valid vkey witness should pass");
}

#[test]
fn shelley_block_rejects_missing_vkey_witness() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Provide witness with WRONG vkey — its hash won't match the address keyhash.
    let ws = witness_set_with_vkeys(vec![make_witness(&WRONG_SEED, &tx_id_hash.0)]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::MissingVKeyWitness { hash } if hash == keyhash),
        "expected MissingVKeyWitness for keyhash, got: {err:?}"
    );
}

#[test]
fn shelley_block_skips_witness_check_when_no_witness_bytes() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // No witness bytes — validation is soft-skipped.
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    state.apply_block(&block).expect("no witnesses should pass (soft skip)");
}

#[test]
fn shelley_block_rejects_empty_witness_set_for_keyhash_input() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Provide witness set with zero vkey witnesses.
    let ws = witness_set_with_vkeys(vec![]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::MissingVKeyWitness { .. }),
        "expected MissingVKeyWitness, got: {err:?}"
    );
}

#[test]
fn conway_block_rejects_missing_voter_vkey_witness() {
    use std::collections::BTreeMap;

    let voter_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let mut votes = BTreeMap::new();
    votes.insert(
        GovActionId {
            transaction_id: [0x22; 32],
            gov_action_index: 0,
        },
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );

    let tx_body = ConwayTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: None,
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
        voting_procedures: Some(VotingProcedures {
            procedures: [(Voter::DRepKeyHash(voter_keyhash), votes)].into_iter().collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let mut state = LedgerState::new(Era::Conway);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xCC, vec![tx]))
        .unwrap_err();

    assert!(
        matches!(err, LedgerError::MissingVKeyWitness { hash } if hash == voter_keyhash),
        "expected MissingVKeyWitness for Conway voter, got: {err:?}"
    );
}

#[test]
fn conway_block_rejects_unregistered_proposal_return_account() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x44; 28]),
    };

    let tx_body = ConwayTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: None,
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
        proposal_procedures: Some(vec![ProposalProcedure {
            deposit: 1,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/proposal".to_owned(),
                data_hash: [0x45; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xCD, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::RewardAccountNotRegistered(reward_account));
}

#[test]
fn conway_block_rejects_treasury_withdrawals_proposal_with_unregistered_target_account() {
    use std::collections::BTreeMap;

    let proposal_return_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x46; 28]),
    };
    let treasury_target_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x47; 28]),
    };

    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(treasury_target_account, 1);

    let tx_body = ConwayTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: None,
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
        proposal_procedures: Some(vec![ProposalProcedure {
            deposit: 1,
            reward_account: proposal_return_account.to_bytes().to_vec(),
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/treasury".to_owned(),
                data_hash: [0x48; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(proposal_return_account.credential);
    state
        .reward_accounts_mut()
        .insert(proposal_return_account, RewardAccountState::new(0, None));

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xCE, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::RewardAccountNotRegistered(treasury_target_account));
}

#[test]
fn conway_block_accepts_proposal_return_account_registered_by_same_tx_certificate() {
    let credential = StakeCredential::AddrKeyHash([0x49; 28]);
    let reward_account = RewardAccount {
        network: 0,
        credential,
    };

    let tx_body = ConwayTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: None,
        certificates: Some(vec![DCert::AccountRegistrationDeposit(credential, 2_000_000)]),
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
        proposal_procedures: Some(vec![ProposalProcedure {
            deposit: 1,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/same-tx".to_owned(),
                data_hash: [0x4A; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xCF, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::NoInputs);
}

// ===========================================================================
// Native script validation tests (Allegra+)
// ===========================================================================

#[test]
fn allegra_block_validates_native_script_success() {
    // Create a NativeScript::ScriptPubkey that references TEST_SEED's VKey hash.
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let script = NativeScript::ScriptPubkey(keyhash);
    let script_hash = native_script_hash(&script);

    // Enterprise script-hash address so the input requires this script.
    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Witness set carries both the VKey (so ScriptPubkey evaluates true)
    // and the native script itself.
    let ws = witness_set_with_vkeys_and_scripts(
        vec![make_witness(&TEST_SEED, &tx_id_hash.0)],
        vec![script],
    );

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_allegra_block_raw(500, 1, 0xBB, vec![tx]);
    state.apply_block(&block).expect("native script should evaluate true");
}

#[test]
fn allegra_block_rejects_native_script_failure() {
    // NativeScript::InvalidBefore(1000) — requires slot >= 1000.
    let script = NativeScript::InvalidBefore(1000);
    let script_hash = native_script_hash(&script);

    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: Some(2000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Witness set carries the script but NO vkey needed (it's a timelock).
    let ws = witness_set_with_vkeys_and_scripts(vec![], vec![script]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    // Block slot is 500, but script requires slot >= 1000 → should fail.
    let block = make_allegra_block_raw(500, 1, 0xCC, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NativeScriptFailed { hash } if hash == script_hash),
        "expected NativeScriptFailed, got: {err:?}"
    );
}

#[test]
fn allegra_block_accepts_native_script_timelock_in_range() {
    // NativeScript::InvalidBefore(100) — requires slot >= 100.
    let script = NativeScript::InvalidBefore(100);
    let script_hash = native_script_hash(&script);

    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: Some(2000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    let ws = witness_set_with_vkeys_and_scripts(vec![], vec![script]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    // Block slot 500 >= 100 → script should evaluate true.
    let block = make_allegra_block_raw(500, 1, 0xDD, vec![tx]);
    state.apply_block(&block).expect("timelock should pass at slot 500");
}

#[test]
fn allegra_block_rejects_native_script_hereafter_exceeded() {
    // NativeScript::InvalidHereafter(200) — requires slot < 200.
    let script = NativeScript::InvalidHereafter(200);
    let script_hash = native_script_hash(&script);

    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: Some(500),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    let ws = witness_set_with_vkeys_and_scripts(vec![], vec![script]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    // Block slot 500 >= 200 → InvalidHereafter fails.
    let block = make_allegra_block_raw(500, 1, 0xEE, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NativeScriptFailed { hash } if hash == script_hash),
        "expected NativeScriptFailed for InvalidHereafter, got: {err:?}"
    );
}

#[test]
fn allegra_block_validates_multisig_all_script() {
    let keyhash1 = vkey_hash(&test_vkey(&TEST_SEED));
    let keyhash2 = vkey_hash(&test_vkey(&WRONG_SEED));

    // ScriptAll: both keyhashes must be present.
    let script = NativeScript::ScriptAll(vec![
        NativeScript::ScriptPubkey(keyhash1),
        NativeScript::ScriptPubkey(keyhash2),
    ]);
    let script_hash = native_script_hash(&script);
    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Both VKeys present → ScriptAll evaluates true.
    let ws = witness_set_with_vkeys_and_scripts(
        vec![
            make_witness(&TEST_SEED, &tx_id_hash.0),
            make_witness(&WRONG_SEED, &tx_id_hash.0),
        ],
        vec![script],
    );

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_allegra_block_raw(500, 1, 0xA1, vec![tx]);
    state.apply_block(&block).expect("ScriptAll with both vkeys should pass");
}

#[test]
fn allegra_block_rejects_multisig_all_missing_one_vkey() {
    let keyhash1 = vkey_hash(&test_vkey(&TEST_SEED));
    let keyhash2 = vkey_hash(&test_vkey(&WRONG_SEED));

    let script = NativeScript::ScriptAll(vec![
        NativeScript::ScriptPubkey(keyhash1),
        NativeScript::ScriptPubkey(keyhash2),
    ]);
    let script_hash = native_script_hash(&script);
    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: Some(1000),
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Only one VKey present → ScriptAll fails.
    let ws = witness_set_with_vkeys_and_scripts(
        vec![make_witness(&TEST_SEED, &tx_id_hash.0)],
        vec![script],
    );

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_allegra_block_raw(500, 1, 0xA2, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::NativeScriptFailed { hash } if hash == script_hash),
        "expected NativeScriptFailed for ScriptAll with missing key, got: {err:?}"
    );
}

// ===========================================================================
// Ed25519 signature verification tests
// ===========================================================================

#[test]
fn shelley_block_rejects_forged_signature() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Correct VKey but signature produced by a different key — forged.
    let forged_sig = test_sign(&WRONG_SEED, &tx_id_hash.0);
    let ws = witness_set_with_vkeys(vec![ShelleyVkeyWitness {
        vkey: test_vkey(&TEST_SEED),
        signature: forged_sig,
    }]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidVKeyWitnessSignature { hash } if hash == keyhash),
        "expected InvalidVKeyWitnessSignature for forged sig, got: {err:?}"
    );
}

#[test]
fn shelley_block_rejects_signature_on_wrong_body() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x01; 32], index: 0 },
        ShelleyTxOut { address: addr.clone(), amount: 5_000_000 },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        outputs: vec![ShelleyTxOut { address: addr, amount: 4_800_000 }],
        fee: 200_000,
        ttl: 1000,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);

    // Sign a different message (all zeros) — signature will not verify
    // against the actual tx body hash.
    let wrong_message = [0u8; 32];
    let bad_sig = test_sign(&TEST_SEED, &wrong_message);
    let ws = witness_set_with_vkeys(vec![ShelleyVkeyWitness {
        vkey: test_vkey(&TEST_SEED),
        signature: bad_sig,
    }]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidVKeyWitnessSignature { hash } if hash == keyhash),
        "expected InvalidVKeyWitnessSignature for wrong-body sig, got: {err:?}"
    );
}
