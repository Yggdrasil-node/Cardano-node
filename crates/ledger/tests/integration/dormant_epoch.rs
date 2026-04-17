//! Dormant epoch tracking tests.
//!
//! Upstream references:
//! - `Cardano.Ledger.Conway.Rules.Epoch` — `updateNumDormantEpochs`
//! - `Cardano.Ledger.Conway.Rules.Certs` — `updateDormantDRepExpiries`,
//!   `updateDormantDRepExpiry`, `updateVotingDRepExpiries`
//! - `Cardano.Ledger.Conway.Rules.GovCert` — `computeDRepExpiry`,
//!   `computeDRepExpiryVersioned`

use super::*;
use std::collections::BTreeMap;
use yggdrasil_ledger::{
    GovernanceActionState, StakeSnapshot, StakeSnapshots, apply_epoch_boundary,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_snapshots() -> StakeSnapshots {
    StakeSnapshots {
        mark: StakeSnapshot::default(),
        set: StakeSnapshot::default(),
        go: StakeSnapshot::default(),
        fee_pot: 0,
    }
}

/// Conway LedgerState with governance and DRep parameters configured.
fn conway_state_with_governance(drep_activity: u64) -> LedgerState {
    let mut state = LedgerState::new(Era::Conway);
    let mut pp = ProtocolParameters::default();
    pp.min_fee_a = 0;
    pp.min_fee_b = 0;
    pp.min_utxo_value = None;
    pp.coins_per_utxo_byte = None;
    pp.drep_activity = Some(drep_activity);
    pp.drep_deposit = Some(500_000);
    pp.gov_action_deposit = Some(100_000);
    pp.gov_action_lifetime = Some(10);
    pp.key_deposit = 2_000_000;
    pp.pool_deposit = 500_000_000;
    state.set_protocol_params(pp);
    state
}

/// Enterprise address bytes for a key hash (type 6, network 0).
fn enterprise_addr_bytes(keyhash: &[u8; 28]) -> Vec<u8> {
    let mut addr = vec![0x60]; // type 6, network 0
    addr.extend_from_slice(keyhash);
    addr
}

/// Reward account bytes for a key hash (type 0xE0, network 0).
fn reward_account_bytes(keyhash: &[u8; 28]) -> Vec<u8> {
    let mut ra = vec![0xE0]; // reward address, network 0
    ra.extend_from_slice(keyhash);
    ra
}

fn minimal_conway_body(
    inputs: Vec<ShelleyTxIn>,
    outputs: Vec<BabbageTxOut>,
    fee: u64,
) -> ConwayTxBody {
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

fn make_block(slot: u64, block_no: u64, hash_seed: u8, txs: Vec<ConwayTxBody>) -> Block {
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

fn simple_proposal() -> ProposalProcedure {
    ProposalProcedure {
        deposit: 100_000,
        reward_account: reward_account_bytes(&[0xBB; 28]),
        gov_action: GovAction::InfoAction,
        anchor: Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0xCC; 32],
        },
    }
}

fn reward_account_from_keyhash(keyhash: &[u8; 28]) -> RewardAccount {
    RewardAccount {
        network: 0,
        credential: StakeCredential::AddrKeyHash(*keyhash),
    }
}

// ---------------------------------------------------------------------------
// CBOR round-trip
// ---------------------------------------------------------------------------

#[test]
fn num_dormant_epochs_cbor_round_trip() {
    let state = LedgerState::new(Era::Conway);
    assert_eq!(state.num_dormant_epochs(), 0);

    let encoded = state.to_cbor_bytes();
    let decoded = LedgerState::from_cbor_bytes(&encoded).unwrap();
    assert_eq!(decoded.num_dormant_epochs(), 0);
}

// ---------------------------------------------------------------------------
// Epoch boundary: dormant counter increment and reset
// ---------------------------------------------------------------------------

#[test]
fn epoch_boundary_increments_dormant_when_no_proposals() {
    let mut state = conway_state_with_governance(20);
    state.set_current_epoch(EpochNo(100));
    assert!(state.governance_actions().is_empty());
    assert_eq!(state.num_dormant_epochs(), 0);

    let event = apply_epoch_boundary(
        &mut state,
        EpochNo(101),
        &mut empty_snapshots(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(event.new_epoch, EpochNo(101));
    assert_eq!(state.num_dormant_epochs(), 1);

    let _event2 = apply_epoch_boundary(
        &mut state,
        EpochNo(102),
        &mut empty_snapshots(),
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(state.num_dormant_epochs(), 2);
}

#[test]
fn epoch_boundary_resets_dormant_when_proposals_exist() {
    let mut state = conway_state_with_governance(20);
    state.set_current_epoch(EpochNo(100));

    // Set non-trivial DRep voting thresholds so proposals need actual votes.
    if let Some(pp) = state.protocol_params_mut() {
        pp.drep_voting_thresholds = Some(yggdrasil_ledger::DRepVotingThresholds {
            motion_no_confidence: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            committee_normal: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            committee_no_confidence: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            update_to_constitution: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            hard_fork_initiation: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            pp_network_group: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            pp_economic_group: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            pp_technical_group: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            pp_gov_group: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
            treasury_withdrawal: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        });
    }

    // Register a DRep with delegated stake so thresholds aren't vacuously met.
    let drep = DRep::KeyHash([0xD2; 28]);
    state.drep_state_mut().register(
        drep,
        RegisteredDrep::new_active(500_000, None, EpochNo(100)),
    );
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xD2; 28]));
    // Delegate the stake credential to this DRep.
    if let Some(sc) = state
        .stake_credentials_mut()
        .get_mut(&StakeCredential::AddrKeyHash([0xD2; 28]))
    {
        sc.set_delegated_drep(Some(drep));
    }
    // Seed a reward account with balance under this credential so that
    // `compute_stake_snapshot` (called during snapshot rotation) picks
    // up real DRep-attributed stake in each epoch's new mark snapshot.
    state.reward_accounts_mut().insert(
        reward_account_from_keyhash(&[0xD2; 28]),
        RewardAccountState::new(1_000_000, None),
    );

    // Build snapshots — initial mark will be rotated by epoch boundary.
    let mut snaps = empty_snapshots();

    // Accumulate 2 dormant epochs.
    let _e1 = apply_epoch_boundary(&mut state, EpochNo(101), &mut snaps, &BTreeMap::new()).unwrap();
    let _e2 = apply_epoch_boundary(&mut state, EpochNo(102), &mut snaps, &BTreeMap::new()).unwrap();
    assert_eq!(state.num_dormant_epochs(), 2);

    // Insert a HardForkInitiation (requires DRep votes to pass, won't pass without them).
    let gov_id = GovActionId {
        transaction_id: [0x01; 32],
        gov_action_index: 0,
    };
    state.reward_accounts_mut().insert(
        reward_account_from_keyhash(&[0xBB; 28]),
        RewardAccountState::new(0, None),
    );
    let hf_proposal = ProposalProcedure {
        deposit: 100_000,
        reward_account: reward_account_bytes(&[0xBB; 28]),
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (99, 0),
        },
        anchor: Anchor {
            url: "https://example.com".to_string(),
            data_hash: [0xCC; 32],
        },
    };
    state
        .governance_actions_mut()
        .insert(gov_id, GovernanceActionState::new(hf_proposal));
    assert!(!state.governance_actions().is_empty());

    // Epoch boundary with active proposal that can't pass → upstream
    // `updateNumDormantEpochs` leaves the counter UNCHANGED (never resets
    // to 0 at epoch boundary). The per-tx `updateDormantDRepExpiries` is
    // responsible for clearing dormant when proposals first appear.
    let _e3 = apply_epoch_boundary(&mut state, EpochNo(103), &mut snaps, &BTreeMap::new()).unwrap();
    // Proposal should still exist (not ratified, not expired).
    assert!(!state.governance_actions().is_empty());
    assert_eq!(
        state.num_dormant_epochs(),
        2,
        "dormant counter must stay unchanged at epoch boundary when proposals exist"
    );
}

