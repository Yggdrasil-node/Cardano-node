//! Integration tests for VKey witness and native script validation
//! wired into the per-era `apply_block()` pipeline.

use super::*;
use yggdrasil_ledger::{GovernanceActionState, ProtocolParameters};

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
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
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

fn make_shelley_block(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<yggdrasil_ledger::Tx>,
) -> Block {
    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn make_allegra_block_raw(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<yggdrasil_ledger::Tx>,
) -> Block {
    Block {
        era: Era::Allegra,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn make_conway_block(
    slot: u64,
    block_no: u64,
    hash_seed: u8,
    txs: Vec<yggdrasil_ledger::Tx>,
) -> Block {
    Block {
        era: Era::Conway,
        header: BlockHeader {
            hash: HeaderHash([hash_seed; 32]),
            prev_hash: HeaderHash([0u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [0x11; 32],
            protocol_version: None,
        },
        transactions: txs,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn conway_bootstrap_protocol_params() -> ProtocolParameters {
    let mut params = ProtocolParameters::default();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.protocol_version = Some((9, 0));
    params
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    state
        .apply_block(&block)
        .expect("valid vkey witness should pass");
}

#[test]
fn shelley_block_rejects_missing_vkey_witness() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    state
        .apply_block(&block)
        .expect("no witnesses should pass (soft skip)");
}

#[test]
fn shelley_block_rejects_empty_witness_set_for_keyhash_input() {
    let keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let addr = enterprise_keyhash_address(&keyhash);

    let mut state = LedgerState::new(Era::Shelley);
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
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
            procedures: [(Voter::DRepKeyHash(voter_keyhash), votes)]
                .into_iter()
                .collect(),
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
        auxiliary_data: None,
        is_valid: None,
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xCD, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ProposalReturnAccountDoesNotExist(reward_account)
    );
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
        auxiliary_data: None,
        is_valid: None,
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

    assert_eq!(
        err,
        LedgerError::TreasuryWithdrawalReturnAccountsDoNotExist(vec![treasury_target_account])
    );
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
        certificates: Some(vec![DCert::AccountRegistrationDeposit(
            credential, 2_000_000,
        )]),
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    state.set_protocol_params(pp);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xCF, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn conway_block_rejects_incorrect_proposal_deposit() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x4A; 28]),
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
                url: "https://example.invalid/deposit-mismatch".to_owned(),
                data_hash: [0x4B; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(0, None));

    let mut params = ProtocolParameters::alonzo_defaults();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.gov_action_deposit = Some(2);
    state.set_protocol_params(params);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD0, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ProposalDepositIncorrect {
            supplied: 1,
            expected: 2,
        }
    );
}

#[test]
fn conway_block_accepts_matching_proposal_deposit() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x4C; 28]),
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
            deposit: 2,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/deposit-match".to_owned(),
                data_hash: [0x4D; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .reward_accounts_mut()
        .insert(reward_account, RewardAccountState::new(0, None));

    let mut params = ProtocolParameters::alonzo_defaults();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.gov_action_deposit = Some(2);
    state.set_protocol_params(params);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD1, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn conway_block_rejects_same_tx_forward_prev_governance_action_reference() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x4E; 28]),
    };
    let tx_id = TxId([0x51; 32]);
    let invalid_proposal = ProposalProcedure {
        deposit: 1,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::NoConfidence {
            prev_action_id: Some(GovActionId {
                transaction_id: tx_id.0,
                gov_action_index: 1,
            }),
        },
        anchor: Anchor {
            url: "https://example.invalid/invalid-prev-action".to_owned(),
            data_hash: [0x52; 32],
        },
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
        proposal_procedures: Some(vec![
            invalid_proposal.clone(),
            ProposalProcedure {
                deposit: 1,
                reward_account: reward_account.to_bytes().to_vec(),
                gov_action: GovAction::InfoAction,
                anchor: Anchor {
                    url: "https://example.invalid/later-proposal".to_owned(),
                    data_hash: [0x53; 32],
                },
            },
        ]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: tx_id,
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD2, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::InvalidPrevGovActionId(invalid_proposal));
}

#[test]
fn conway_block_rejects_first_hard_fork_that_cannot_follow_current_protocol_version() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x60; 28]),
    };
    let invalid_proposal = ProposalProcedure {
        deposit: 1,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 2),
        },
        anchor: Anchor {
            url: "https://example.invalid/hard-fork-cant-follow-current".to_owned(),
            data_hash: [0x61; 32],
        },
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
        proposal_procedures: Some(vec![invalid_proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    let mut params = ProtocolParameters::alonzo_defaults();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.protocol_version = Some((10, 0));
    state.set_protocol_params(params);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0x62, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ProposalCantFollow {
            prev_action_id: None,
            supplied: (10, 2),
            expected: (10, 0),
        }
    );
}

#[test]
fn conway_block_rejects_chained_hard_fork_that_cannot_follow_previous_proposal() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x63; 28]),
    };
    let previous_action_id = GovActionId {
        transaction_id: [0x64; 32],
        gov_action_index: 0,
    };
    let invalid_proposal = ProposalProcedure {
        deposit: 1,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: Some(previous_action_id.clone()),
            protocol_version: (10, 3),
        },
        anchor: Anchor {
            url: "https://example.invalid/hard-fork-cant-follow-previous".to_owned(),
            data_hash: [0x65; 32],
        },
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
        proposal_procedures: Some(vec![invalid_proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state.governance_actions_mut().insert(
        previous_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 1),
            },
            anchor: Anchor {
                url: "https://example.invalid/previous-hard-fork".to_owned(),
                data_hash: [0x66; 32],
            },
        }),
    );

    let mut params = ProtocolParameters::alonzo_defaults();
    params.min_fee_a = 0;
    params.min_fee_b = 0;
    params.protocol_version = Some((10, 0));
    state.set_protocol_params(params);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0x67, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ProposalCantFollow {
            prev_action_id: Some(previous_action_id),
            supplied: (10, 3),
            expected: (10, 1),
        }
    );
}

#[test]
fn conway_block_rejects_malformed_empty_parameter_change_proposal() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x53; 28]),
    };
    let malformed_action = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: ProtocolParameterUpdate::default(),
        guardrails_script_hash: None,
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
            gov_action: malformed_action.clone(),
            anchor: Anchor {
                url: "https://example.invalid/malformed-parameter-change".to_owned(),
                data_hash: [0x54; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x55; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD2, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::MalformedProposal(malformed_action));
}

