//! Parity integration tests for Conway governance fixes:
//!
//! 1. `UnelectedCommitteeVoters` — upstream `unelectedCommitteeVoters` from
//!    `Cardano.Ledger.Conway.Rules.Gov`, gated on PV ≥ 10
//!    (hardforkConwayDisallowUnelectedCommitteeFromVoting).
//!
//! 2. `DelegateeDRepNotRegistered` — upstream `DelegateeNotRegisteredDELEG`
//!    from `Cardano.Ledger.Conway.Rules.Deleg`.
//!
//! 3. Conway `AccountUnregistrationDeposit` (tag 8) enforces reward-balance
//!    check — upstream `ConwayUnRegCert` asserts
//!    `StakeKeyHasNonZeroAccountBalanceDELEG` just like Shelley tag 1.
//!
//! 4. DRep unregistration clears delegations — upstream `clearDRepDelegations`
//!    from `Cardano.Ledger.Conway.Rules.GovCert`.
//!
//! 5. Committee `isPotentialFutureMember` — authorization/resignation accepts
//!    cold credentials that appear in pending `UpdateCommittee` proposals.
//!
//! 6. `ZeroTreasuryWithdrawals` is only enforced after the bootstrap phase
//!    (PV ≥ 10) — upstream `hardforkConwayBootstrapPhase`.
//!
//! 7. Bootstrap-phase DRep delegation skip — upstream `preserveIncorrectDelegation`
//!    / `hardforkConwayBootstrapPhase` from `Cardano.Ledger.Conway.Rules.Deleg`:
//!    during bootstrap (PV == 9), delegation to an unregistered DRep is allowed.
//!
//! 8. Bootstrap-phase DRep expiry — upstream `computeDRepExpiryVersioned` from
//!    `Cardano.Ledger.Conway.Rules.GovCert`: during bootstrap, dormant epoch
//!    subtraction is skipped when computing DRep last-active-epoch.

use super::*;
use yggdrasil_ledger::GovernanceActionState;

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn make_conway_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<ConwayTxBody>) -> Block {
    let tx_list: Vec<Tx> = txs
        .iter()
        .map(|body| {
            let raw = body.to_cbor_bytes();
            let id_hash = yggdrasil_crypto::hash_bytes_256(&raw);
            Tx {
                id: TxId(id_hash.0),
                body: raw,
                witnesses: None,
                auxiliary_data: None,
                is_valid: None,
            }
        })
        .collect();

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
        transactions: tx_list,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn conway_state_pv(major: u64, minor: u64, key_deposit: u64) -> LedgerState {
    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((major, minor));
    pp.key_deposit = key_deposit;
    pp.drep_deposit = Some(500_000);
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    state.set_protocol_params(pp);
    state
}

fn conway_tx_single_cert(
    input_hash: [u8; 32],
    output_coin: u64,
    fee: u64,
    cert: DCert,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(200),
        certificates: Some(vec![cert]),
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: None,
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }
}

fn conway_tx_with_votes(
    input_hash: [u8; 32],
    output_coin: u64,
    fee: u64,
    voting_procedures: VotingProcedures,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(200),
        certificates: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: None,
        voting_procedures: Some(voting_procedures),
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }
}

fn conway_tx_with_proposal(
    input_hash: [u8; 32],
    output_coin: u64,
    fee: u64,
    proposals: Vec<ProposalProcedure>,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(200),
        certificates: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: None,
        voting_procedures: None,
        proposal_procedures: Some(proposals),
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }
}

/// Reward-account bytes for a network-1 keyhash credential.
fn reward_account_bytes(keyhash: [u8; 28]) -> Vec<u8> {
    let mut bytes = vec![0xe1]; // network 1, key-hash type
    bytes.extend_from_slice(&keyhash);
    bytes
}

/// Shortcut: insert a ShelleyTxOut into the multi-era UTxO set.
fn add_utxo(state: &mut LedgerState, input: ShelleyTxIn, amount: u64) {
    state.multi_era_utxo_mut().insert_shelley(
        input,
        ShelleyTxOut {
            address: vec![0x01],
            amount,
        },
    );
}

/// Insert a governance action into the ledger state for vote-target purposes.
fn seed_governance_action(state: &mut LedgerState) -> GovActionId {
    let gov_action_id = GovActionId {
        transaction_id: [0xAA; 32],
        gov_action_index: 0,
    };
    state.governance_actions_mut().insert(
        gov_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account_bytes([0x99; 28]),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.com".to_string(),
                data_hash: [0u8; 32],
            },
        }),
    );
    gov_action_id
}