// ---------------------------------------------------------------------------
// Per-tx: dormant DRep expiry bump when proposals appear
// ---------------------------------------------------------------------------

#[test]
fn tx_with_proposals_bumps_drep_expiry_and_resets_dormant() {
    let mut state = conway_state_with_governance(20);
    state.set_current_epoch(EpochNo(50));

    // Register a DRep with last_active_epoch = 30.
    let drep = DRep::KeyHash([0xD1; 28]);
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new_active(500_000, None, EpochNo(30)));

    // Register the proposal reward account and its stake credential.
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xBB; 28]));
    state.reward_accounts_mut().insert(
        reward_account_from_keyhash(&[0xBB; 28]),
        RewardAccountState::new(0, None),
    );

    // Accumulate 5 dormant epochs.
    for epoch in 51..56 {
        let _ = apply_epoch_boundary(
            &mut state,
            EpochNo(epoch),
            &mut empty_snapshots(),
            &BTreeMap::new(),
        )
        .unwrap();
    }
    assert_eq!(state.num_dormant_epochs(), 5);
    assert_eq!(
        state.drep_state().get(&drep).unwrap().last_active_epoch(),
        Some(EpochNo(30)),
    );

    // Seed a UTxO.
    let addr = enterprise_addr_bytes(&[0xAA; 28]);
    let input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(200_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    // Build a tx with a proposal.
    let mut body = minimal_conway_body(
        vec![input],
        vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(199_700_000),
            datum_option: None,
            script_ref: None,
        }],
        200_000,
    );
    body.proposal_procedures = Some(vec![simple_proposal()]);

    let block = make_block(100, 100, 0x01, vec![body]);
    state.apply_block(&block).unwrap();

    // Dormant counter should be reset to 0.
    assert_eq!(state.num_dormant_epochs(), 0);

    // DRep's last_active should be bumped by 5 (from 30 to 35).
    // Guard: old_expiry = 30+20 = 50, new_expiry = 50+5 = 55 >= 55 (current) ✓
    assert_eq!(
        state.drep_state().get(&drep).unwrap().last_active_epoch(),
        Some(EpochNo(35)),
    );
}