#[test]
fn conway_block_rejects_non_bootstrap_proposal_during_bootstrap() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x56; 28]),
    };
    let disallowed_proposal = ProposalProcedure {
        deposit: 1,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::NewConstitution {
            prev_action_id: None,
            constitution: Constitution {
                anchor: Anchor {
                    url: "https://example.invalid/bootstrap-constitution".to_owned(),
                    data_hash: [0x57; 32],
                },
                guardrails_script_hash: None,
            },
        },
        anchor: Anchor {
            url: "https://example.invalid/disallowed-bootstrap-proposal".to_owned(),
            data_hash: [0x58; 32],
        },
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
        proposal_procedures: Some(vec![disallowed_proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x59; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD3, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedProposalDuringBootstrap(disallowed_proposal)
    );
}

#[test]
fn conway_block_accepts_allowed_hard_fork_proposal_during_bootstrap() {
    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x5B; 28]),
    };

    let proposal = ProposalProcedure {
        deposit: 0,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        anchor: Anchor {
            url: "https://example.invalid/allowed-bootstrap-hardfork".to_owned(),
            data_hash: [0x5C; 32],
        },
    };

    let tx_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x5D; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
        proposal_procedures: Some(vec![proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x5D; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    state
        .apply_block(&make_conway_block(500, 1, 0xD3, vec![tx]))
        .expect("allowed bootstrap proposal should succeed");

    let gov_action_id = GovActionId {
        transaction_id: tx_id_hash.0,
        gov_action_index: 0,
    };
    let stored_action = state
        .governance_action(&gov_action_id)
        .expect("bootstrap hard fork proposal stored");
    assert_eq!(stored_action.proposal(), &proposal);
}

#[test]
fn conway_block_accepts_allowed_parameter_change_proposal_during_bootstrap() {
    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x5E; 28]),
    };

    let proposal = ProposalProcedure {
        deposit: 0,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: ProtocolParameterUpdate {
                min_fee_a: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.invalid/allowed-bootstrap-pparam".to_owned(),
            data_hash: [0x5F; 32],
        },
    };

    let tx_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x60; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
        proposal_procedures: Some(vec![proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x60; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    state
        .apply_block(&make_conway_block(500, 1, 0xD4, vec![tx]))
        .expect("allowed bootstrap parameter-change proposal should succeed");

    let gov_action_id = GovActionId {
        transaction_id: tx_id_hash.0,
        gov_action_index: 0,
    };
    let stored_action = state
        .governance_action(&gov_action_id)
        .expect("bootstrap parameter-change proposal stored");
    assert_eq!(stored_action.proposal(), &proposal);
}

#[test]
fn conway_block_rejects_drep_non_info_vote_during_bootstrap() {
    use std::collections::BTreeMap;

    let drep_keyhash = [0x5A; 28];
    let gov_action_id = GovActionId {
        transaction_id: [0x5B; 32],
        gov_action_index: 0,
    };
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x5C; 28]),
    };

    let mut votes = BTreeMap::new();
    votes.insert(
        gov_action_id.clone(),
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
            procedures: [(Voter::DRepKeyHash(drep_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x5D; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));
    state.governance_actions_mut().insert(
        gov_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/bootstrap-parameter-change".to_owned(),
                data_hash: [0x5E; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD4, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedVotesDuringBootstrap(vec![(
            Voter::DRepKeyHash(drep_keyhash),
            gov_action_id,
        )])
    );
}

#[test]
fn bootstrap_witness_signature_verifies() {
    let tx_body_hash = [0x42; 32];
    let witness = BootstrapWitness {
        public_key: test_vkey(&TEST_SEED),
        signature: test_sign(&TEST_SEED, &tx_body_hash),
        chain_code: [0x11; 32],
        attributes: vec![0xA0],
    };

    yggdrasil_ledger::witnesses::verify_bootstrap_witnesses(&tx_body_hash, &[witness])
        .expect("valid bootstrap witness");
}

#[test]
fn bootstrap_witness_rejects_bad_signature() {
    let tx_body_hash = [0x43; 32];
    let witness = BootstrapWitness {
        public_key: test_vkey(&TEST_SEED),
        signature: test_sign(&WRONG_SEED, &tx_body_hash),
        chain_code: [0x22; 32],
        attributes: vec![0xA0],
    };

    let err = yggdrasil_ledger::witnesses::verify_bootstrap_witnesses(&tx_body_hash, &[witness])
        .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::InvalidBootstrapWitnessSignature { .. }
    ));
}

#[test]
fn bootstrap_witness_rejects_non_map_attributes() {
    let tx_body_hash = [0x44; 32];
    let witness = BootstrapWitness {
        public_key: test_vkey(&TEST_SEED),
        signature: test_sign(&TEST_SEED, &tx_body_hash),
        chain_code: [0x33; 32],
        attributes: vec![0x01],
    };

    let err = yggdrasil_ledger::witnesses::verify_bootstrap_witnesses(&tx_body_hash, &[witness])
        .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::InvalidBootstrapWitnessAttributes(_)
    ));
}