/// Apply a CommitteeAuthorization cert through a block so the committee
/// member gets a hot credential set via the normal block-apply path.
fn authorize_committee_via_block(
    state: &mut LedgerState,
    cold_cred: StakeCredential,
    hot_cred: StakeCredential,
    utxo_seed: u8,
) {
    let input = ShelleyTxIn {
        transaction_id: [utxo_seed; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert_shelley(
        input.clone(),
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_000_000,
        },
    );

    let block = make_conway_block(
        1,
        1,
        utxo_seed,
        vec![ConwayTxBody {
            inputs: vec![input],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            fee: 0,
            ttl: Some(100),
            certificates: Some(vec![DCert::CommitteeAuthorization(cold_cred, hot_cred)]),
            validity_interval_start: None,
            mint: None,
            collateral: None,
            required_signers: None,
            network_id: None,
            collateral_return: None,
            total_collateral: None,
            reference_inputs: None,
            script_data_hash: None,
            withdrawals: None,
            voting_procedures: None,
            proposal_procedures: None,
            treasury_donation: None,
            current_treasury_value: None,
            auxiliary_data_hash: None,
        }],
    );

    state
        .apply_block(&block)
        .expect("committee authorization block");
}

// -----------------------------------------------------------------------
// 1. UnelectedCommitteeVoters
// -----------------------------------------------------------------------

#[test]
fn unelected_committee_voter_rejected_at_pv11() {
    // PV > 10 (PV 11) — `harforkConwayDisallowUnelectedCommitteeFromVoting`
    let mut state = conway_state_pv(11, 0, 2_000_000);
    let gov_action_id = seed_governance_action(&mut state);

    // Register a cold committee member and authorize its hot credential.
    let cold_cred = StakeCredential::AddrKeyHash([0xCC; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xDD; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    authorize_committee_via_block(&mut state, cold_cred, hot_cred, 0xF0);

    // An UNKNOWN hot credential that is NOT authorized by any member.
    let rogue_hot = [0xEE; 28];

    let mut procedures = std::collections::BTreeMap::new();
    let mut votes_for_voter = std::collections::BTreeMap::new();
    votes_for_voter.insert(
        gov_action_id,
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures.insert(Voter::CommitteeKeyHash(rogue_hot), votes_for_voter);
    let voting_procedures = VotingProcedures { procedures };

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x01; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 10_000_000,
        },
    );

    let block = make_conway_block(
        10,
        2,
        0x01,
        vec![conway_tx_with_votes(
            [0x01; 32],
            10_000_000,
            0,
            voting_procedures,
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::UnelectedCommitteeVoters(creds) => {
            assert_eq!(creds, vec![StakeCredential::AddrKeyHash(rogue_hot)]);
        }
        other => panic!("expected UnelectedCommitteeVoters, got {other:?}"),
    }
}

#[test]
fn elected_committee_voter_accepted_at_pv11() {
    let mut state = conway_state_pv(11, 0, 2_000_000);
    let gov_action_id = seed_governance_action(&mut state);

    let cold_cred = StakeCredential::AddrKeyHash([0xCC; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xDD; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    authorize_committee_via_block(&mut state, cold_cred, hot_cred, 0xF1);

    // Vote using the AUTHORIZED hot credential [0xDD; 28].
    let mut procedures = std::collections::BTreeMap::new();
    let mut votes_for_voter = std::collections::BTreeMap::new();
    votes_for_voter.insert(
        gov_action_id,
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures.insert(Voter::CommitteeKeyHash([0xDD; 28]), votes_for_voter);
    let voting_procedures = VotingProcedures { procedures };

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x02; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 10_000_000,
        },
    );

    let block = make_conway_block(
        10,
        2,
        0x02,
        vec![conway_tx_with_votes(
            [0x02; 32],
            10_000_000,
            0,
            voting_procedures,
        )],
    );

    state
        .apply_block(&block)
        .expect("authorized hot credential should be accepted");
}

#[test]
fn unelected_committee_voter_allowed_at_pv9() {
    // PV 9 — before the hardfork gate (PV > 10).
    let mut state = conway_state_pv(9, 0, 2_000_000);
    let gov_action_id = seed_governance_action(&mut state);

    // Register a committee member but do NOT authorize any hot credential.
    let cold_cred = StakeCredential::AddrKeyHash([0xCC; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);

    // Vote with a completely unrelated hot credential — at PV 9 the
    // unelected-committee-voters check is skipped.
    let rogue_hot = [0xEE; 28];
    let mut procedures = std::collections::BTreeMap::new();
    let mut votes_for_voter = std::collections::BTreeMap::new();
    votes_for_voter.insert(
        gov_action_id,
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures.insert(Voter::CommitteeKeyHash(rogue_hot), votes_for_voter);
    let voting_procedures = VotingProcedures { procedures };

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x03; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 10_000_000,
        },
    );

    let block = make_conway_block(
        10,
        2,
        0x03,
        vec![conway_tx_with_votes(
            [0x03; 32],
            10_000_000,
            0,
            voting_procedures,
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    // Should fail with a DIFFERENT error (voter not found), NOT
    // UnelectedCommitteeVoters — proves the PV ≤ 10 gate works.
    if let LedgerError::UnelectedCommitteeVoters(_) = err {
        panic!("should not get UnelectedCommitteeVoters at PV 9");
    }
}

#[test]
fn unelected_committee_voter_allowed_at_pv10() {
    // PV 10 — still below the hardfork gate (PV > 10).
    // Upstream: `harforkConwayDisallowUnelectedCommitteeFromVoting pv = pvMajor pv > natVersion @10`
    let mut state = conway_state_pv(10, 0, 2_000_000);
    let gov_action_id = seed_governance_action(&mut state);

    let cold_cred = StakeCredential::AddrKeyHash([0xCC; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);

    let rogue_hot = [0xEE; 28];
    let mut procedures = std::collections::BTreeMap::new();
    let mut votes_for_voter = std::collections::BTreeMap::new();
    votes_for_voter.insert(
        gov_action_id,
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures.insert(Voter::CommitteeKeyHash(rogue_hot), votes_for_voter);
    let voting_procedures = VotingProcedures { procedures };

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x07; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 10_000_000,
        },
    );

    let block = make_conway_block(
        10,
        2,
        0x07,
        vec![conway_tx_with_votes(
            [0x07; 32],
            10_000_000,
            0,
            voting_procedures,
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    // At PV 10 the unelected-committee check is skipped — we should NOT
    // see UnelectedCommitteeVoters.
    if let LedgerError::UnelectedCommitteeVoters(_) = err {
        panic!("should not get UnelectedCommitteeVoters at PV 10");
    }
}

#[test]
fn resigned_committee_member_hot_cred_rejected_at_pv11() {
    let mut state = conway_state_pv(11, 0, 2_000_000);
    let gov_action_id = seed_governance_action(&mut state);

    // Register committee member and authorize hot credential via block.
    let cold_cred = StakeCredential::AddrKeyHash([0xCC; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xDD; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    authorize_committee_via_block(&mut state, cold_cred, hot_cred, 0xF2);

    // Resign the member through a block.
    let resign_input = ShelleyTxIn {
        transaction_id: [0xF3; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert_shelley(
        resign_input.clone(),
        ShelleyTxOut {
            address: vec![0x01],
            amount: 1_000_000,
        },
    );
    let resign_block = make_conway_block(
        2,
        2,
        0xF3,
        vec![ConwayTxBody {
            inputs: vec![resign_input],
            outputs: vec![BabbageTxOut {
                address: vec![0x02],
                amount: Value::Coin(1_000_000),
                datum_option: None,
                script_ref: None,
            }],
            fee: 0,
            ttl: Some(100),
            certificates: Some(vec![DCert::CommitteeResignation(cold_cred, None)]),
            validity_interval_start: None,
            mint: None,
            collateral: None,
            required_signers: None,
            network_id: None,
            collateral_return: None,
            total_collateral: None,
            reference_inputs: None,
            script_data_hash: None,
            withdrawals: None,
            voting_procedures: None,
            proposal_procedures: None,
            treasury_donation: None,
            current_treasury_value: None,
            auxiliary_data_hash: None,
        }],
    );
    state.apply_block(&resign_block).expect("resignation block");

    // Vote using the formerly-authorized hot credential — should fail.
    let mut procedures = std::collections::BTreeMap::new();
    let mut votes_for_voter = std::collections::BTreeMap::new();
    votes_for_voter.insert(
        gov_action_id,
        VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures.insert(Voter::CommitteeKeyHash([0xDD; 28]), votes_for_voter);
    let voting_procedures = VotingProcedures { procedures };

    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x04; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: 10_000_000,
        },
    );

    let block = make_conway_block(
        10,
        3,
        0x04,
        vec![conway_tx_with_votes(
            [0x04; 32],
            10_000_000,
            0,
            voting_procedures,
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::UnelectedCommitteeVoters(creds) => {
            assert_eq!(creds, vec![StakeCredential::AddrKeyHash([0xDD; 28])]);
        }
        other => panic!("expected UnelectedCommitteeVoters, got {other:?}"),
    }
}

// -----------------------------------------------------------------------
// 2. DelegateeDRepNotRegistered
// -----------------------------------------------------------------------

#[test]
fn delegation_to_unregistered_drep_returns_delegatee_error() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register a stake credential.
    let cred = StakeCredential::AddrKeyHash([0xF1; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );

    // Attempt delegation to an unregistered KeyHash DRep.
    let drep = DRep::KeyHash([0xF2; 28]);

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x10; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x10,
        vec![conway_tx_single_cert(
            [0x10; 32],
            consumed,
            0,
            DCert::DelegationToDrep(cred, drep),
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(err, LedgerError::DelegateeDRepNotRegistered(drep));
}

#[test]
fn delegation_to_always_abstain_succeeds_without_drep_registration() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xF3; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x11; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x11,
        vec![conway_tx_single_cert(
            [0x11; 32],
            consumed,
            0,
            DCert::DelegationToDrep(cred, DRep::AlwaysAbstain),
        )],
    );

    state
        .apply_block(&block)
        .expect("AlwaysAbstain delegation should succeed");
}

#[test]
fn delegation_to_always_no_confidence_succeeds_without_drep_registration() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xF4; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x12; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x12,
        vec![conway_tx_single_cert(
            [0x12; 32],
            consumed,
            0,
            DCert::DelegationToDrep(cred, DRep::AlwaysNoConfidence),
        )],
    );

    state
        .apply_block(&block)
        .expect("AlwaysNoConfidence delegation should succeed");
}

// -----------------------------------------------------------------------
// 3. Conway AccountUnregistrationDeposit with non-zero reward balance
// -----------------------------------------------------------------------

#[test]
fn conway_unreg_deposit_rejects_nonzero_rewards() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state_pv(10, 0, key_deposit);

    // Register credential and seed a non-zero reward balance.
    let cred = StakeCredential::AddrKeyHash([0xD1; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(5_000_000, None), // non-zero!
    );
    state.deposit_pot_mut().add_key_deposit(key_deposit);

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x20; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x20,
        vec![conway_tx_single_cert(
            [0x20; 32],
            input_amount + key_deposit,
            0,
            DCert::AccountUnregistrationDeposit(cred, key_deposit),
        )],
    );

    // Upstream: ConwayUnRegCert asserts StakeKeyHasNonZeroAccountBalanceDELEG
    // just like Shelley tag 1 — non-zero rewards must be rejected.
    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::StakeCredentialHasRewards {
            credential,
            balance,
        } => {
            assert_eq!(credential, cred);
            assert_eq!(balance, 5_000_000);
        }
        other => panic!("expected StakeCredentialHasRewards, got {other:?}"),
    }
}

#[test]
fn shelley_unreg_still_rejects_nonzero_rewards() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state_pv(10, 0, key_deposit);

    // Register credential and seed a non-zero reward balance.
    let cred = StakeCredential::AddrKeyHash([0xD2; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(5_000_000, None), // non-zero!
    );
    state.deposit_pot_mut().add_key_deposit(key_deposit);

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x21; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    // Shelley-style AccountUnregistration (tag 1) in Conway era should
    // STILL reject non-zero reward balance.
    let block = make_conway_block(
        10,
        1,
        0x21,
        vec![conway_tx_single_cert(
            [0x21; 32],
            input_amount + key_deposit,
            0,
            DCert::AccountUnregistration(cred),
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    match err {
        LedgerError::StakeCredentialHasRewards {
            credential,
            balance,
        } => {
            assert_eq!(credential, cred);
            assert_eq!(balance, 5_000_000);
        }
        other => panic!("expected StakeCredentialHasRewards, got {other:?}"),
    }
}

#[test]
fn conway_unreg_deposit_with_zero_rewards_succeeds() {
    let key_deposit = 2_000_000u64;
    let mut state = conway_state_pv(10, 0, key_deposit);

    let cred = StakeCredential::AddrKeyHash([0xD3; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );
    state.deposit_pot_mut().add_key_deposit(key_deposit);

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x22; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x22,
        vec![conway_tx_single_cert(
            [0x22; 32],
            input_amount + key_deposit,
            0,
            DCert::AccountUnregistrationDeposit(cred, key_deposit),
        )],
    );

    state
        .apply_block(&block)
        .expect("Conway unreg-deposit with zero balance should succeed");
    assert!(!state.stake_credentials().is_registered(&cred));
}

// -----------------------------------------------------------------------
// 4. DRep unregistration clears delegations (upstream clearDRepDelegations)
// -----------------------------------------------------------------------

#[test]
fn drep_unreg_clears_delegations() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register a DRep.
    let drep_cred = StakeCredential::AddrKeyHash([0xE1; 28]);
    let drep = DRep::KeyHash([0xE1; 28]);
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new(500_000, None));

    // Register two stakers and delegate them to this DRep.
    let cred_a = StakeCredential::AddrKeyHash([0xA1; 28]);
    let cred_b = StakeCredential::AddrKeyHash([0xA2; 28]);
    state.stake_credentials_mut().register(cred_a);
    state.stake_credentials_mut().register(cred_b);
    state
        .stake_credentials_mut()
        .get_mut(&cred_a)
        .unwrap()
        .set_delegated_drep(Some(drep));
    state
        .stake_credentials_mut()
        .get_mut(&cred_b)
        .unwrap()
        .set_delegated_drep(Some(drep));

    // Verify delegations are set.
    assert_eq!(
        state
            .stake_credentials()
            .get(&cred_a)
            .unwrap()
            .delegated_drep(),
        Some(drep)
    );
    assert_eq!(
        state
            .stake_credentials()
            .get(&cred_b)
            .unwrap()
            .delegated_drep(),
        Some(drep)
    );

    // Seed UTxO for the DRep unregistration transaction.
    // Value preservation: input = output + fee + 0 (deposit refunded via cert)
    // The DRep deposit (500_000) is refunded, so output = input + refund.
    let input_amount = 10_000_000u64;
    state.deposit_pot_mut().add_drep_deposit(500_000);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x30; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x30,
        vec![conway_tx_single_cert(
            [0x30; 32],
            input_amount + 500_000, // captures the refunded DRep deposit
            0,
            DCert::DrepUnregistration(drep_cred, 500_000),
        )],
    );

    state
        .apply_block(&block)
        .expect("DRep unregistration block");

    // DRep should be unregistered.
    assert!(!state.drep_state().is_registered(&drep));

    // Both stakers' DRep delegations should be cleared.
    assert_eq!(
        state
            .stake_credentials()
            .get(&cred_a)
            .unwrap()
            .delegated_drep(),
        None
    );
    assert_eq!(
        state
            .stake_credentials()
            .get(&cred_b)
            .unwrap()
            .delegated_drep(),
        None
    );
}

#[test]
fn drep_unreg_does_not_clear_other_drep_delegations() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register two DReps.
    let drep_1 = DRep::KeyHash([0xE1; 28]);
    let drep_2 = DRep::KeyHash([0xE2; 28]);
    state
        .drep_state_mut()
        .register(drep_1, RegisteredDrep::new(500_000, None));
    state
        .drep_state_mut()
        .register(drep_2, RegisteredDrep::new(500_000, None));

    // Staker A -> DRep 1, Staker B -> DRep 2.
    let cred_a = StakeCredential::AddrKeyHash([0xA1; 28]);
    let cred_b = StakeCredential::AddrKeyHash([0xA2; 28]);
    state.stake_credentials_mut().register(cred_a);
    state.stake_credentials_mut().register(cred_b);
    state
        .stake_credentials_mut()
        .get_mut(&cred_a)
        .unwrap()
        .set_delegated_drep(Some(drep_1));
    state
        .stake_credentials_mut()
        .get_mut(&cred_b)
        .unwrap()
        .set_delegated_drep(Some(drep_2));

    // Unregister DRep 1.
    let drep_1_cred = StakeCredential::AddrKeyHash([0xE1; 28]);
    let input_amount = 10_000_000u64;
    state.deposit_pot_mut().add_drep_deposit(500_000);
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x31; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x31,
        vec![conway_tx_single_cert(
            [0x31; 32],
            input_amount + 500_000,
            0,
            DCert::DrepUnregistration(drep_1_cred, 500_000),
        )],
    );

    state
        .apply_block(&block)
        .expect("DRep 1 unregistration block");

    // Staker A's delegation should be cleared (was delegated to DRep 1).
    assert_eq!(
        state
            .stake_credentials()
            .get(&cred_a)
            .unwrap()
            .delegated_drep(),
        None
    );
    // Staker B's delegation to DRep 2 should be unaffected.
    assert_eq!(
        state
            .stake_credentials()
            .get(&cred_b)
            .unwrap()
            .delegated_drep(),
        Some(drep_2)
    );
}

// -----------------------------------------------------------------------
// 5. Committee isPotentialFutureMember
// -----------------------------------------------------------------------

#[test]
fn committee_auth_for_potential_future_member_succeeds() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Cold credential that is NOT yet a committee member.
    let cold_cred = StakeCredential::AddrKeyHash([0xC1; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xC2; 28]);

    // Seed a pending UpdateCommittee proposal that includes cold_cred.
    let gov_action_id = GovActionId {
        transaction_id: [0xBB; 32],
        gov_action_index: 0,
    };
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(cold_cred, 100u64); // term-limit epoch
    state.governance_actions_mut().insert(
        gov_action_id,
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account_bytes([0x99; 28]),
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 2,
                    denominator: 3,
                },
            },
            anchor: Anchor {
                url: "https://example.com".to_string(),
                data_hash: [0u8; 32],
            },
        }),
    );

    // Authorize the cold credential — should succeed via isPotentialFutureMember.
    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x40; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x40,
        vec![conway_tx_single_cert(
            [0x40; 32],
            input_amount,
            0,
            DCert::CommitteeAuthorization(cold_cred, hot_cred),
        )],
    );

    state
        .apply_block(&block)
        .expect("isPotentialFutureMember authorization should succeed");

    // The cold credential should now be registered and authorized.
    let member = state.committee_state().get(&cold_cred);
    assert!(
        member.is_some(),
        "cold credential should be auto-registered"
    );
}

#[test]
fn committee_auth_unknown_cred_without_proposal_fails() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Cold credential that is NOT a committee member and NOT in any proposal.
    let cold_cred = StakeCredential::AddrKeyHash([0xC3; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xC4; 28]);

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x41; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x41,
        vec![conway_tx_single_cert(
            [0x41; 32],
            input_amount,
            0,
            DCert::CommitteeAuthorization(cold_cred, hot_cred),
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    assert!(matches!(err, LedgerError::CommitteeIsUnknown(c) if c == cold_cred));
}

#[test]
fn committee_resign_for_potential_future_member_succeeds() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Cold credential that is NOT yet a committee member.
    let cold_cred = StakeCredential::AddrKeyHash([0xC5; 28]);

    // Seed a pending UpdateCommittee proposal that includes cold_cred.
    let gov_action_id = GovActionId {
        transaction_id: [0xBC; 32],
        gov_action_index: 0,
    };
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(cold_cred, 200u64);
    state.governance_actions_mut().insert(
        gov_action_id,
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account_bytes([0x99; 28]),
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: Anchor {
                url: "https://example.com".to_string(),
                data_hash: [0u8; 32],
            },
        }),
    );

    // Resign the cold credential — should succeed via isPotentialFutureMember.
    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x42; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x42,
        vec![conway_tx_single_cert(
            [0x42; 32],
            input_amount,
            0,
            DCert::CommitteeResignation(cold_cred, None),
        )],
    );

    state
        .apply_block(&block)
        .expect("isPotentialFutureMember resignation should succeed");

    // The cold credential should now be registered as resigned.
    let member = state.committee_state().get(&cold_cred);
    assert!(
        member.is_some(),
        "cold credential should be auto-registered"
    );
    assert!(
        member.unwrap().is_resigned(),
        "cold credential should be resigned"
    );
}

// -----------------------------------------------------------------------
// 5b. Committee resignation preservation (upstream csCommitteeCreds parity)
//
// Upstream `checkAndOverwriteCommitteeMemberState` checks resignation in
// `csCommitteeCreds` BEFORE checking membership in `committeeMembers`.
// Importantly, `UpdateCommittee` enactment modifies `committeeMembers`
// but does NOT touch `csCommitteeCreds` — so a resigned member re-added
// via `UpdateCommittee` remains resigned and gets
// `ConwayCommitteeHasPreviouslyResigned` on re-authorization attempt.
//
// Reference: `Cardano.Ledger.Conway.Rules.GovCert` —
// `checkAndOverwriteCommitteeMemberState`.
// -----------------------------------------------------------------------

#[test]
fn resigned_member_readded_via_update_committee_still_resigned() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register committee member and authorize a hot credential.
    let cold_cred = StakeCredential::AddrKeyHash([0xE1; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xE2; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    authorize_committee_via_block(&mut state, cold_cred, hot_cred, 0xE0);

    // Resign the member through a block.
    let resign_input = ShelleyTxIn {
        transaction_id: [0xE1; 32],
        index: 0,
    };
    add_utxo(&mut state, resign_input.clone(), 10_000_000);
    let resign_block = make_conway_block(
        10,
        2,
        0xE1,
        vec![conway_tx_single_cert(
            [0xE1; 32],
            10_000_000,
            0,
            DCert::CommitteeResignation(cold_cred, None),
        )],
    );
    state
        .apply_block(&resign_block)
        .expect("resignation block should succeed");
    assert!(
        state
            .committee_state()
            .get(&cold_cred)
            .unwrap()
            .is_resigned()
    );

    // Re-add the resigned member via UpdateCommittee enactment.
    // This simulates the governance action being enacted — it should only
    // update committeeMembers (expires_at) without clearing resignation
    // state in csCommitteeCreds.
    let action_id = GovActionId {
        transaction_id: [0xE2; 32],
        gov_action_index: 0,
    };
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(cold_cred, 300u64); // new term epoch
    state.enact_action(
        action_id,
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        },
    );

    // Verify: the member has an updated term but is STILL resigned.
    let member = state.committee_state().get(&cold_cred).unwrap();
    assert_eq!(member.expires_at(), Some(300), "term should be updated");
    assert!(member.is_resigned(), "resignation should be preserved");

    // Attempt to re-authorize → should get CommitteeHasPreviouslyResigned.
    let reauth_input = ShelleyTxIn {
        transaction_id: [0xE3; 32],
        index: 0,
    };
    add_utxo(&mut state, reauth_input.clone(), 10_000_000);
    let new_hot = StakeCredential::AddrKeyHash([0xE4; 28]);
    let reauth_block = make_conway_block(
        20,
        3,
        0xE3,
        vec![conway_tx_single_cert(
            [0xE3; 32],
            10_000_000,
            0,
            DCert::CommitteeAuthorization(cold_cred, new_hot),
        )],
    );
    let err = state.apply_block(&reauth_block).unwrap_err();
    assert!(
        matches!(err, LedgerError::CommitteeHasPreviouslyResigned(c) if c == cold_cred),
        "re-auth of resigned member should fail with CommitteeHasPreviouslyResigned, got: {err:?}"
    );
}

#[test]
fn no_confidence_then_readd_preserves_resignation() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register, authorize, then resign a committee member.
    let cold_cred = StakeCredential::AddrKeyHash([0xF1; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xF2; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    authorize_committee_via_block(&mut state, cold_cred, hot_cred, 0xF0);

    let resign_input = ShelleyTxIn {
        transaction_id: [0xF1; 32],
        index: 0,
    };
    add_utxo(&mut state, resign_input.clone(), 10_000_000);
    let resign_block = make_conway_block(
        10,
        2,
        0xF1,
        vec![conway_tx_single_cert(
            [0xF1; 32],
            10_000_000,
            0,
            DCert::CommitteeResignation(cold_cred, None),
        )],
    );
    state.apply_block(&resign_block).expect("resignation block");
    assert!(
        state
            .committee_state()
            .get(&cold_cred)
            .unwrap()
            .is_resigned()
    );

    // Enact NoConfidence — clears membership but preserves resignation.
    let nc_id = GovActionId {
        transaction_id: [0xF2; 32],
        gov_action_index: 0,
    };
    state.enact_action(
        nc_id,
        &GovAction::NoConfidence {
            prev_action_id: None,
        },
    );
    let member = state.committee_state().get(&cold_cred).unwrap();
    assert!(
        member.expires_at().is_none(),
        "membership cleared by NoConfidence"
    );
    assert!(
        member.is_resigned(),
        "resignation preserved after NoConfidence"
    );

    // Re-add via UpdateCommittee.
    let uc_id = GovActionId {
        transaction_id: [0xF3; 32],
        gov_action_index: 0,
    };
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(cold_cred, 400u64);
    state.enact_action(
        uc_id,
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
    );

    // Verify: member re-enrolled but STILL resigned.
    let member = state.committee_state().get(&cold_cred).unwrap();
    assert_eq!(member.expires_at(), Some(400));
    assert!(
        member.is_resigned(),
        "resignation preserved after NoConfidence + re-add"
    );

    // Re-auth attempt fails.
    let reauth_input = ShelleyTxIn {
        transaction_id: [0xF4; 32],
        index: 0,
    };
    add_utxo(&mut state, reauth_input.clone(), 10_000_000);
    let new_hot = StakeCredential::AddrKeyHash([0xF5; 28]);
    let reauth_block = make_conway_block(
        20,
        3,
        0xF4,
        vec![conway_tx_single_cert(
            [0xF4; 32],
            10_000_000,
            0,
            DCert::CommitteeAuthorization(cold_cred, new_hot),
        )],
    );
    let err = state.apply_block(&reauth_block).unwrap_err();
    assert!(
        matches!(err, LedgerError::CommitteeHasPreviouslyResigned(c) if c == cold_cred),
        "re-auth after NoConfidence + re-add should fail, got: {err:?}"
    );
}

#[test]
fn resignation_check_before_unknown_check() {
    // Upstream ordering: resigned checked BEFORE unknown.
    // If a credential resigned via `isPotentialFutureMember` path (no
    // enacted membership) and then the proposal is removed, the FIRST
    // error should be CommitteeHasPreviouslyResigned, NOT CommitteeIsUnknown.
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Seed a pending UpdateCommittee proposal so the credential qualifies
    // as a potential future member.
    let cold_cred = StakeCredential::AddrKeyHash([0xA1; 28]);
    let gov_action_id = GovActionId {
        transaction_id: [0xAA; 32],
        gov_action_index: 0,
    };
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(cold_cred, 100u64);
    state.governance_actions_mut().insert(
        gov_action_id.clone(),
        GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: reward_account_bytes([0x99; 28]),
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: Anchor {
                url: "https://example.com".to_string(),
                data_hash: [0u8; 32],
            },
        }),
    );

    // Resign via isPotentialFutureMember path.
    let resign_input = ShelleyTxIn {
        transaction_id: [0xA1; 32],
        index: 0,
    };
    add_utxo(&mut state, resign_input.clone(), 10_000_000);
    let resign_block = make_conway_block(
        10,
        1,
        0xA1,
        vec![conway_tx_single_cert(
            [0xA1; 32],
            10_000_000,
            0,
            DCert::CommitteeResignation(cold_cred, None),
        )],
    );
    state
        .apply_block(&resign_block)
        .expect("resignation via future member path");
    assert!(
        state
            .committee_state()
            .get(&cold_cred)
            .unwrap()
            .is_resigned()
    );

    // Remove the proposal — now the credential is neither enacted nor a
    // future member, but it IS resigned.
    state.governance_actions_mut().remove(&gov_action_id);

    // Try to authorize — should get CommitteeHasPreviouslyResigned (not Unknown).
    let auth_input = ShelleyTxIn {
        transaction_id: [0xA2; 32],
        index: 0,
    };
    add_utxo(&mut state, auth_input.clone(), 10_000_000);
    let hot = StakeCredential::AddrKeyHash([0xA3; 28]);
    let auth_block = make_conway_block(
        20,
        2,
        0xA2,
        vec![conway_tx_single_cert(
            [0xA2; 32],
            10_000_000,
            0,
            DCert::CommitteeAuthorization(cold_cred, hot),
        )],
    );
    let err = state.apply_block(&auth_block).unwrap_err();
    assert!(
        matches!(err, LedgerError::CommitteeHasPreviouslyResigned(c) if c == cold_cred),
        "resigned credential should get CommitteeHasPreviouslyResigned, not CommitteeIsUnknown, got: {err:?}"
    );
}

