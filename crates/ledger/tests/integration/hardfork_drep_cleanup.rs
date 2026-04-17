//! Parity tests for the Conway HARDFORK rule `updateDRepDelegations`.
//!
//! When a `HardForkInitiation` governance action is enacted transitioning the
//! protocol version to major 10 (bootstrap → post-bootstrap), the upstream
//! ledger runs `updateDRepDelegations` which removes DRep delegations from
//! accounts that point to non-existent (unregistered) DReps.
//!
//! During the bootstrap phase (PV 9), delegating to an unregistered DRep was
//! permitted (`preserveIncorrectDelegation` / `hardforkConwayBootstrapPhase`).
//! The PV 9→10 cleanup ensures that these dangling delegations do not persist
//! into the post-bootstrap era.
//!
//! Reference: `Cardano.Ledger.Conway.Rules.HardFork.updateDRepDelegations`.

use super::*;
use std::collections::BTreeMap;
use yggdrasil_ledger::{
    GovernanceActionState, StakeSnapshot, StakeSnapshots, apply_epoch_boundary,
    compute_stake_snapshot,
};

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn make_conway_block_hf(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<ConwayTxBody>) -> Block {
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
        },
        transactions: tx_list,
        raw_cbor: None,
        header_cbor_size: None,
    }
}

fn reward_account_bytes_hf(keyhash: [u8; 28]) -> Vec<u8> {
    let mut bytes = vec![0xe1]; // network 1, key-hash type
    bytes.extend_from_slice(&keyhash);
    bytes
}

/// Build a PV 9 (bootstrap) Conway state with committee + DRep governance
/// infrastructure ready for ratification.
fn bootstrap_state_with_governance() -> LedgerState {
    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.protocol_version = Some((9, 0));
    pp.key_deposit = 2_000_000;
    pp.drep_deposit = Some(500_000);
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    pp.gov_action_deposit = Some(100_000);
    pp.gov_action_lifetime = Some(20);
    pp.drep_activity = Some(100);
    pp.pool_voting_thresholds = Some(yggdrasil_ledger::PoolVotingThresholds::default());
    pp.drep_voting_thresholds = Some(yggdrasil_ledger::DRepVotingThresholds {
        motion_no_confidence: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        committee_normal: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        committee_no_confidence: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        update_to_constitution: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        hard_fork_initiation: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        pp_network_group: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        pp_economic_group: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        pp_technical_group: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        pp_gov_group: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
        treasury_withdrawal: UnitInterval {
            numerator: 51,
            denominator: 100,
        },
    });
    state.set_protocol_params(pp);
    state.set_current_epoch(EpochNo(100));
    state.accounting_mut().treasury = 1_000_000;

    // --- Committee (one member, quorum 1/1) ---
    let cold_cred = StakeCredential::AddrKeyHash([0xC1; 28]);
    state
        .committee_state_mut()
        .register_with_term(cold_cred, 200);
    state.enact_state_mut().committee_quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    state.enact_state_mut().has_committee = true;
    // Authorize hot key via a block so it goes through proper cert processing.
    {
        let auth_input = ShelleyTxIn {
            transaction_id: [0xE1; 32],
            index: 0,
        };
        state.multi_era_utxo_mut().insert_shelley(
            auth_input.clone(),
            ShelleyTxOut {
                address: vec![0x01],
                amount: 1_000_000,
            },
        );
        let auth_block = make_conway_block_hf(
            1,
            1,
            0xE1,
            vec![ConwayTxBody {
                inputs: vec![auth_input],
                outputs: vec![BabbageTxOut {
                    address: vec![0x02],
                    amount: Value::Coin(1_000_000),
                    datum_option: None,
                    script_ref: None,
                }],
                fee: 0,
                ttl: Some(100),
                certificates: Some(vec![DCert::CommitteeAuthorization(
                    StakeCredential::AddrKeyHash([0xC1; 28]),
                    StakeCredential::AddrKeyHash([0xC2; 28]),
                )]),
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
            .apply_block(&auth_block)
            .expect("committee authorization block");
    }

    // --- DRep (registered, 100% delegated stake) ---
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
        sc.set_delegated_pool(Some([0x50; 28]));
        sc.set_delegated_drep(Some(drep));
    }
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xD1; 28]),
        },
        RewardAccountState::new(1_000_000, None),
    );

    // --- SPO (one pool, with stake for hard fork SPO threshold) ---
    let pool_id = [0x50u8; 28];
    state.pool_state_mut().register(PoolParams {
        operator: pool_id,
        vrf_keyhash: [0x01; 32],
        pledge: 1_000_000,
        cost: 340_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 100,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0x50; 28]),
        },
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    });

    // Proposal return account.
    state.reward_accounts_mut().insert(
        RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA1; 28]),
        },
        RewardAccountState::new(0, None),
    );

    state
}