#[test]
fn conway_block_rejects_committee_vote_on_non_bootstrap_action_during_bootstrap() {
    use std::collections::BTreeMap;

    let cold_credential = StakeCredential::AddrKeyHash([0x5F; 28]);
    let hot_keyhash = [0x60; 28];
    let gov_action_id = GovActionId {
        transaction_id: [0x61; 32],
        gov_action_index: 0,
    };
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x62; 28]),
    };

    let mut votes = BTreeMap::new();
    votes.insert(
        gov_action_id.clone(),
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
        certificates: Some(vec![DCert::CommitteeAuthorization(
            cold_credential,
            StakeCredential::AddrKeyHash(hot_keyhash),
        )]),
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
            procedures: [(Voter::CommitteeKeyHash(hot_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x63; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.governance_actions_mut().insert(
        gov_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add: BTreeMap::new(),
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: Anchor {
                url: "https://example.invalid/bootstrap-committee-update".to_owned(),
                data_hash: [0x64; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD5, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedVotesDuringBootstrap(vec![(
            Voter::CommitteeKeyHash(hot_keyhash),
            gov_action_id,
        )])
    );
}

#[test]
fn conway_block_rejects_stake_pool_vote_on_non_bootstrap_action_during_bootstrap() {
    use std::collections::BTreeMap;

    let stake_pool_keyhash = [0x65; 28];
    let gov_action_id = GovActionId {
        transaction_id: [0x66; 32],
        gov_action_index: 0,
    };
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x67; 28]),
    };

    let mut votes = BTreeMap::new();
    votes.insert(
        gov_action_id.clone(),
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
            procedures: [(Voter::StakePool(stake_pool_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x68; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state.pool_state_mut().register(PoolParams {
        operator: stake_pool_keyhash,
        vrf_keyhash: [0x69; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0x6A; 28]),
        },
        pool_owners: vec![stake_pool_keyhash],
        relays: vec![],
        pool_metadata: None,
    });
    state.governance_actions_mut().insert(
        gov_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals: [(reward_account, 1)].into_iter().collect(),
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/bootstrap-spo-treasury".to_owned(),
                data_hash: [0x6B; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD6, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedVotesDuringBootstrap(vec![(
            Voter::StakePool(stake_pool_keyhash),
            gov_action_id,
        )])
    );
}

#[test]
fn conway_block_accepts_allowed_bootstrap_votes() {
    use std::collections::BTreeMap;

    let payment_keyhash = [0x7A; 28];
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let drep_keyhash = [0x6C; 28];
    let cold_credential = StakeCredential::AddrKeyHash([0x6D; 28]);
    let committee_hot_keyhash = [0x6E; 28];
    let stake_pool_keyhash = [0x6F; 28];

    let drep_action_id = GovActionId {
        transaction_id: [0x70; 32],
        gov_action_index: 0,
    };
    let committee_action_id = GovActionId {
        transaction_id: [0x71; 32],
        gov_action_index: 0,
    };
    let spo_action_id = GovActionId {
        transaction_id: [0x72; 32],
        gov_action_index: 0,
    };

    let mut drep_votes = BTreeMap::new();
    drep_votes.insert(
        drep_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let mut committee_votes = BTreeMap::new();
    committee_votes.insert(
        committee_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let mut spo_votes = BTreeMap::new();
    spo_votes.insert(
        spo_action_id.clone(),
        VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );

    let tx_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x7B; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 0,
        ttl: None,
        certificates: Some(vec![DCert::CommitteeAuthorization(
            cold_credential,
            StakeCredential::AddrKeyHash(committee_hot_keyhash),
        )]),
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
            procedures: [
                (Voter::DRepKeyHash(drep_keyhash), drep_votes),
                (
                    Voter::CommitteeKeyHash(committee_hot_keyhash),
                    committee_votes,
                ),
                (Voter::StakePool(stake_pool_keyhash), spo_votes),
            ]
            .into_iter()
            .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x73; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.pool_state_mut().register(PoolParams {
        operator: stake_pool_keyhash,
        vrf_keyhash: [0x74; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0x75; 28]),
        },
        pool_owners: vec![stake_pool_keyhash],
        relays: vec![],
        pool_metadata: None,
    });
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x7B; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x76; 28]),
    };
    state.governance_actions_mut().insert(
        drep_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/bootstrap-info".to_owned(),
                data_hash: [0x77; 32],
            },
        }),
    );
    state.governance_actions_mut().insert(
        committee_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/bootstrap-committee-pparam".to_owned(),
                data_hash: [0x78; 32],
            },
        }),
    );
    state.governance_actions_mut().insert(
        spo_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    min_fee_a: Some(1),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/bootstrap-spo-pparam".to_owned(),
                data_hash: [0x79; 32],
            },
        }),
    );

    state
        .apply_block(&make_conway_block(500, 1, 0xD7, vec![tx]))
        .expect("allowed bootstrap votes should succeed");

    let drep_action = state
        .governance_action(&drep_action_id)
        .expect("drep action stored");
    assert_eq!(
        drep_action.votes().get(&Voter::DRepKeyHash(drep_keyhash)),
        Some(&Vote::Yes)
    );

    let committee_action = state
        .governance_action(&committee_action_id)
        .expect("committee action stored");
    assert_eq!(
        committee_action
            .votes()
            .get(&Voter::CommitteeKeyHash(committee_hot_keyhash)),
        Some(&Vote::Yes)
    );

    let spo_action = state
        .governance_action(&spo_action_id)
        .expect("spo action stored");
    assert_eq!(
        spo_action
            .votes()
            .get(&Voter::StakePool(stake_pool_keyhash)),
        Some(&Vote::No)
    );
}

#[test]
fn conway_block_accepts_allowed_info_action_proposal_during_bootstrap() {
    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x80; 28]),
    };

    let proposal = ProposalProcedure {
        deposit: 0,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::InfoAction,
        anchor: Anchor {
            url: "https://example.invalid/bootstrap-info-proposal".to_owned(),
            data_hash: [0x81; 32],
        },
    };

    let tx_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x82; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
        proposal_procedures: Some(vec![proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(conway_bootstrap_protocol_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x82; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    state
        .apply_block(&make_conway_block(500, 1, 0xD8, vec![tx]))
        .expect("allowed bootstrap InfoAction proposal should succeed");

    let gov_action_id = GovActionId {
        transaction_id: tx_id_hash.0,
        gov_action_index: 0,
    };
    let stored_action = state
        .governance_action(&gov_action_id)
        .expect("bootstrap InfoAction proposal stored");
    assert_eq!(stored_action.proposal(), &proposal);
}

#[test]
fn conway_block_rejects_missing_cross_tx_prev_governance_action_reference() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x54; 28]),
    };
    let invalid_proposal = ProposalProcedure {
        deposit: 1,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::NewConstitution {
            prev_action_id: Some(GovActionId {
                transaction_id: [0x55; 32],
                gov_action_index: 0,
            }),
            constitution: Constitution {
                anchor: Anchor {
                    url: "https://example.invalid/missing-prev-constitution".to_owned(),
                    data_hash: [0x56; 32],
                },
                guardrails_script_hash: None,
            },
        },
        anchor: Anchor {
            url: "https://example.invalid/missing-prev-action".to_owned(),
            data_hash: [0x57; 32],
        },
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
        proposal_procedures: Some(vec![invalid_proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x58; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD3, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::InvalidPrevGovActionId(invalid_proposal));
}

#[test]
fn conway_block_rejects_cross_tx_prev_governance_action_with_wrong_purpose() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x59; 28]),
    };
    let existing_constitution_action_id = GovActionId {
        transaction_id: [0x5A; 32],
        gov_action_index: 0,
    };
    let invalid_proposal = ProposalProcedure {
        deposit: 1,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::NoConfidence {
            prev_action_id: Some(existing_constitution_action_id.clone()),
        },
        anchor: Anchor {
            url: "https://example.invalid/wrong-purpose-prev-action".to_owned(),
            data_hash: [0x5B; 32],
        },
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
        proposal_procedures: Some(vec![invalid_proposal.clone()]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let tx = yggdrasil_ledger::Tx {
        id: TxId([0x5C; 32]),
        body: tx_body.to_cbor_bytes(),
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state.governance_actions_mut().insert(
        existing_constitution_action_id,
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::NewConstitution {
                prev_action_id: None,
                constitution: Constitution {
                    anchor: Anchor {
                        url: "https://example.invalid/existing-constitution".to_owned(),
                        data_hash: [0x5D; 32],
                    },
                    guardrails_script_hash: None,
                },
            },
            anchor: Anchor {
                url: "https://example.invalid/stored-constitution-action".to_owned(),
                data_hash: [0x5E; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD4, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::InvalidPrevGovActionId(invalid_proposal));
}

#[test]
fn conway_block_rejects_proposal_return_account_with_wrong_network_id() {
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x4E; 28]),
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
                url: "https://example.invalid/network-mismatch".to_owned(),
                data_hash: [0x4F; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state.set_expected_network_id(1);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD2, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ProposalProcedureNetworkIdMismatch {
            account: reward_account,
            expected_network: 1,
        }
    );
}

#[test]
fn conway_block_rejects_treasury_withdrawals_target_with_wrong_network_id() {
    use std::collections::BTreeMap;

    let proposal_return_account = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash([0x50; 28]),
    };
    let treasury_target_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x51; 28]),
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
                url: "https://example.invalid/withdrawal-network-mismatch".to_owned(),
                data_hash: [0x52; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(proposal_return_account.credential);
    state
        .stake_credentials_mut()
        .register(treasury_target_account.credential);
    state.set_expected_network_id(1);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD3, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::TreasuryWithdrawalsNetworkIdMismatch {
            account: treasury_target_account,
            expected_network: 1,
        }
    );
}