// -----------------------------------------------------------------------
// 6. ZeroTreasuryWithdrawals bootstrap gate
// -----------------------------------------------------------------------

#[test]
fn zero_treasury_withdrawal_blocked_during_bootstrap() {
    // PV 9 = bootstrap phase.  TreasuryWithdrawals is not a bootstrap
    // action, so the entire proposal is rejected as
    // `DisallowedProposalDuringBootstrap` before ZeroTreasuryWithdrawals
    // can fire.
    let mut state = conway_state_pv(9, 0, 2_000_000);

    // Register the proposal's return reward account.
    let ra_cred = StakeCredential::AddrKeyHash([0x99; 28]);
    state.stake_credentials_mut().register(ra_cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: ra_cred,
        },
        RewardAccountState::new(0, None),
    );

    let target_cred = StakeCredential::AddrKeyHash([0xB1; 28]);
    state.stake_credentials_mut().register(target_cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: target_cred,
        },
        RewardAccountState::new(0, None),
    );

    state.enact_state_mut().constitution.guardrails_script_hash = None;

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(
        RewardAccount {
            network: 1,
            credential: target_cred,
        },
        0u64,
    );

    let proposal = ProposalProcedure {
        deposit: 0,
        reward_account: reward_account_bytes([0x99; 28]),
        gov_action: GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0u8; 32],
        },
    };

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x50; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x50,
        vec![conway_tx_with_proposal(
            [0x50; 32],
            input_amount,
            0,
            vec![proposal],
        )],
    );

    // During bootstrap, TreasuryWithdrawals is not a valid action,
    // so the error is NOT ZeroTreasuryWithdrawals.
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::DisallowedProposalDuringBootstrap(_)),
        "expected DisallowedProposalDuringBootstrap, got {err:?}",
    );
}