/// Seed a HardForkInitiation governance action (PV 9 → 10) and have
/// committee + DRep + SPO vote Yes so it passes ratification at the
/// next epoch boundary.
fn seed_hardfork_proposal(state: &mut LedgerState) -> GovActionId {
    let gov_id = GovActionId {
        transaction_id: [0xAB; 32],
        gov_action_index: 0,
    };
    let proposal = ProposalProcedure {
        deposit: 100_000,
        reward_account: reward_account_bytes_hf([0xA1; 28]),
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        anchor: Anchor {
            url: "https://example.com/hardfork-10".to_string(),
            data_hash: [0xF1; 32],
        },
    };
    let mut action = GovernanceActionState::new(proposal);
    // Committee votes Yes.
    action.record_vote(Voter::CommitteeKeyHash([0xC2; 28]), Vote::Yes);
    // DRep votes Yes.
    action.record_vote(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    // SPO votes Yes.
    action.record_vote(Voter::StakePool([0x50; 28]), Vote::Yes);
    state
        .governance_actions_mut()
        .insert(gov_id.clone(), action);
    gov_id
}

fn snapshots_with_stake(state: &LedgerState) -> StakeSnapshots {
    let mark = compute_stake_snapshot(
        state.multi_era_utxo(),
        state.stake_credentials(),
        state.reward_accounts(),
        state.pool_state(),
    );
    StakeSnapshots {
        mark,
        set: StakeSnapshot::default(),
        go: StakeSnapshot::default(),
        fee_pot: 0,
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

/// PV 9→10 transition clears delegation to a non-existent DRep.
///
/// Upstream: during bootstrap (PV 9), delegating to an unregistered DRep is
/// allowed. When the HardForkInitiation enacting PV 10 passes, the HARDFORK
/// rule's `updateDRepDelegations` removes these dangling delegations.
#[test]
fn hardfork_pv9_to_10_clears_dangling_drep_delegation() {
    let mut state = bootstrap_state_with_governance();

    // Register a stake credential that delegates to an UNREGISTERED DRep.
    // During bootstrap (PV 9), this is accepted by `preserveIncorrectDelegation`.
    let unregistered_drep = DRep::KeyHash([0xDD; 28]);
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xBB; 28]));
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xBB; 28]))
    {
        sc.set_delegated_drep(Some(unregistered_drep));
    }

    // Confirm the delegation is present before the hardfork.
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xBB; 28]))
            .and_then(|s| s.delegated_drep()),
        Some(unregistered_drep),
        "dangling delegation should exist before PV 9→10 transition",
    );

    // Seed HardForkInitiation (9→10) proposal that will pass ratification.
    let gov_id = seed_hardfork_proposal(&mut state);

    // Run epoch boundary → ratification enacts HardFork → HARDFORK rule
    // triggers updateDRepDelegations.
    let mut snapshots = snapshots_with_stake(&state);
    let _event =
        apply_epoch_boundary(&mut state, EpochNo(101), &mut snapshots, &BTreeMap::new()).unwrap();

    // Hardfork should have been enacted.
    assert!(
        !state.governance_actions().contains_key(&gov_id),
        "HardForkInitiation should have been ratified and removed",
    );
    assert_eq!(
        state.protocol_params().unwrap().protocol_version,
        Some((10, 0)),
        "protocol version should be updated to (10, 0)",
    );

    // The dangling delegation to the unregistered DRep should be cleared.
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xBB; 28]))
            .and_then(|s| s.delegated_drep()),
        None,
        "dangling DRep delegation should have been removed by \
         updateDRepDelegations at PV 9→10 transition",
    );
}

/// PV 9→10 transition preserves valid delegations and builtin DReps.
///
/// `updateDRepDelegations` should NOT clear delegations to registered DReps
/// or to builtin DReps (`AlwaysAbstain`, `AlwaysNoConfidence`).
#[test]
fn hardfork_pv9_to_10_preserves_valid_and_builtin_delegations() {
    let mut state = bootstrap_state_with_governance();

    // Delegation to a registered DRep (D1, already set up by helper).
    let registered_drep = DRep::KeyHash([0xD1; 28]);

    // Delegation to AlwaysAbstain (builtin).
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xB1; 28]));
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xB1; 28]))
    {
        sc.set_delegated_drep(Some(DRep::AlwaysAbstain));
    }

    // Delegation to AlwaysNoConfidence (builtin).
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xB2; 28]));
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xB2; 28]))
    {
        sc.set_delegated_drep(Some(DRep::AlwaysNoConfidence));
    }

    // Seed HardForkInitiation and run epoch boundary.
    let _gov_id = seed_hardfork_proposal(&mut state);
    let mut snapshots = snapshots_with_stake(&state);
    let _event =
        apply_epoch_boundary(&mut state, EpochNo(101), &mut snapshots, &BTreeMap::new()).unwrap();

    // Registered DRep delegation preserved.
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xD1; 28]))
            .and_then(|s| s.delegated_drep()),
        Some(registered_drep),
        "delegation to registered DRep should be preserved after PV 9→10 transition",
    );

    // AlwaysAbstain delegation preserved.
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xB1; 28]))
            .and_then(|s| s.delegated_drep()),
        Some(DRep::AlwaysAbstain),
        "AlwaysAbstain delegation should be preserved (builtin DRep)",
    );

    // AlwaysNoConfidence delegation preserved.
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xB2; 28]))
            .and_then(|s| s.delegated_drep()),
        Some(DRep::AlwaysNoConfidence),
        "AlwaysNoConfidence delegation should be preserved (builtin DRep)",
    );
}