#[test]
fn conway_block_rejects_empty_treasury_withdrawals_proposal() {
    use std::collections::BTreeMap;

    let proposal_return_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x53; 28]),
    };
    let withdrawals = BTreeMap::new();

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
                url: "https://example.invalid/empty-withdrawals".to_owned(),
                data_hash: [0x54; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    // ZeroTreasuryWithdrawals is only enforced after the Conway bootstrap
    // phase (PV major ≥ 10).
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((10, 0));
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    state.set_protocol_params(pp);
    state
        .stake_credentials_mut()
        .register(proposal_return_account.credential);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD4, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ZeroTreasuryWithdrawals(GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        })
    );
}

#[test]
fn conway_block_rejects_all_zero_treasury_withdrawals_proposal() {
    use std::collections::BTreeMap;

    let proposal_return_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x55; 28]),
    };
    let treasury_target_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x56; 28]),
    };

    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(treasury_target_account, 0);

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
                withdrawals: withdrawals.clone(),
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/all-zero-withdrawals".to_owned(),
                data_hash: [0x57; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((10, 0));
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    state.set_protocol_params(pp);
    state
        .stake_credentials_mut()
        .register(proposal_return_account.credential);
    state
        .stake_credentials_mut()
        .register(treasury_target_account.credential);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD5, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ZeroTreasuryWithdrawals(GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        })
    );
}

#[test]
fn conway_block_rejects_conflicting_committee_update_proposal() {
    use std::collections::BTreeMap;

    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x58; 28]),
    };
    let conflicting_member = StakeCredential::AddrKeyHash([0x59; 28]);

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
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![conflicting_member],
                members_to_add: BTreeMap::from([(conflicting_member, 10)]),
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: Anchor {
                url: "https://example.invalid/conflicting-committee-update".to_owned(),
                data_hash: [0x5A; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD6, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ConflictingCommitteeUpdate(vec![conflicting_member])
    );
}

#[test]
fn conway_block_rejects_committee_update_with_expired_member_epoch() {
    use std::collections::BTreeMap;

    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x5B; 28]),
    };
    let expiring_member = StakeCredential::AddrKeyHash([0x5C; 28]);

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
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add: BTreeMap::from([(expiring_member, 5)]),
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: Anchor {
                url: "https://example.invalid/expiration-epoch-too-small".to_owned(),
                data_hash: [0x5D; 32],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state.set_current_epoch(EpochNo(5));

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD8, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::ExpirationEpochTooSmall(vec![(expiring_member, EpochNo(5))])
    );
}

#[test]
fn conway_block_rejects_mismatched_current_treasury_value() {
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
        proposal_procedures: None,
        current_treasury_value: Some(42),
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.accounting_mut().treasury = 41;

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD7, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::CurrentTreasuryValueIncorrect {
            supplied: 42,
            actual: 41,
        }
    );
}