#[test]
fn zero_treasury_withdrawal_rejected_after_bootstrap() {
    // PV 10 = post-bootstrap phase.
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register the proposal's return reward account (prevents
    // ProposalReturnAccountDoesNotExist).
    let ra_cred = StakeCredential::AddrKeyHash([0x99; 28]);
    state.stake_credentials_mut().register(ra_cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: ra_cred,
        },
        RewardAccountState::new(0, None),
    );

    let target_cred = StakeCredential::AddrKeyHash([0xB2; 28]);
    state.stake_credentials_mut().register(target_cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: target_cred,
        },
        RewardAccountState::new(0, None),
    );

    state.enact_state_mut().constitution.guardrails_script_hash = None;

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(
        RewardAccount {
            network: 1,
            credential: target_cred,
        },
        0u64,
    );

    let proposal = ProposalProcedure {
        deposit: 0,
        reward_account: reward_account_bytes([0x99; 28]),
        gov_action: GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0u8; 32],
        },
    };

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x51; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x51,
        vec![conway_tx_with_proposal(
            [0x51; 32],
            input_amount,
            0,
            vec![proposal],
        )],
    );

    // After bootstrap (PV 10), zero-sum treasury withdrawals are rejected.
    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::ZeroTreasuryWithdrawals(_)),
        "expected ZeroTreasuryWithdrawals, got {err:?}",
    );
}