#[test]
fn tx_without_proposals_preserves_dormant_counter() {
    let mut state = conway_state_with_governance(20);
    state.set_current_epoch(EpochNo(50));

    // Accumulate 3 dormant epochs.
    for epoch in 51..54 {
        let _ = apply_epoch_boundary(
            &mut state,
            EpochNo(epoch),
            &mut empty_snapshots(),
            &BTreeMap::new(),
        )
        .unwrap();
    }
    assert_eq!(state.num_dormant_epochs(), 3);

    // Seed a UTxO and apply a tx with no proposals.
    let addr = enterprise_addr_bytes(&[0xAA; 28]);
    let input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(200_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let body = minimal_conway_body(
        vec![input],
        vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(199_800_000),
            datum_option: None,
            script_ref: None,
        }],
        200_000,
    );

    let block = make_block(100, 100, 0x01, vec![body]);
    state.apply_block(&block).unwrap();

    // Dormant counter unchanged.
    assert_eq!(state.num_dormant_epochs(), 3);
}

#[test]
fn dormant_bump_skips_fully_expired_dreps() {
    // A DRep whose bumped expiry would still be before current_epoch is
    // skipped — the DRep has already lapsed beyond recovery.
    let mut state = conway_state_with_governance(10); // drep_activity = 10
    state.set_current_epoch(EpochNo(100));

    // Register a DRep that was last active at epoch 5 (very old).
    let drep = DRep::KeyHash([0xD1; 28]);
    state
        .drep_state_mut()
        .register(drep, RegisteredDrep::new_active(500_000, None, EpochNo(5)));

    // Register proposal reward account and its stake credential.
    state
        .stake_credentials_mut()
        .register(StakeCredential::AddrKeyHash([0xBB; 28]));
    state.reward_accounts_mut().insert(
        reward_account_from_keyhash(&[0xBB; 28]),
        RewardAccountState::new(0, None),
    );

    // Accumulate 3 dormant epochs.
    for epoch in 101..104 {
        let _ = apply_epoch_boundary(
            &mut state,
            EpochNo(epoch),
            &mut empty_snapshots(),
            &BTreeMap::new(),
        )
        .unwrap();
    }
    assert_eq!(state.num_dormant_epochs(), 3);
    // old_expiry = 5+10 = 15, new_expiry = 15+3 = 18 < 103 → skip.

    // Seed UTxO and submit tx with proposal.
    let addr = enterprise_addr_bytes(&[0xAA; 28]);
    let input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    state.multi_era_utxo_mut().insert(
        input.clone(),
        MultiEraTxOut::Babbage(BabbageTxOut {
            address: addr.clone(),
            amount: Value::Coin(200_000_000),
            datum_option: None,
            script_ref: None,
        }),
    );

    let mut body = minimal_conway_body(
        vec![input],
        vec![BabbageTxOut {
            address: addr,
            amount: Value::Coin(199_700_000),
            datum_option: None,
            script_ref: None,
        }],
        200_000,
    );
    body.proposal_procedures = Some(vec![simple_proposal()]);

    let block = make_block(200, 200, 0x01, vec![body]);
    state.apply_block(&block).unwrap();

    // Dormant counter reset to 0 regardless.
    assert_eq!(state.num_dormant_epochs(), 0);

    // But the DRep's last_active_epoch was NOT bumped (stayed at 5).
    assert_eq!(
        state.drep_state().get(&drep).unwrap().last_active_epoch(),
        Some(EpochNo(5)),
    );
}