#[test]
fn conway_block_accepts_matching_current_treasury_value() {
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
        proposal_procedures: None,
        current_treasury_value: Some(42),
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.accounting_mut().treasury = 42;

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD7, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn conway_block_rejects_unknown_drep_voter() {
    use std::collections::BTreeMap;

    let voter_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let mut votes = BTreeMap::new();
    votes.insert(
        GovActionId {
            transaction_id: [0x61; 32],
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
            procedures: [(Voter::DRepKeyHash(voter_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD8, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::VotersDoNotExist(vec![Voter::DRepKeyHash(voter_keyhash)])
    );
}

#[test]
fn conway_block_rejects_unknown_stake_pool_voter() {
    use std::collections::BTreeMap;

    let voter_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let mut votes = BTreeMap::new();
    votes.insert(
        GovActionId {
            transaction_id: [0x62; 32],
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
            procedures: [(Voter::StakePool(voter_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xD9, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::VotersDoNotExist(vec![Voter::StakePool(voter_keyhash)])
    );
}

#[test]
fn conway_block_accepts_same_tx_registered_drep_voter() {
    use std::collections::BTreeMap;

    let voter_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let known_action_id = GovActionId {
        transaction_id: [0x63; 32],
        gov_action_index: 0,
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        known_action_id.clone(),
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
        certificates: Some(vec![DCert::DrepRegistration(
            StakeCredential::AddrKeyHash(voter_keyhash),
            0,
            None,
        )]),
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
            procedures: [(Voter::DRepKeyHash(voter_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.governance_actions_mut().insert(
        known_action_id,
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x6C; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/known-drep-vote".to_owned(),
                data_hash: [0x6D; 32],
            },
        }),
    );
    let err = state
        .apply_block(&make_conway_block(500, 1, 0xDA, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn conway_block_accepts_same_tx_committee_hot_authorization_for_voter() {
    use std::collections::BTreeMap;

    let hot_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let cold_credential = StakeCredential::AddrKeyHash(hot_keyhash);
    let known_action_id = GovActionId {
        transaction_id: [0x65; 32],
        gov_action_index: 0,
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        known_action_id.clone(),
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
        certificates: Some(vec![DCert::CommitteeAuthorization(
            cold_credential,
            StakeCredential::AddrKeyHash(hot_keyhash),
        )]),
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
            procedures: [(Voter::CommitteeKeyHash(hot_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.governance_actions_mut().insert(
        known_action_id,
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x6E; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/known-committee-vote".to_owned(),
                data_hash: [0x6F; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xDB, vec![tx]))
        .unwrap_err();

    assert_eq!(err, LedgerError::NoInputs);
}

#[test]
fn conway_block_rejects_vote_for_unknown_governance_action() {
    use std::collections::BTreeMap;

    let voter_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let mut votes = BTreeMap::new();
    let missing_action_id = GovActionId {
        transaction_id: [0x66; 32],
        gov_action_index: 9,
    };
    votes.insert(
        missing_action_id.clone(),
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
            procedures: [(Voter::DRepKeyHash(voter_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .drep_state_mut()
        .register(DRep::KeyHash(voter_keyhash), RegisteredDrep::new(0, None));

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xDC, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::GovActionsDoNotExist(vec![missing_action_id])
    );
}

#[test]
fn conway_block_rejects_vote_for_expired_governance_action() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x68; 28]),
    };
    let drep_keyhash = payment_keyhash;

    let proposal_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x86; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/expiring-proposal".to_owned(),
                data_hash: [0x69; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let proposal_body_bytes = proposal_body.to_cbor_bytes();
    let proposal_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&proposal_body_bytes);
    let proposal_tx = yggdrasil_ledger::Tx {
        id: TxId(proposal_tx_id_hash.0),
        body: proposal_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &proposal_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let gov_action_id = GovActionId {
        transaction_id: proposal_tx_id_hash.0,
        gov_action_index: 0,
    };
    let mut vote_map = BTreeMap::new();
    vote_map.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );

    let vote_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x87; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let vote_body_bytes = vote_body.to_cbor_bytes();
    let vote_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&vote_body_bytes);
    let vote_tx = yggdrasil_ledger::Tx {
        id: TxId(vote_tx_id_hash.0),
        body: vote_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &vote_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x86; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x87; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));

    let mut params = ProtocolParameters::alonzo_defaults();
    params.gov_action_lifetime = Some(1);
    state.set_protocol_params(params);
    state.set_current_epoch(EpochNo(0));

    state
        .apply_block(&make_conway_block(500, 1, 0xE4, vec![proposal_tx]))
        .expect("proposal transaction should apply");

    let stored_action = state
        .governance_action(&gov_action_id)
        .expect("stored governance action");
    assert_eq!(stored_action.proposed_in(), Some(EpochNo(0)));
    assert_eq!(stored_action.expires_after(), Some(EpochNo(1)));

    state.set_current_epoch(EpochNo(2));

    let err = state
        .apply_block(&make_conway_block(501, 2, 0xE5, vec![vote_tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::VotingOnExpiredGovAction(vec![(
            Voter::DRepKeyHash(drep_keyhash),
            gov_action_id,
        )])
    );
}

#[test]
fn conway_block_rejects_committee_votes_for_disallowed_actions() {
    use std::collections::BTreeMap;

    let hot_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let cold_credential = StakeCredential::AddrKeyHash(hot_keyhash);
    let no_confidence_action_id = GovActionId {
        transaction_id: [0x70; 32],
        gov_action_index: 0,
    };
    let update_committee_action_id = GovActionId {
        transaction_id: [0x71; 32],
        gov_action_index: 0,
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        no_confidence_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    votes.insert(
        update_committee_action_id.clone(),
        VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );

    let tx_body = ConwayTxBody {
        inputs: vec![],
        outputs: vec![],
        fee: 0,
        ttl: None,
        certificates: Some(vec![DCert::CommitteeAuthorization(
            cold_credential,
            StakeCredential::AddrKeyHash(hot_keyhash),
        )]),
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
            procedures: [(Voter::CommitteeKeyHash(hot_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state
        .committee_state_mut()
        .register_with_term(cold_credential, 200);
    state.governance_actions_mut().insert(
        no_confidence_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x72; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::NoConfidence {
                prev_action_id: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/no-confidence".to_owned(),
                data_hash: [0x73; 32],
            },
        }),
    );
    state.governance_actions_mut().insert(
        update_committee_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x74; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add: BTreeMap::new(),
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: Anchor {
                url: "https://example.invalid/update-committee".to_owned(),
                data_hash: [0x75; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xDF, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedVoters(vec![
            (
                Voter::CommitteeKeyHash(hot_keyhash),
                no_confidence_action_id
            ),
            (
                Voter::CommitteeKeyHash(hot_keyhash),
                update_committee_action_id
            ),
        ])
    );
}

#[test]
fn conway_block_rejects_stake_pool_votes_for_disallowed_actions() {
    use std::collections::BTreeMap;

    let stake_pool_keyhash = [0x76; 28];
    let treasury_action_id = GovActionId {
        transaction_id: [0x77; 32],
        gov_action_index: 0,
    };
    let constitution_action_id = GovActionId {
        transaction_id: [0x78; 32],
        gov_action_index: 0,
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        treasury_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    votes.insert(
        constitution_action_id.clone(),
        VotingProcedure {
            vote: Vote::Abstain,
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
            procedures: [(Voter::StakePool(stake_pool_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.pool_state_mut().register(PoolParams {
        operator: stake_pool_keyhash,
        vrf_keyhash: [0x79; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0x7A; 28]),
        },
        pool_owners: vec![stake_pool_keyhash],
        relays: vec![],
        pool_metadata: None,
    });
    state.governance_actions_mut().insert(
        treasury_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x7B; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals: [(
                    RewardAccount {
                        network: 0,
                        credential: StakeCredential::AddrKeyHash([0x7C; 28]),
                    },
                    1_000,
                )]
                .into_iter()
                .collect(),
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/treasury-withdrawals".to_owned(),
                data_hash: [0x7D; 32],
            },
        }),
    );
    state.governance_actions_mut().insert(
        constitution_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0x7E; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::NewConstitution {
                prev_action_id: None,
                constitution: Constitution {
                    anchor: Anchor {
                        url: "https://example.invalid/constitution".to_owned(),
                        data_hash: [0x7F; 32],
                    },
                    guardrails_script_hash: None,
                },
            },
            anchor: Anchor {
                url: "https://example.invalid/new-constitution".to_owned(),
                data_hash: [0x80; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xE0, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedVoters(vec![
            (Voter::StakePool(stake_pool_keyhash), treasury_action_id),
            (Voter::StakePool(stake_pool_keyhash), constitution_action_id),
        ])
    );
}

#[test]
fn conway_block_rejects_spo_vote_on_non_security_parameter_change() {
    use std::collections::BTreeMap;

    let stake_pool_keyhash = [0xA4; 28];
    let action_id = GovActionId {
        transaction_id: [0xA5; 32],
        gov_action_index: 0,
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        action_id.clone(),
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
            procedures: [(Voter::StakePool(stake_pool_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.pool_state_mut().register(PoolParams {
        operator: stake_pool_keyhash,
        vrf_keyhash: [0xA6; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0xA7; 28]),
        },
        pool_owners: vec![stake_pool_keyhash],
        relays: vec![],
        pool_metadata: None,
    });
    // key_deposit is Economic-only (no security group) — SPO should be rejected
    state.governance_actions_mut().insert(
        action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0xA8; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    key_deposit: Some(2_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/non-security-pparam".to_owned(),
                data_hash: [0xA9; 32],
            },
        }),
    );

    let err = state
        .apply_block(&make_conway_block(500, 1, 0xE1, vec![tx]))
        .unwrap_err();

    assert_eq!(
        err,
        LedgerError::DisallowedVoters(vec![(Voter::StakePool(stake_pool_keyhash), action_id),])
    );
}

#[test]
fn conway_block_accepts_spo_vote_on_security_group_parameter_change() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let stake_pool_keyhash = payment_keyhash;
    let action_id = GovActionId {
        transaction_id: [0xAA; 32],
        gov_action_index: 0,
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );

    let tx_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            procedures: [(Voter::StakePool(stake_pool_keyhash), votes)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let body_bytes = tx_body.to_cbor_bytes();
    let tx_id_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let ws = witness_set_with_vkeys(vec![make_witness(&TEST_SEED, &tx_id_hash.0)]);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    let mut params = conway_bootstrap_protocol_params();
    // Set protocol version to 10.0 (post-bootstrap)
    params.protocol_version = Some((10, 0));
    state.set_protocol_params(params);
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0xAB; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state.pool_state_mut().register(PoolParams {
        operator: stake_pool_keyhash,
        vrf_keyhash: [0xAC; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: StakeCredential::AddrKeyHash([0xAD; 28]),
        },
        pool_owners: vec![stake_pool_keyhash],
        relays: vec![],
        pool_metadata: None,
    });
    // max_block_body_size is Network + Security group — SPO should be allowed
    state.governance_actions_mut().insert(
        action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 0,
                credential: StakeCredential::AddrKeyHash([0xAE; 28]),
            }
            .to_bytes()
            .to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    max_block_body_size: Some(65536),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: "https://example.invalid/security-pparam".to_owned(),
                data_hash: [0xAF; 32],
            },
        }),
    );

    state
        .apply_block(&make_conway_block(500, 1, 0xE2, vec![tx]))
        .expect("SPO vote on security-group parameter change should succeed");

    let stored_action = state
        .governance_action(&action_id)
        .expect("action should be stored");
    assert_eq!(
        stored_action
            .votes()
            .get(&Voter::StakePool(stake_pool_keyhash)),
        Some(&Vote::Yes)
    );
}

#[test]
fn conway_block_persists_governance_action_and_records_votes() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x67; 28]),
    };
    let drep_keyhash = payment_keyhash;

    let proposal_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x69; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/persisted-proposal".to_owned(),
                data_hash: [0x6A; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let proposal_body_bytes = proposal_body.to_cbor_bytes();
    let proposal_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&proposal_body_bytes);
    let proposal_tx = yggdrasil_ledger::Tx {
        id: TxId(proposal_tx_id_hash.0),
        body: proposal_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &proposal_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let gov_action_id = GovActionId {
        transaction_id: proposal_tx_id_hash.0,
        gov_action_index: 0,
    };
    let mut vote_map = BTreeMap::new();
    vote_map.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );

    let vote_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x6B; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let vote_body_bytes = vote_body.to_cbor_bytes();
    let vote_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&vote_body_bytes);
    let vote_tx = yggdrasil_ledger::Tx {
        id: TxId(vote_tx_id_hash.0),
        body: vote_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &vote_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x69; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [0x6B; 32],
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: payment_address,
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));

    state
        .apply_block(&make_conway_block(500, 1, 0xDD, vec![proposal_tx]))
        .expect("proposal transaction should apply");
    state
        .apply_block(&make_conway_block(501, 2, 0xDE, vec![vote_tx]))
        .expect("vote transaction should apply");

    let stored_action = state
        .governance_action(&gov_action_id)
        .expect("stored governance action");
    assert_eq!(stored_action.proposal().gov_action, GovAction::InfoAction);
    assert_eq!(
        stored_action.votes().get(&Voter::DRepKeyHash(drep_keyhash)),
        Some(&Vote::Yes)
    );
}

#[test]
fn conway_block_removes_votes_for_unregistered_drep() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x81; 28]),
    };
    let drep_keyhash = payment_keyhash;

    let proposal_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x82; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/persisted-proposal-for-unregister".to_owned(),
                data_hash: [0x83; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let proposal_body_bytes = proposal_body.to_cbor_bytes();
    let proposal_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&proposal_body_bytes);
    let proposal_tx = yggdrasil_ledger::Tx {
        id: TxId(proposal_tx_id_hash.0),
        body: proposal_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &proposal_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let gov_action_id = GovActionId {
        transaction_id: proposal_tx_id_hash.0,
        gov_action_index: 0,
    };
    let mut vote_map = BTreeMap::new();
    vote_map.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );

    let vote_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x84; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
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
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map)]
                .into_iter()
                .collect(),
        }),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };

    let vote_body_bytes = vote_body.to_cbor_bytes();
    let vote_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&vote_body_bytes);
    let vote_tx = yggdrasil_ledger::Tx {
        id: TxId(vote_tx_id_hash.0),
        body: vote_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &vote_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let unregister_body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x85; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: Some(vec![DCert::DrepUnregistration(
            StakeCredential::AddrKeyHash(drep_keyhash),
            0,
        )]),
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

    let unregister_body_bytes = unregister_body.to_cbor_bytes();
    let unregister_tx_id_hash = yggdrasil_crypto::hash_bytes_256(&unregister_body_bytes);
    let unregister_tx = yggdrasil_ledger::Tx {
        id: TxId(unregister_tx_id_hash.0),
        body: unregister_body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &unregister_tx_id_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };

    let mut state = LedgerState::new(Era::Conway);
    for transaction_id in [[0x82; 32], [0x84; 32], [0x85; 32]] {
        state.multi_era_utxo_mut().insert(
            ShelleyTxIn {
                transaction_id,
                index: 0,
            },
            MultiEraTxOut::Babbage(BabbageTxOut {
                address: payment_address.clone(),
                amount: Value::Coin(5_000_000),
                datum_option: None,
                script_ref: None,
            }),
        );
    }
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));

    state
        .apply_block(&make_conway_block(500, 1, 0xE1, vec![proposal_tx]))
        .expect("proposal transaction should apply");
    state
        .apply_block(&make_conway_block(501, 2, 0xE2, vec![vote_tx]))
        .expect("vote transaction should apply");
    state
        .apply_block(&make_conway_block(502, 3, 0xE3, vec![unregister_tx]))
        .expect("unregister transaction should apply");

    let stored_action = state
        .governance_action(&gov_action_id)
        .expect("stored governance action");
    assert_eq!(stored_action.proposal().gov_action, GovAction::InfoAction);
    assert_eq!(
        stored_action.votes().get(&Voter::DRepKeyHash(drep_keyhash)),
        None
    );
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let block = make_allegra_block_raw(500, 1, 0xBB, vec![tx]);
    state
        .apply_block(&block)
        .expect("native script should evaluate true");
}

#[test]
fn allegra_block_rejects_native_script_failure() {
    // NativeScript::InvalidBefore(1000) — requires slot >= 1000.
    let script = NativeScript::InvalidBefore(1000);
    let script_hash = native_script_hash(&script);

    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
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
fn allegra_block_rejects_missing_required_script_witness() {
    let script = NativeScript::InvalidBefore(0);
    let script_hash = native_script_hash(&script);
    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
    let ws = witness_set_with_vkeys_and_scripts(vec![], vec![]);

    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_id_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&ws)),
        auxiliary_data: None,
        is_valid: None,
    };

    let block = make_allegra_block_raw(500, 1, 0xCD, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::MissingScriptWitness { hash } if hash == script_hash),
        "expected MissingScriptWitness, got: {err:?}"
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
    };

    // Block slot 500 >= 100 → script should evaluate true.
    let block = make_allegra_block_raw(500, 1, 0xDD, vec![tx]);
    state
        .apply_block(&block)
        .expect("timelock should pass at slot 500");
}