#[test]
fn nonzero_treasury_withdrawal_accepted_after_bootstrap() {
    // PV 10 = post-bootstrap.  A proper (non-zero) TreasuryWithdrawals
    // should be accepted.
    let mut state = conway_state_pv(10, 0, 2_000_000);

    // Register the proposal's return reward account.
    let ra_cred = StakeCredential::AddrKeyHash([0x99; 28]);
    state.stake_credentials_mut().register(ra_cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: ra_cred,
        },
        RewardAccountState::new(0, None),
    );

    let target_cred = StakeCredential::AddrKeyHash([0xB3; 28]);
    state.stake_credentials_mut().register(target_cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: target_cred,
        },
        RewardAccountState::new(0, None),
    );

    state.enact_state_mut().constitution.guardrails_script_hash = None;

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(
        RewardAccount {
            network: 1,
            credential: target_cred,
        },
        1_000_000u64, // non-zero
    );

    let proposal = ProposalProcedure {
        deposit: 0,
        reward_account: reward_account_bytes([0x99; 28]),
        gov_action: GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0u8; 32],
        },
    };

    let input_amount = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0x52; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: input_amount,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0x52,
        vec![conway_tx_with_proposal(
            [0x52; 32],
            input_amount,
            0,
            vec![proposal],
        )],
    );

    state
        .apply_block(&block)
        .expect("non-zero treasury withdrawal should be accepted");
}

// -----------------------------------------------------------------------
// 7. Bootstrap-phase DRep delegation (preserveIncorrectDelegation)
// -----------------------------------------------------------------------

/// During bootstrap phase (PV 9), delegating to an unregistered KeyHash DRep
/// is allowed — upstream `preserveIncorrectDelegation` /
/// `hardforkConwayBootstrapPhase` from `Cardano.Ledger.Conway.Rules.Deleg`.
#[test]
fn bootstrap_phase_allows_delegation_to_unregistered_drep() {
    let mut state = conway_state_pv(9, 0, 2_000_000);

    // Register a stake credential.
    let cred = StakeCredential::AddrKeyHash([0xB1; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );

    // Attempt delegation to an unregistered KeyHash DRep — during bootstrap
    // this should succeed (upstream skips checkDRepRegistered).
    let drep = DRep::KeyHash([0xB2; 28]);

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xB0; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0xB0,
        vec![conway_tx_single_cert(
            [0xB0; 32],
            consumed,
            0,
            DCert::DelegationToDrep(cred, drep),
        )],
    );

    state
        .apply_block(&block)
        .expect("bootstrap phase should allow delegation to unregistered DRep");
}

