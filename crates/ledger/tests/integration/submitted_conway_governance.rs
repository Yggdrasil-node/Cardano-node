//! Integration tests for Conway submitted-transaction governance validation.
//!
//! Verifies that `apply_submitted_tx` for Conway-era transactions enforces
//! the same governance rules as the block-application path:
//! - `validate_conway_voters()` — reject unknown voters
//! - `validate_conway_proposals()` — validate proposal format/lineage
//! - `validate_conway_vote_targets()` — reject votes on non-existent proposals
//! - `validate_conway_voter_permissions()` — enforce voter authority
//! - `validate_conway_current_treasury_value()` — reject wrong treasury declaration
//! - `apply_conway_votes()` — accumulate votes into GovernanceState
//! - `touch_drep_activity_for_certs()` — DRep activity tracking
//!
//! Reference: `Cardano.Ledger.Conway.Rules.GOV`, `GOVVOTES`, `RATIFY`.

use super::*;

fn enterprise_addr(network: u8, keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60 | (network & 0x0f)];
    addr.extend_from_slice(keyhash);
    addr
}

fn witness_set_for(signers: &[&TestSigner], body: &ConwayTxBody) -> ShelleyWitnessSet {
    let tx_body_hash = compute_tx_id(&body.to_cbor_bytes()).0;
    ShelleyWitnessSet {
        vkey_witnesses: signers.iter().map(|s| s.witness(&tx_body_hash)).collect(),
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
    params.gov_action_deposit = Some(0);
    params.gov_action_lifetime = Some(100);
    params
}

fn minimal_conway_body(inputs: Vec<ShelleyTxIn>, outputs: Vec<BabbageTxOut>, fee: u64) -> ConwayTxBody {
    ConwayTxBody {
        inputs,
        outputs,
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
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    }
}

fn conway_state_with_input(signer: &TestSigner, tx_id_bytes: [u8; 32], amount: u64) -> LedgerState {
    let addr = signer.enterprise_addr();
    let mut state = LedgerState::new(Era::Conway);
    state.set_protocol_params(permissive_params());
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: tx_id_bytes, index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr,
            amount: Value::Coin(amount),
            datum_option: None,
            script_ref: None,
        }),
    );
    state
}

// ===========================================================================
// Voter existence validation
// ===========================================================================

#[test]
fn conway_submitted_tx_rejects_unknown_voter() {
    let signer = TestSigner::new([0x01; 32]);
    let voter_signer = TestSigner::new([0xDD; 32]);
    let mut state = conway_state_with_input(&signer, [0x01; 32], 5_000_000);

    let unknown_voter = Voter::DRepKeyHash(voter_signer.vkey_hash);
    let fake_action_id = GovActionId {
        transaction_id: [0xFF; 32],
        gov_action_index: 0,
    };

    // Stage a governance action so the target exists.
    state.governance_actions_mut().insert(
        fake_action_id.clone(),
        yggdrasil_ledger::GovernanceActionState::new(ProposalProcedure {
            deposit: 0,
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([0x42; 28]),
            }.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: "https://example.invalid/test".to_string(),
                data_hash: [0xAA; 32],
            },
        }),
    );

    let mut vote_map = std::collections::BTreeMap::new();
    vote_map.insert(
        fake_action_id,
        VotingProcedure { vote: Vote::Yes, anchor: None },
    );

    let mut body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    body.voting_procedures = Some(VotingProcedures {
        procedures: [(unknown_voter.clone(), vote_map)].into_iter().collect(),
    });

    let ws = witness_set_for(&[&signer, &voter_signer], &body);
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::VotersDoNotExist(ref voters)) if voters.contains(&unknown_voter)),
        "expected VotersDoNotExist, got: {:?}",
        result,
    );
}

// ===========================================================================
// Vote target validation
// ===========================================================================

#[test]
fn conway_submitted_tx_rejects_vote_on_nonexistent_proposal() {
    let signer = TestSigner::new([0x01; 32]);
    let voter_signer = TestSigner::new([0xDD; 32]);
    let drep_key = voter_signer.vkey_hash;
    let mut state = conway_state_with_input(&signer, [0x01; 32], 5_000_000);
    // Register DRep so voter exists, but target action does NOT exist.
    state.drep_state_mut().register(DRep::KeyHash(drep_key), RegisteredDrep::new(0, None));

    let nonexistent_action = GovActionId {
        transaction_id: [0xFF; 32],
        gov_action_index: 99,
    };

    let mut vote_map = std::collections::BTreeMap::new();
    vote_map.insert(
        nonexistent_action,
        VotingProcedure { vote: Vote::No, anchor: None },
    );

    let mut body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    body.voting_procedures = Some(VotingProcedures {
        procedures: [(Voter::DRepKeyHash(drep_key), vote_map)].into_iter().collect(),
    });

    let ws = witness_set_for(&[&signer, &voter_signer], &body);
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::GovActionsDoNotExist(_))),
        "expected GovActionsDoNotExist, got: {:?}",
        result,
    );
}

// ===========================================================================
// Treasury value validation
// ===========================================================================