#[test]
fn allegra_block_rejects_native_script_hereafter_exceeded() {
    // NativeScript::InvalidHereafter(200) — requires slot < 200.
    let script = NativeScript::InvalidHereafter(200);
    let script_hash = native_script_hash(&script);

    let addr = enterprise_scripthash_address(&script_hash);

    let mut state = LedgerState::new(Era::Allegra);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let block = make_allegra_block_raw(500, 1, 0xA1, vec![tx]);
    state
        .apply_block(&block)
        .expect("ScriptAll with both vkeys should pass");
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = AllegraTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
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
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: addr.clone(),
            amount: 5_000_000,
        },
    );

    let tx_body = ShelleyTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        }],
        outputs: vec![ShelleyTxOut {
            address: addr,
            amount: 4_800_000,
        }],
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
        auxiliary_data: None,
        is_valid: None,
    };

    let block = make_shelley_block(500, 1, 0xAA, vec![tx]);
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::InvalidVKeyWitnessSignature { hash } if hash == keyhash),
        "expected InvalidVKeyWitnessSignature for wrong-body sig, got: {err:?}"
    );
}

// ===========================================================================
// E2E Governance lifecycle integration tests
// ===========================================================================

/// Helper: build a Conway tx with only proposal procedures.
fn make_proposal_tx(
    input_tx_id: [u8; 32],
    payment_address: &[u8],
    output_amount: u64,
    fee: u64,
    proposals: Vec<ProposalProcedure>,
    seed: &[u8; 32],
) -> (yggdrasil_ledger::Tx, [u8; 32]) {
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_tx_id,
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.to_vec(),
            amount: Value::Coin(output_amount),
            datum_option: None,
            script_ref: None,
        }],
        fee,
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
        proposal_procedures: Some(proposals),
        current_treasury_value: None,
        treasury_donation: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let tx = yggdrasil_ledger::Tx {
        id: TxId(tx_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(seed, &tx_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };
    (tx, tx_hash.0)
}

/// Helper: build a Conway tx with only voting procedures.
fn make_vote_tx(
    input_tx_id: [u8; 32],
    payment_address: &[u8],
    output_amount: u64,
    fee: u64,
    voting_procedures: VotingProcedures,
    seed: &[u8; 32],
) -> yggdrasil_ledger::Tx {
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_tx_id,
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.to_vec(),
            amount: Value::Coin(output_amount),
            datum_option: None,
            script_ref: None,
        }],
        fee,
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
        voting_procedures: Some(voting_procedures),
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    yggdrasil_ledger::Tx {
        id: TxId(tx_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(seed, &tx_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    }
}

/// Helper: seed UTxO for a given tx input.
fn seed_utxo(state: &mut LedgerState, tx_id: [u8; 32], address: &[u8], amount: u64) {
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: tx_id,
            index: 0,
        },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: address.to_vec(),
            amount: Value::Coin(amount),
            datum_option: None,
            script_ref: None,
        }),
    );
}