/// Same scenario at PV 10 (post-bootstrap) must be rejected.
#[test]
fn post_bootstrap_rejects_delegation_to_unregistered_drep() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xB3; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );

    let drep = DRep::KeyHash([0xB4; 28]);

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xB5; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0xB5,
        vec![conway_tx_single_cert(
            [0xB5; 32],
            consumed,
            0,
            DCert::DelegationToDrep(cred, drep),
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(err, LedgerError::DelegateeDRepNotRegistered(drep));
}

/// Bootstrap phase allows DelegationToStakePoolAndDrep with an unregistered
/// DRep — dual delegation variant of the same upstream gate.
#[test]
fn bootstrap_phase_allows_dual_delegation_to_unregistered_drep() {
    let mut state = conway_state_pv(9, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xB6; 28]);
    state.stake_credentials_mut().register(cred);
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: cred,
        },
        RewardAccountState::new(0, None),
    );

    // Register a pool so pool delegation leg succeeds.
    let pool_id: [u8; 28] = [0xBB; 28];
    state.pool_state_mut().register(PoolParams {
        operator: pool_id,
        vrf_keyhash: [0u8; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xB6; 28]),
        },
        pool_owners: vec![pool_id],
        relays: vec![],
        pool_metadata: None,
    });

    let drep = DRep::KeyHash([0xB7; 28]); // NOT registered

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xB8; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0xB8,
        vec![conway_tx_single_cert(
            [0xB8; 32],
            consumed,
            0,
            DCert::DelegationToStakePoolAndDrep(cred, pool_id, drep),
        )],
    );

    state
        .apply_block(&block)
        .expect("bootstrap phase should allow dual delegation with unregistered DRep");
}

/// AccountRegistrationDelegationToDrep with unregistered DRep during bootstrap.
#[test]
fn bootstrap_phase_allows_reg_deleg_to_unregistered_drep() {
    let mut state = conway_state_pv(9, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xB9; 28]);
    let drep = DRep::KeyHash([0xBA; 28]); // NOT registered

    let key_deposit = 2_000_000u64;
    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xBC; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let block = make_conway_block(
        10,
        1,
        0xBC,
        vec![conway_tx_single_cert(
            [0xBC; 32],
            consumed - key_deposit,
            0,
            DCert::AccountRegistrationDelegationToDrep(cred, drep, key_deposit),
        )],
    );

    state.apply_block(&block).expect(
        "bootstrap phase should allow AccountRegistrationDelegationToDrep with unregistered DRep",
    );
}

// ---------------------------------------------------------------------------
// 9. ConwayWdrlNotDelegatedToDRep — upstream `validateWithdrawalsDelegated`
//    from `Cardano.Ledger.Conway.Rules.Ledger`.
//    Post-bootstrap only: key-hash withdrawal credentials must have a DRep
//    delegation in the pre-CERTS stake credential state.
// ---------------------------------------------------------------------------

fn conway_tx_with_withdrawal(
    input_hash: [u8; 32],
    output_coin: u64,
    fee: u64,
    withdrawals: std::collections::BTreeMap<RewardAccount, u64>,
) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: input_hash,
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x02],
            amount: Value::Coin(output_coin),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: Some(200),
        certificates: None,
        validity_interval_start: None,
        mint: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        script_data_hash: None,
        withdrawals: Some(withdrawals),
        voting_procedures: None,
        proposal_procedures: None,
        treasury_donation: None,
        current_treasury_value: None,
        auxiliary_data_hash: None,
    }
}

/// Post-bootstrap: withdrawal from a key-hash credential that is NOT
/// delegated to a DRep should be rejected.
#[test]
fn post_bootstrap_rejects_withdrawal_without_drep_delegation() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xC1; 28]);
    let account = RewardAccount {
        network: 1,
        credential: cred,
    };

    // Register the stake credential (no DRep delegation).
    state.stake_credentials_mut().register(cred);
    state
        .reward_accounts_mut()
        .insert(account, RewardAccountState::new(1_000, None));

    let consumed = 10_000_000u64;
    let withdrawal_amount = 1_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xC2; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let mut wdrls = std::collections::BTreeMap::new();
    wdrls.insert(account, withdrawal_amount);

    let block = make_conway_block(
        10,
        1,
        0xC2,
        vec![conway_tx_with_withdrawal(
            [0xC2; 32],
            consumed + withdrawal_amount,
            0,
            wdrls,
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    assert!(
        matches!(err, LedgerError::WithdrawalNotDelegatedToDRep { credential } if credential == [0xC1; 28]),
        "expected WithdrawalNotDelegatedToDRep, got: {err:?}",
    );
}

/// Post-bootstrap: withdrawal from a key-hash credential that IS delegated
/// to a DRep should succeed.
#[test]
fn post_bootstrap_accepts_withdrawal_with_drep_delegation() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xC3; 28]);
    let drep = DRep::KeyHash([0xC4; 28]);
    let account = RewardAccount {
        network: 1,
        credential: cred,
    };

    // Register the stake credential WITH a DRep delegation.
    state.stake_credentials_mut().register(cred);
    state
        .stake_credentials_mut()
        .get_mut(&cred)
        .unwrap()
        .set_delegated_drep(Some(drep));
    state
        .reward_accounts_mut()
        .insert(account, RewardAccountState::new(1_000, None));

    // Register the DRep (required for non-bootstrap).
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new(0, None));

    let consumed = 10_000_000u64;
    let withdrawal_amount = 1_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xC5; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let mut wdrls = std::collections::BTreeMap::new();
    wdrls.insert(account, withdrawal_amount);

    let block = make_conway_block(
        10,
        1,
        0xC5,
        vec![conway_tx_with_withdrawal(
            [0xC5; 32],
            consumed + withdrawal_amount,
            0,
            wdrls,
        )],
    );

    state
        .apply_block(&block)
        .expect("post-bootstrap withdrawal with DRep delegation should succeed");
}

/// Bootstrap phase (PV 9): withdrawal from a credential without DRep
/// delegation should succeed (check is skipped during bootstrap).
#[test]
fn bootstrap_phase_allows_withdrawal_without_drep_delegation() {
    let mut state = conway_state_pv(9, 0, 2_000_000);

    let cred = StakeCredential::AddrKeyHash([0xC6; 28]);
    let account = RewardAccount {
        network: 1,
        credential: cred,
    };

    // Register the stake credential (no DRep delegation).
    state.stake_credentials_mut().register(cred);
    state
        .reward_accounts_mut()
        .insert(account, RewardAccountState::new(500, None));

    let consumed = 10_000_000u64;
    let withdrawal_amount = 500u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xC7; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let mut wdrls = std::collections::BTreeMap::new();
    wdrls.insert(account, withdrawal_amount);

    let block = make_conway_block(
        10,
        1,
        0xC7,
        vec![conway_tx_with_withdrawal(
            [0xC7; 32],
            consumed + withdrawal_amount,
            0,
            wdrls,
        )],
    );

    state
        .apply_block(&block)
        .expect("bootstrap phase should skip withdrawal delegation check");
}