#[test]
fn conway_submitted_tx_rejects_wrong_treasury_value() {
    let signer = TestSigner::new([0x01; 32]);
    let mut state = conway_state_with_input(&signer, [0x01; 32], 5_000_000);
    // Set actual treasury to 1_000_000.
    state.accounting_mut().treasury = 1_000_000;

    let mut body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    // Declare a treasury value that doesn't match the actual treasury.
    body.current_treasury_value = Some(999_999);

    let ws = witness_set_for(&[&signer], &body);
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(
        matches!(result, Err(LedgerError::CurrentTreasuryValueIncorrect { .. })),
        "expected CurrentTreasuryValueIncorrect, got: {:?}",
        result,
    );
}

#[test]
fn conway_submitted_tx_accepts_correct_treasury_value() {
    let signer = TestSigner::new([0x01; 32]);
    let mut state = conway_state_with_input(&signer, [0x01; 32], 5_000_000);
    state.accounting_mut().treasury = 1_000_000;

    let mut body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    body.current_treasury_value = Some(1_000_000);

    let ws = witness_set_for(&[&signer], &body);
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));

    let result = state.apply_submitted_tx(&submitted, SlotNo(10));
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// ===========================================================================
// Governance state mutation (vote accumulation + proposal staging)
// ===========================================================================

#[test]
fn conway_submitted_tx_accumulates_votes_into_governance_state() {
    let signer = TestSigner::new([0x01; 32]);
    let voter_signer = TestSigner::new([0xDD; 32]);
    let drep_key = voter_signer.vkey_hash;
    let reward_account = RewardAccount {
        network: 1,
        credential: StakeCredential::AddrKeyHash([0x42; 28]),
    };

    let mut state = conway_state_with_input(&signer, [0x01; 32], 10_000_000);
    // Also add a second input for the vote transaction.
    state.multi_era_utxo_mut().insert(
        ShelleyTxIn { transaction_id: [0x02; 32], index: 0 },
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );
    state.stake_credentials_mut().register(reward_account.credential);
    state.drep_state_mut().register(DRep::KeyHash(drep_key), RegisteredDrep::new(0, None));

    // Step 1: Submit a proposal via apply_submitted_tx.
    let mut proposal_body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    proposal_body.proposal_procedures = Some(vec![ProposalProcedure {
        deposit: 0,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::InfoAction,
        anchor: Anchor {
            url: "https://example.invalid/submitted-proposal".to_string(),
            data_hash: [0xBB; 32],
        },
    }]);

    let ws = witness_set_for(&[&signer], &proposal_body);
    let proposal_submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        proposal_body, ws, true, None,
    ));
    let proposal_tx_id = proposal_submitted.tx_id();
    state.apply_submitted_tx(&proposal_submitted, SlotNo(100))
        .expect("proposal should be accepted");

    let gov_action_id = GovActionId {
        transaction_id: proposal_tx_id.0,
        gov_action_index: 0,
    };
    assert!(
        state.governance_action(&gov_action_id).is_some(),
        "proposal should be stored in governance state after submitted-tx"
    );

    // Step 2: Submit a vote on that proposal.
    let mut vote_map = std::collections::BTreeMap::new();
    vote_map.insert(
        gov_action_id.clone(),
        VotingProcedure { vote: Vote::Yes, anchor: None },
    );

    let mut vote_body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x02; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(10_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    vote_body.voting_procedures = Some(VotingProcedures {
        procedures: [(Voter::DRepKeyHash(drep_key), vote_map)].into_iter().collect(),
    });

    let ws = witness_set_for(&[&signer, &voter_signer], &vote_body);
    let vote_submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        vote_body, ws, true, None,
    ));
    state.apply_submitted_tx(&vote_submitted, SlotNo(101))
        .expect("vote should be accepted");

    // Verify the vote was recorded.
    let stored = state.governance_action(&gov_action_id)
        .expect("governance action should still exist");
    assert_eq!(
        stored.votes().get(&Voter::DRepKeyHash(drep_key)),
        Some(&Vote::Yes),
        "DRep vote should be recorded via submitted-tx path"
    );
}

// ===========================================================================
// DRep activity tracking
// ===========================================================================

#[test]
fn conway_submitted_tx_tracks_drep_activity_for_registration() {
    let signer = TestSigner::new([0x01; 32]);
    let drep_signer = TestSigner::new([0xDD; 32]);
    let drep_key = drep_signer.vkey_hash;

    let mut state = conway_state_with_input(&signer, [0x01; 32], 5_000_000);
    state.set_current_epoch(EpochNo(42));

    let mut body = minimal_conway_body(
        vec![ShelleyTxIn { transaction_id: [0x01; 32], index: 0 }],
        vec![BabbageTxOut {
            address: signer.enterprise_addr(),
            amount: Value::Coin(5_000_000),
            datum_option: None,
            script_ref: None,
        }],
        0,
    );
    body.certificates = Some(vec![
        DCert::DrepRegistration(StakeCredential::AddrKeyHash(drep_key), 0, None),
    ]);

    let ws = witness_set_for(&[&signer, &drep_signer], &body);
    let submitted = MultiEraSubmittedTx::Conway(AlonzoCompatibleSubmittedTx::new(
        body, ws, true, None,
    ));
    state.apply_submitted_tx(&submitted, SlotNo(10))
        .expect("DRep registration should be accepted");

    // Verify the DRep is registered and activity was touched.
    let drep = state.drep_state().get(&DRep::KeyHash(drep_key));
    assert!(drep.is_some(), "DRep should be registered via submitted-tx");
    assert_eq!(
        drep.unwrap().last_active_epoch(),
        Some(EpochNo(42)),
        "DRep activity epoch should match current epoch"
    );
}