#[test]
fn conway_block_vote_recast_overwrites_previous_vote() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0x90; 28]),
    };
    let drep_keyhash = payment_keyhash;

    // Propose an InfoAction.
    let (proposal_tx, proposal_hash) = make_proposal_tx(
        [0x91; 32],
        &payment_address,
        4_800_000,
        200_000,
        vec![ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/vote-recast".to_owned(),
                data_hash: [0x92; 32],
            },
        }],
        &TEST_SEED,
    );
    let gov_action_id = GovActionId {
        transaction_id: proposal_hash,
        gov_action_index: 0,
    };

    // First vote: Yes.
    let mut vote_map1 = BTreeMap::new();
    vote_map1.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let vote_tx1 = make_vote_tx(
        [0x93; 32],
        &payment_address,
        4_800_000,
        200_000,
        VotingProcedures {
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map1)]
                .into_iter()
                .collect(),
        },
        &TEST_SEED,
    );

    // Second vote: No (recast).
    let mut vote_map2 = BTreeMap::new();
    vote_map2.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );
    let vote_tx2 = make_vote_tx(
        [0x94; 32],
        &payment_address,
        4_800_000,
        200_000,
        VotingProcedures {
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map2)]
                .into_iter()
                .collect(),
        },
        &TEST_SEED,
    );

    let mut state = LedgerState::new(Era::Conway);
    for tx_id in [[0x91; 32], [0x93; 32], [0x94; 32]] {
        seed_utxo(&mut state, tx_id, &payment_address, 5_000_000);
    }
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));

    state
        .apply_block(&make_conway_block(500, 1, 0xF1, vec![proposal_tx]))
        .unwrap();
    state
        .apply_block(&make_conway_block(501, 2, 0xF2, vec![vote_tx1]))
        .unwrap();

    // Verify initial vote is Yes.
    assert_eq!(
        state
            .governance_action(&gov_action_id)
            .unwrap()
            .votes()
            .get(&Voter::DRepKeyHash(drep_keyhash)),
        Some(&Vote::Yes),
    );

    state
        .apply_block(&make_conway_block(502, 3, 0xF3, vec![vote_tx2]))
        .unwrap();

    // Verify vote has been recast to No.
    assert_eq!(
        state
            .governance_action(&gov_action_id)
            .unwrap()
            .votes()
            .get(&Voter::DRepKeyHash(drep_keyhash)),
        Some(&Vote::No),
    );
}

#[test]
fn conway_block_proposal_and_vote_in_same_block() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0xA0; 28]),
    };
    let drep_keyhash = payment_keyhash;

    // Proposal tx.
    let (proposal_tx, proposal_hash) = make_proposal_tx(
        [0xA1; 32],
        &payment_address,
        4_800_000,
        200_000,
        vec![ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/same-block".to_owned(),
                data_hash: [0xA2; 32],
            },
        }],
        &TEST_SEED,
    );
    let gov_action_id = GovActionId {
        transaction_id: proposal_hash,
        gov_action_index: 0,
    };

    // Vote tx in the same block, referencing the proposal from the first tx.
    let mut vote_map = BTreeMap::new();
    vote_map.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let vote_tx = make_vote_tx(
        [0xA3; 32],
        &payment_address,
        4_800_000,
        200_000,
        VotingProcedures {
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map)]
                .into_iter()
                .collect(),
        },
        &TEST_SEED,
    );

    let mut state = LedgerState::new(Era::Conway);
    for tx_id in [[0xA1; 32], [0xA3; 32]] {
        seed_utxo(&mut state, tx_id, &payment_address, 5_000_000);
    }
    state
        .stake_credentials_mut()
        .register(reward_account.credential);
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));

    // Both txs in the same block — proposal should be visible to the vote.
    state
        .apply_block(&make_conway_block(500, 1, 0xB1, vec![proposal_tx, vote_tx]))
        .unwrap();

    let stored = state.governance_action(&gov_action_id).unwrap();
    assert_eq!(stored.proposal().gov_action, GovAction::InfoAction);
    assert_eq!(
        stored.votes().get(&Voter::DRepKeyHash(drep_keyhash)),
        Some(&Vote::Yes)
    );
}