/// Post-bootstrap: script-hash withdrawal credentials are NOT checked
/// for DRep delegation (upstream `credKeyHash` filters them out).
/// The withdrawal will fail with `MissingScriptWitness` because no
/// script witness is provided, but critically NOT with
/// `WithdrawalNotDelegatedToDRep`.
#[test]
fn post_bootstrap_allows_script_hash_withdrawal_without_drep() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let cred = StakeCredential::ScriptHash([0xC8; 28]);
    let account = RewardAccount {
        network: 1,
        credential: cred,
    };

    // Register the stake credential (script-hash, no DRep).
    state.stake_credentials_mut().register(cred);
    state
        .reward_accounts_mut()
        .insert(account, RewardAccountState::new(800, None));

    let consumed = 10_000_000u64;
    let withdrawal_amount = 800u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xC9; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let mut wdrls = std::collections::BTreeMap::new();
    wdrls.insert(account, withdrawal_amount);

    let block = make_conway_block(
        10,
        1,
        0xC9,
        vec![conway_tx_with_withdrawal(
            [0xC9; 32],
            consumed + withdrawal_amount,
            0,
            wdrls,
        )],
    );

    let err = state.apply_block(&block).unwrap_err();
    // Must NOT be WithdrawalNotDelegatedToDRep — script-hash credentials
    // are excluded from the DRep delegation check.
    assert!(
        !matches!(err, LedgerError::WithdrawalNotDelegatedToDRep { .. }),
        "script-hash withdrawal should not trigger DRep delegation check, got: {err:?}",
    );
}

// -----------------------------------------------------------------------
// 9. Interleaved proposal staging — same-tx proposal chaining
//    (upstream `foldlM'` + `processProposal` ordering)
// -----------------------------------------------------------------------

/// Verifies that proposal 1 can reference proposal 0 from a previous
/// transaction via the governance actions map, and the interleaved
/// validate+stage ordering (upstream `foldlM'` + `processProposal`)
/// stages each valid proposal before validating the next.
#[test]
fn conway_proposal_chaining_across_txs_succeeds() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xE1; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0xE2; 28]);
    state.stake_credentials_mut().register(cred);
    let return_account = RewardAccount {
        network: 1,
        credential: cred,
    };
    state
        .reward_accounts_mut()
        .insert(return_account, RewardAccountState::new(0, None));

    // --- Block 1: Stage a ParameterChange proposal (prev_action_id = None) ---
    let proposal_0 = ProposalProcedure {
        deposit: 0,
        reward_account: RewardAccount {
            network: 1,
            credential: cred,
        }
        .to_bytes()
        .to_vec(),
        gov_action: GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: {
                let mut ppu = ProtocolParameterUpdate::default();
                ppu.min_fee_a = Some(1);
                ppu
            },
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.invalid/proposal-0".to_string(),
            data_hash: [0xE3; 32],
        },
    };

    let body_0 = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xE1; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01],
            amount: Value::Coin(consumed),
            datum_option: None,
            script_ref: None,
        }],
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
        proposal_procedures: Some(vec![proposal_0]),
        current_treasury_value: None,
        treasury_donation: None,
    };
    let tx_id_0 = compute_tx_id(&body_0.to_cbor_bytes()).0;

    let block1 = make_conway_block(10, 1, 0xE1, vec![body_0]);
    state
        .apply_block(&block1)
        .expect("block 1 with initial proposal should succeed");

    // --- Block 2: Chain a second ParameterChange referencing proposal 0 ---
    let consumed_2 = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xE5; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed_2,
        },
    );

    let proposal_1 = ProposalProcedure {
        deposit: 0,
        reward_account: RewardAccount {
            network: 1,
            credential: cred,
        }
        .to_bytes()
        .to_vec(),
        gov_action: GovAction::ParameterChange {
            prev_action_id: Some(GovActionId {
                transaction_id: tx_id_0,
                gov_action_index: 0,
            }),
            protocol_param_update: {
                let mut ppu = ProtocolParameterUpdate::default();
                ppu.min_fee_b = Some(2);
                ppu
            },
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.invalid/proposal-1".to_string(),
            data_hash: [0xE4; 32],
        },
    };

    let body_1 = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xE5; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01],
            amount: Value::Coin(consumed_2),
            datum_option: None,
            script_ref: None,
        }],
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
        proposal_procedures: Some(vec![proposal_1]),
        current_treasury_value: None,
        treasury_donation: None,
    };

    let block2 = make_conway_block(20, 2, 0xE5, vec![body_1]);
    state.apply_block(&block2).expect(
        "proposal chaining across txs: proposal 1 references proposal 0 via prev_action_id",
    );
}

/// Verifies that a same-tx reference to a not-yet-validated proposal fails
/// correctly. Proposal 0 references proposal 1 (forward reference) which
/// violates the upstream invariant where proposals are processed in order.
#[test]
fn conway_same_tx_forward_reference_rejected() {
    let mut state = conway_state_pv(10, 0, 2_000_000);

    let consumed = 10_000_000u64;
    state.multi_era_utxo_mut().insert_shelley(
        ShelleyTxIn {
            transaction_id: [0xF1; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x01],
            amount: consumed,
        },
    );

    let cred = StakeCredential::AddrKeyHash([0xF2; 28]);
    state.stake_credentials_mut().register(cred);
    let return_account = RewardAccount {
        network: 1,
        credential: cred,
    };
    state
        .reward_accounts_mut()
        .insert(return_account, RewardAccountState::new(0, None));

    let body_template = ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xF1; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01],
            amount: Value::Coin(consumed),
            datum_option: None,
            script_ref: None,
        }],
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
        current_treasury_value: None,
        treasury_donation: None,
    };
    let tx_id_bytes = compute_tx_id(&body_template.to_cbor_bytes()).0;

    // Proposal 0 references proposal 1 (forward reference — index 1 >= own index 0)
    let proposal_0_fwd = ProposalProcedure {
        deposit: 0,
        reward_account: RewardAccount {
            network: 1,
            credential: cred,
        }
        .to_bytes()
        .to_vec(),
        gov_action: GovAction::ParameterChange {
            prev_action_id: Some(GovActionId {
                transaction_id: tx_id_bytes,
                gov_action_index: 1,
            }),
            protocol_param_update: {
                let mut ppu = ProtocolParameterUpdate::default();
                ppu.min_fee_a = Some(1);
                ppu
            },
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.invalid/forward-ref-0".to_string(),
            data_hash: [0xF3; 32],
        },
    };

    let proposal_1_info = ProposalProcedure {
        deposit: 0,
        reward_account: RewardAccount {
            network: 1,
            credential: cred,
        }
        .to_bytes()
        .to_vec(),
        gov_action: GovAction::InfoAction,
        anchor: Anchor {
            url: "https://example.invalid/forward-ref-1".to_string(),
            data_hash: [0xF4; 32],
        },
    };

    let body = ConwayTxBody {
        proposal_procedures: Some(vec![proposal_0_fwd.clone(), proposal_1_info]),
        ..body_template
    };

    let block = make_conway_block(10, 1, 0xF1, vec![body]);
    let err = state.apply_block(&block).unwrap_err();
    assert_eq!(err, LedgerError::InvalidPrevGovActionId(proposal_0_fwd));
}

// -----------------------------------------------------------------------
// 11. Ratification thresholds evolve after ParameterChange enactment
// -----------------------------------------------------------------------
//
// Upstream `ratifyTransition` recursively passes the updated `RatifyState`
// (including `ensCurPParams`) so that after a `ParameterChange` enactment,
// subsequent proposals see updated voting thresholds.
//
// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `ratifyTransition`:
//   `votingDRepThreshold` reads `rs ^. rsEnactStateL . ensCurPParamsL`
//   for each proposal evaluated.
//
// This test verifies that:
//   - Proposal A (ParameterChange, priority 4) is ratified first.
//   - Proposal B (TreasuryWithdrawals, priority 5) would fail under the
//     original 67% DRep threshold but passes with the zero threshold
//     enacted by Proposal A.