/// Non-hardfork epoch does NOT trigger the DRep cleanup.
///
/// When no HardForkInitiation is enacted (protocol version stays the same),
/// the `updateDRepDelegations` cleanup should not be triggered — dangling
/// delegations remain as-is.
#[test]
fn non_hardfork_epoch_does_not_trigger_drep_cleanup() {
    let mut state = bootstrap_state_with_governance();

    // Register a dangling delegation.
    let unregistered_drep = DRep::KeyHash([0xDD; 28]);
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xBB; 28]));
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xBB; 28]))
    {
        sc.set_delegated_drep(Some(unregistered_drep));
    }

    // Do NOT seed any HardForkInitiation proposal.

    // Run epoch boundary → no hardfork, no cleanup.
    let mut snapshots = snapshots_with_stake(&state);
    let _event =
        apply_epoch_boundary(&mut state, EpochNo(101), &mut snapshots, &BTreeMap::new()).unwrap();

    // Protocol version unchanged.
    assert_eq!(
        state.protocol_params().unwrap().protocol_version,
        Some((9, 0)),
    );

    // Dangling delegation still present.
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xBB; 28]))
            .and_then(|s| s.delegated_drep()),
        Some(unregistered_drep),
        "dangling delegation should remain when no hardfork is enacted",
    );
}

/// PV 10→11 transition does NOT trigger the DRep cleanup.
///
/// The `updateDRepDelegations` cleanup is only for PV major == 10 transition.
/// A transition to PV 11 runs `populateVRFKeyHashes` instead (which we
/// implement via linear VRF scan, so no state effect).
#[test]
fn hardfork_pv10_to_11_does_not_trigger_drep_cleanup() {
    let mut state = bootstrap_state_with_governance();

    // Start at PV 10 (post-bootstrap).
    state
        .protocol_params_mut()
        .as_mut()
        .unwrap()
        .protocol_version = Some((10, 0));

    // Register a dangling delegation (unlikely in practice after PV 10,
    // but test the gate logic).
    let unregistered_drep = DRep::KeyHash([0xDD; 28]);
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xBB; 28]));
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xBB; 28]))
    {
        sc.set_delegated_drep(Some(unregistered_drep));
    }

    // Seed HardForkInitiation to PV 11.
    let gov_id = GovActionId {
        transaction_id: [0xAC; 32],
        gov_action_index: 0,
    };
    let proposal = ProposalProcedure {
        deposit: 100_000,
        reward_account: reward_account_bytes_hf([0xA1; 28]),
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (11, 0),
        },
        anchor: Anchor {
            url: "https://example.com/hardfork-11".to_string(),
            data_hash: [0xF2; 32],
        },
    };
    let mut action = GovernanceActionState::new(proposal);
    action.record_vote(Voter::CommitteeKeyHash([0xC2; 28]), Vote::Yes);
    action.record_vote(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    action.record_vote(Voter::StakePool([0x50; 28]), Vote::Yes);
    state
        .governance_actions_mut()
        .insert(gov_id.clone(), action);

    let mut snapshots = snapshots_with_stake(&state);
    let _event =
        apply_epoch_boundary(&mut state, EpochNo(101), &mut snapshots, &BTreeMap::new()).unwrap();

    // Hardfork should be enacted.
    assert_eq!(
        state.protocol_params().unwrap().protocol_version,
        Some((11, 0)),
    );

    // Dangling delegation should still be present (PV 11 cleanup is
    // populateVRFKeyHashes, not updateDRepDelegations).
    assert_eq!(
        state
            .stake_credentials()
            .get(&StakeCredential::AddrKeyHash([0xBB; 28]))
            .and_then(|s| s.delegated_drep()),
        Some(unregistered_drep),
        "dangling delegation should persist at PV 10→11 transition \
         (updateDRepDelegations only runs at PV 9→10)",
    );
}