#[test]
fn conway_block_cross_block_lineage_chain() {
    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0xB0; 28]),
    };

    // First proposal: HardForkInitiation with no prev.
    let (proposal_tx1, hash1) = make_proposal_tx(
        [0xB1; 32],
        &payment_address,
        4_800_000,
        200_000,
        vec![ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: Anchor {
                url: "https://example.invalid/hf1".to_owned(),
                data_hash: [0xB2; 32],
            },
        }],
        &TEST_SEED,
    );
    let gov_id1 = GovActionId {
        transaction_id: hash1,
        gov_action_index: 0,
    };

    // Second proposal: HardForkInitiation chaining from the first.
    // Uses (10, 1) — same major, minor+1 — which is valid under
    // upstream `preceedingHardFork`: with no enacted root,
    // `succVersion(9) = 10 >= 10` passes, then chaining walks to
    // the pending (10, 0) and `pvCanFollow (10,0) (10,1)` succeeds.
    let (proposal_tx2, hash2) = make_proposal_tx(
        [0xB3; 32],
        &payment_address,
        4_800_000,
        200_000,
        vec![ProposalProcedure {
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: Some(gov_id1.clone()),
                protocol_version: (10, 1),
            },
            anchor: Anchor {
                url: "https://example.invalid/hf2".to_owned(),
                data_hash: [0xB4; 32],
            },
        }],
        &TEST_SEED,
    );
    let gov_id2 = GovActionId {
        transaction_id: hash2,
        gov_action_index: 0,
    };

    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.protocol_version = Some((9, 0));
    *state.protocol_params_mut() = Some(pp);
    for tx_id in [[0xB1; 32], [0xB3; 32]] {
        seed_utxo(&mut state, tx_id, &payment_address, 5_000_000);
    }
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    // Block 1: first hard fork proposal.
    state
        .apply_block(&make_conway_block(500, 1, 0xC1, vec![proposal_tx1]))
        .unwrap();
    assert!(state.governance_action(&gov_id1).is_some());

    // Block 2: second hard fork proposal chaining from first.
    state
        .apply_block(&make_conway_block(501, 2, 0xC2, vec![proposal_tx2]))
        .unwrap();
    let stored2 = state.governance_action(&gov_id2).unwrap();
    assert_eq!(
        stored2.proposal().gov_action,
        GovAction::HardForkInitiation {
            prev_action_id: Some(gov_id1),
            protocol_version: (10, 1)
        },
    );
}

#[test]
fn conway_block_cert_and_proposal_and_vote_combo() {
    use std::collections::BTreeMap;

    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let staker_cred = StakeCredential::AddrKeyHash([0xC0; 28]);
    let reward_account = RewardAccount {
        network: 0,
        credential: staker_cred,
    };
    let drep_keyhash = payment_keyhash;

    // Single tx: register stake cred + propose + vote.
    let body = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xC1; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: payment_address.clone(),
            amount: Value::Coin(4_800_000),
            datum_option: None,
            script_ref: None,
        }],
        fee: 200_000,
        ttl: None,
        certificates: Some(vec![DCert::AccountRegistration(staker_cred)]),
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
            deposit: 0,
            reward_account: reward_account.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/combo".to_owned(),
                data_hash: [0xC2; 32],
            },
        }]),
        current_treasury_value: None,
        treasury_donation: None,
    };
    let body_bytes = body.to_cbor_bytes();
    let tx_hash = yggdrasil_crypto::hash_bytes_256(&body_bytes);
    let combo_tx = yggdrasil_ledger::Tx {
        id: TxId(tx_hash.0),
        body: body_bytes,
        witnesses: Some(encode_witness_set(&witness_set_with_vkeys(vec![
            make_witness(&TEST_SEED, &tx_hash.0),
        ]))),
        auxiliary_data: None,
        is_valid: None,
    };
    let gov_action_id = GovActionId {
        transaction_id: tx_hash.0,
        gov_action_index: 0,
    };

    // Vote in separate tx same block.
    let mut vote_map = BTreeMap::new();
    vote_map.insert(
        gov_action_id.clone(),
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let vote_tx = make_vote_tx(
        [0xC3; 32],
        &payment_address,
        4_800_000,
        200_000,
        VotingProcedures {
            procedures: [(Voter::DRepKeyHash(drep_keyhash), vote_map)]
                .into_iter()
                .collect(),
        },
        &TEST_SEED,
    );

    let mut state = LedgerState::new(Era::Conway);
    for tx_id in [[0xC1; 32], [0xC3; 32]] {
        seed_utxo(&mut state, tx_id, &payment_address, 5_000_000);
    }
    // Pre-register the DRep (separate from the staking cred).
    state
        .drep_state_mut()
        .register(DRep::KeyHash(drep_keyhash), RegisteredDrep::new(0, None));

    state
        .apply_block(&make_conway_block(500, 1, 0xD1, vec![combo_tx, vote_tx]))
        .unwrap();

    // Staker credential registered via cert in same tx.
    assert!(state.stake_credentials().is_registered(&staker_cred));
    // Proposal persisted.
    let stored = state.governance_action(&gov_action_id).unwrap();
    assert_eq!(stored.proposal().gov_action, GovAction::InfoAction);
    // Vote recorded.
    assert_eq!(
        stored.votes().get(&Voter::DRepKeyHash(drep_keyhash)),
        Some(&Vote::Yes)
    );
}

#[test]
fn conway_block_multiple_proposals_in_one_tx() {
    let payment_keyhash = vkey_hash(&test_vkey(&TEST_SEED));
    let payment_address = enterprise_keyhash_address(&payment_keyhash);
    let reward_account = RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash([0xD0; 28]),
    };

    let (tx, tx_hash) = make_proposal_tx(
        [0xD1; 32],
        &payment_address,
        4_800_000,
        200_000,
        vec![
            ProposalProcedure {
                deposit: 0,
                reward_account: reward_account.to_bytes().to_vec(),
                gov_action: GovAction::InfoAction,
                anchor: Anchor {
                    url: "https://example.invalid/multi1".to_owned(),
                    data_hash: [0xD2; 32],
                },
            },
            ProposalProcedure {
                deposit: 0,
                reward_account: reward_account.to_bytes().to_vec(),
                gov_action: GovAction::InfoAction,
                anchor: Anchor {
                    url: "https://example.invalid/multi2".to_owned(),
                    data_hash: [0xD3; 32],
                },
            },
        ],
        &TEST_SEED,
    );

    let gov_id_0 = GovActionId {
        transaction_id: tx_hash,
        gov_action_index: 0,
    };
    let gov_id_1 = GovActionId {
        transaction_id: tx_hash,
        gov_action_index: 1,
    };

    let mut state = LedgerState::new(Era::Conway);
    seed_utxo(&mut state, [0xD1; 32], &payment_address, 5_000_000);
    state
        .stake_credentials_mut()
        .register(reward_account.credential);

    state
        .apply_block(&make_conway_block(500, 1, 0xE1, vec![tx]))
        .unwrap();

    // Both proposals should be persisted with correct indices.
    let s0 = state.governance_action(&gov_id_0).expect("proposal 0");
    assert_eq!(s0.proposal().anchor.url, "https://example.invalid/multi1");
    let s1 = state.governance_action(&gov_id_1).expect("proposal 1");
    assert_eq!(s1.proposal().anchor.url, "https://example.invalid/multi2");
}