#[test]
fn ratification_thresholds_evolve_after_parameter_change() {
    use std::collections::BTreeMap;
    use yggdrasil_ledger::{
        GovernanceActionState, StakeSnapshot, StakeSnapshots, apply_epoch_boundary,
    };

    // Set up Conway PV 10 state (non-bootstrap).
    let mut state = conway_state_pv(10, 0, 2_000_000);
    state.set_current_epoch(EpochNo(100));

    // Configure DRep thresholds — treasury_withdrawal requires 67% stake.
    {
        let pp = state.protocol_params_mut().as_mut().unwrap();
        pp.drep_voting_thresholds = Some(yggdrasil_ledger::DRepVotingThresholds {
            motion_no_confidence: UnitInterval {
                numerator: 67,
                denominator: 100,
            },
            committee_normal: UnitInterval {
                numerator: 67,
                denominator: 100,
            },
            committee_no_confidence: UnitInterval {
                numerator: 60,
                denominator: 100,
            },
            update_to_constitution: UnitInterval {
                numerator: 75,
                denominator: 100,
            },
            hard_fork_initiation: UnitInterval {
                numerator: 60,
                denominator: 100,
            },
            pp_network_group: UnitInterval {
                numerator: 67,
                denominator: 100,
            },
            pp_economic_group: UnitInterval {
                numerator: 67,
                denominator: 100,
            },
            pp_technical_group: UnitInterval {
                numerator: 67,
                denominator: 100,
            },
            pp_gov_group: UnitInterval {
                numerator: 75,
                denominator: 100,
            },
            treasury_withdrawal: UnitInterval {
                numerator: 67,
                denominator: 100,
            },
        });

        // SPO pool thresholds default — TreasuryWithdrawals never requires SPO
        // votes (spo_threshold_for_action returns None), so these are irrelevant.
        pp.pool_voting_thresholds = Some(yggdrasil_ledger::PoolVotingThresholds::default());
        pp.drep_activity = Some(100);
        pp.gov_action_deposit = Some(100_000);
        pp.gov_action_lifetime = Some(20);
    }

    // Fund treasury.
    state.accounting_mut().treasury = 1_000_000;

    // --- Committee (one member, quorum 1/1) ---
    let cold_cred = StakeCredential::AddrKeyHash([0xC1; 28]);
    let hot_cred = StakeCredential::AddrKeyHash([0xC2; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    authorize_committee_via_block(&mut state, cold_cred, hot_cred, 0xE1);
    state.enact_state_mut().committee_quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    state.enact_state_mut().has_committee = true;

    // --- DRep with 100% delegated stake ---
    let drep = DRep::KeyHash([0xD1; 28]);
    state.drep_state_mut().register(
        drep,
        RegisteredDrep::new_active(500_000, None, EpochNo(100)),
    );
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xD1; 28]));
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xD1; 28]))
    {
        sc.set_delegated_drep(Some(drep));
    }
    // Seed reward balance so DRep-attributed stake appears in snapshots.
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xD1; 28]),
        },
        RewardAccountState::new(1_000_000, None),
    );

    // Reward account for the withdrawal target.
    let ra_target = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash([0xBB; 28]),
    };
    state
        .reward_accounts_mut()
        .insert(ra_target, RewardAccountState::new(0, None));

    // Reward accounts for proposal return addresses.
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA1; 28]),
        },
        RewardAccountState::new(0, None),
    );
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA2; 28]),
        },
        RewardAccountState::new(0, None),
    );

    // --- Proposal A: ParameterChange lowering treasury_withdrawal DRep
    //     threshold to 0 so all subsequent proposals pass without DRep votes ---
    let gov_id_a = GovActionId {
        transaction_id: [0xA1; 32],
        gov_action_index: 0,
    };
    let new_thresholds = yggdrasil_ledger::DRepVotingThresholds {
        // Keep all at 67% except treasury_withdrawal → 0%.
        motion_no_confidence: UnitInterval {
            numerator: 67,
            denominator: 100,
        },
        committee_normal: UnitInterval {
            numerator: 67,
            denominator: 100,
        },
        committee_no_confidence: UnitInterval {
            numerator: 60,
            denominator: 100,
        },
        update_to_constitution: UnitInterval {
            numerator: 75,
            denominator: 100,
        },
        hard_fork_initiation: UnitInterval {
            numerator: 60,
            denominator: 100,
        },
        pp_network_group: UnitInterval {
            numerator: 67,
            denominator: 100,
        },
        pp_economic_group: UnitInterval {
            numerator: 67,
            denominator: 100,
        },
        pp_technical_group: UnitInterval {
            numerator: 67,
            denominator: 100,
        },
        pp_gov_group: UnitInterval {
            numerator: 75,
            denominator: 100,
        },
        treasury_withdrawal: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
    };
    let proposal_a = ProposalProcedure {
        deposit: 100_000,
        reward_account: reward_account_bytes([0xA1; 28]),
        gov_action: GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: ProtocolParameterUpdate {
                drep_voting_thresholds: Some(new_thresholds),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.com/lower-threshold".to_string(),
            data_hash: [0xF1; 32],
        },
    };
    let mut action_a = GovernanceActionState::new(proposal_a);

    // DRep votes Yes on proposal A.
    action_a.record_vote(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    // Committee votes Yes on proposal A.
    action_a.record_vote(Voter::CommitteeKeyHash([0xC2; 28]), Vote::Yes);
    state
        .governance_actions_mut()
        .insert(gov_id_a.clone(), action_a);

    // --- Proposal B: TreasuryWithdrawals — no DRep votes ---
    let gov_id_b = GovActionId {
        transaction_id: [0xA2; 32],
        gov_action_index: 0,
    };
    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(ra_target, 100u64);
    let proposal_b = ProposalProcedure {
        deposit: 100_000,
        reward_account: reward_account_bytes([0xA2; 28]),
        gov_action: GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        anchor: Anchor {
            url: "https://example.com/treasury".to_string(),
            data_hash: [0xF2; 32],
        },
    };
    let mut action_b = GovernanceActionState::new(proposal_b);

    // Committee votes Yes on proposal B — but NO DRep votes.
    action_b.record_vote(Voter::CommitteeKeyHash([0xC2; 28]), Vote::Yes);
    state
        .governance_actions_mut()
        .insert(gov_id_b.clone(), action_b);

    assert_eq!(state.governance_actions().len(), 2);

    // Build a mark snapshot with DRep-attributed stake so thresholds
    // are not vacuously satisfied.
    let mut mark = StakeSnapshot::default();
    mark.stake
        .add(StakeCredential::AddrKeyHash([0xD1; 28]), 1_000_000);
    let mut snapshots = StakeSnapshots {
        mark,
        set: StakeSnapshot::default(),
        go: StakeSnapshot::default(),
        fee_pot: 0,
    };

    // Run epoch boundary — ratification happens inside.
    let _event =
        apply_epoch_boundary(&mut state, EpochNo(101), &mut snapshots, &BTreeMap::new()).unwrap();

    // Both proposals should have been enacted:
    // - Proposal A passed (100% DRep Yes > 67% threshold).
    // - After A enacted, treasury_withdrawal threshold → 0%.
    // - Proposal B now passes (0% meets 0% threshold).
    assert!(
        !state.governance_actions().contains_key(&gov_id_a),
        "Proposal A (ParameterChange) should have been ratified and removed",
    );
    assert!(
        !state.governance_actions().contains_key(&gov_id_b),
        "Proposal B (TreasuryWithdrawals) should have been ratified using the \
         UPDATED zero threshold enacted by Proposal A — upstream ratifyTransition \
         reads thresholds from the evolving ensCurPParams",
    );

    // Verify the ParameterChange was actually applied (threshold is now 0).
    let pp = state.protocol_params().unwrap();
    assert_eq!(
        pp.drep_voting_thresholds
            .as_ref()
            .unwrap()
            .treasury_withdrawal,
        UnitInterval {
            numerator: 0,
            denominator: 1
        },
    );

    // Verify the TreasuryWithdrawal was applied (100 lovelace credited).
    let ra = state
        .reward_accounts()
        .get(&RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xBB; 28]),
        })
        .unwrap();
    assert_eq!(ra.balance(), 100);
}
