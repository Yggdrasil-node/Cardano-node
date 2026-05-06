// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::*;
use crate::eras::conway::{GovAction, Vote, Voter};
use crate::eras::shelley::ShelleyTxOut;
use crate::protocol_params::{PoolVotingThresholds, ProtocolParameterUpdate, ProtocolParameters};
use crate::types::{BlockNo, HeaderHash, PoolParams, Relay, RewardAccount, SlotNo, UnitInterval};

fn sample_pool_params(relays: Vec<Relay>, operator: u8) -> PoolParams {
    PoolParams {
        operator: [operator; 28],
        vrf_keyhash: [operator; 32],
        pledge: 1,
        cost: 1,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([operator; 28]),
        },
        pool_owners: vec![[operator; 28]],
        relays,
        pool_metadata: None,
    }
}

fn empty_test_block(era: Era, slot: u64, block_no: u64, issuer: u8) -> crate::tx::Block {
    crate::tx::Block {
        era,
        header: crate::tx::BlockHeader {
            hash: HeaderHash([slot as u8; 32]),
            prev_hash: HeaderHash([block_no.saturating_sub(1) as u8; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(block_no),
            issuer_vkey: [issuer; 32],
            protocol_version: None,
        },
        transactions: Vec::new(),
        raw_cbor: None,
        header_cbor_size: None,
    }
}

#[test]
fn registered_pool_relay_access_points_skip_non_dialable_relays() {
    let pool = RegisteredPool {
        params: sample_pool_params(
            vec![
                Relay::SingleHostAddr(Some(3001), Some([127, 0, 0, 1]), None),
                Relay::SingleHostName(Some(3002), "relay.example".to_owned()),
                Relay::SingleHostName(None, "missing-port.example".to_owned()),
                Relay::MultiHostName("srv.example".to_owned()),
            ],
            1,
        ),
        retiring_epoch: None,
        deposit: 0,
    };

    assert_eq!(
        pool.relay_access_points(),
        vec![
            PoolRelayAccessPoint {
                address: "127.0.0.1".to_owned(),
                port: 3001,
            },
            PoolRelayAccessPoint {
                address: "relay.example".to_owned(),
                port: 3002,
            },
        ]
    );
}

#[test]
fn pool_state_relay_access_points_deduplicate_across_pools() {
    let mut pool_state = PoolState::new();
    pool_state.register(sample_pool_params(
        vec![Relay::SingleHostName(
            Some(3001),
            "shared.example".to_owned(),
        )],
        1,
    ));
    pool_state.register(sample_pool_params(
        vec![
            Relay::SingleHostName(Some(3001), "shared.example".to_owned()),
            Relay::SingleHostAddr(Some(3002), Some([127, 0, 0, 2]), None),
        ],
        2,
    ));

    assert_eq!(
        pool_state.relay_access_points(),
        vec![
            PoolRelayAccessPoint {
                address: "shared.example".to_owned(),
                port: 3001,
            },
            PoolRelayAccessPoint {
                address: "127.0.0.2".to_owned(),
                port: 3002,
            },
        ]
    );
}

#[test]
fn overlay_slot_detection_matches_upstream_step_function() {
    let first_slot = 100;
    let d = UnitInterval {
        numerator: 1,
        denominator: 2,
    };

    assert!(is_overlay_slot_for_blocks_made(first_slot, d, 100));
    assert!(!is_overlay_slot_for_blocks_made(first_slot, d, 101));
    assert!(is_overlay_slot_for_blocks_made(first_slot, d, 102));
    assert!(!is_overlay_slot_for_blocks_made(first_slot, d, 103));
    assert!(!is_overlay_slot_for_blocks_made(
        first_slot,
        UnitInterval {
            numerator: 0,
            denominator: 1
        },
        100
    ));
}

#[test]
fn apply_block_validated_skips_overlay_slots_for_blocks_made() {
    let mut params = ProtocolParameters::default();
    params.d = Some(UnitInterval {
        numerator: 1,
        denominator: 2,
    });

    let mut state = LedgerState::new(Era::Shelley);
    state.set_current_epoch(EpochNo(1));
    state.set_slots_per_epoch(100);
    state.set_protocol_params(params);

    let overlay = empty_test_block(Era::Shelley, 100, 1, 7);
    state.apply_block_validated(&overlay, None).unwrap();
    assert!(
        state.blocks_made().is_empty(),
        "overlay slots must not increment nesBcur"
    );

    let regular = empty_test_block(Era::Shelley, 101, 2, 7);
    state.apply_block_validated(&regular, None).unwrap();
    let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&[7; 32]).0;
    assert_eq!(state.blocks_made().get(&pool_hash), Some(&1));
}

#[test]
fn apply_due_pending_pparam_updates_catches_up_stale_epoch() {
    let mut state = LedgerState::new(Era::Babbage);
    state.protocol_params = Some(ProtocolParameters::alonzo_defaults());
    state.gen_delegs.insert(
        [0x01; 28],
        GenesisDelegationState {
            delegate: [0x02; 28],
            vrf: [0x03; 32],
        },
    );

    let mut cost_models = BTreeMap::new();
    cost_models.insert(0, vec![10, 20, 30]);
    let update = ProtocolParameterUpdate {
        min_fee_a: Some(99),
        cost_models: Some(cost_models.clone()),
        ..Default::default()
    };
    let field_count = update.field_count();
    state
        .pending_pparam_updates
        .entry(EpochNo(8))
        .or_default()
        .insert([0x01; 28], update);

    let applied = state.apply_due_pending_pparam_updates(EpochNo(9));

    assert_eq!(applied, field_count);
    let params = state.protocol_params().expect("protocol params");
    assert_eq!(params.min_fee_a, 99);
    assert_eq!(params.cost_models, Some(cost_models));
    assert!(state.pending_pparam_updates().is_empty());
}

#[test]
fn ledger_state_checkpoint_round_trips_governance_actions() {
    let reward_account = RewardAccount {
        network: 0,
        credential: crate::StakeCredential::AddrKeyHash([0x22; 28]),
    };
    let gov_action_id = crate::eras::conway::GovActionId {
        transaction_id: [0x11; 32],
        gov_action_index: 0,
    };
    let proposal = crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: reward_account.to_bytes().to_vec(),
        gov_action: GovAction::InfoAction,
        anchor: crate::Anchor {
            url: "https://example.invalid/proposal".to_owned(),
            data_hash: [0x33; 32],
        },
    };

    let mut state = LedgerState::new(Era::Conway);
    state.governance_actions.insert(
        gov_action_id.clone(),
        GovernanceActionState::new(proposal.clone()),
    );

    let checkpoint = state.checkpoint();
    let restored = checkpoint.restore();
    assert_eq!(
        restored
            .governance_action(&gov_action_id)
            .unwrap()
            .proposal(),
        &proposal
    );

    let round_trip = LedgerStateCheckpoint::from_cbor_bytes(&checkpoint.to_cbor_bytes())
        .expect("checkpoint round-trip");
    assert_eq!(round_trip.restore(), state);
}

// -- RegisteredDrep activity tracking ---------------------------------

#[test]
fn effective_gen_delegs_falls_back_to_pending_before_activation() {
    // Mirrors the preview/preprod cold-sync entry path: at chain birth
    // `LedgerState::new(Era::Byron)` leaves `gen_delegs` empty and the
    // genesis delegate map sits in `pending_shelley_genesis_delegs`
    // until the first Shelley-family block triggers
    // `maybe_activate_pending_shelley_genesis`.  The TPraos overlay
    // schedule lookup must observe the entries immediately; otherwise
    // slot 0 is rejected as `TpraosOverlaySlotNotActive`.
    //
    // Reference: `Cardano.Ledger.Shelley.Genesis.initialState`
    // populates `_dsGenDelegs` from `sgGenDelegs` at chain birth.
    let mut state = LedgerState::new(Era::Byron);
    assert!(state.gen_delegs().is_empty());
    assert!(state.effective_gen_delegs().is_empty());

    let mut pending: BTreeMap<GenesisHash, GenesisDelegationState> = BTreeMap::new();
    pending.insert(
        [0x10; 28],
        GenesisDelegationState {
            delegate: [0xA0; 28],
            vrf: [0xB0; 32],
        },
    );
    state.configure_pending_shelley_genesis_delegs(pending.clone());

    // `gen_delegs()` still reports empty (no activation has occurred).
    assert!(state.gen_delegs().is_empty());
    // `effective_gen_delegs()` exposes the pending map so VRF/overlay
    // checks for the very first Shelley-family block work correctly.
    let effective = state.effective_gen_delegs();
    assert_eq!(effective.len(), 1);
    assert_eq!(effective.get(&[0x10; 28]), pending.get(&[0x10; 28]));

    // After activation `effective_gen_delegs` continues to mirror the
    // active map (matches upstream behaviour where `dsGenDelegs` is
    // the single source of truth post-genesis).
    state.gen_delegs_mut().extend(pending.iter().map(|(k, v)| {
        (
            *k,
            GenesisDelegationState {
                delegate: v.delegate,
                vrf: v.vrf,
            },
        )
    }));
    assert_eq!(state.effective_gen_delegs().len(), 1);
    assert!(std::ptr::eq(
        state.effective_gen_delegs(),
        state.gen_delegs(),
    ));
}

#[test]
fn test_registered_drep_new_has_no_activity() {
    let drep = RegisteredDrep::new(500_000_000, None);
    assert_eq!(drep.last_active_epoch(), None);
}

#[test]
fn test_registered_drep_new_active() {
    let drep = RegisteredDrep::new_active(500_000_000, None, EpochNo(42));
    assert_eq!(drep.last_active_epoch(), Some(EpochNo(42)));
}

#[test]
fn test_registered_drep_touch_activity() {
    let mut drep = RegisteredDrep::new(500_000_000, None);
    assert_eq!(drep.last_active_epoch(), None);
    drep.touch_activity(EpochNo(10));
    assert_eq!(drep.last_active_epoch(), Some(EpochNo(10)));
    drep.touch_activity(EpochNo(20));
    assert_eq!(drep.last_active_epoch(), Some(EpochNo(20)));
}

#[test]
fn test_registered_drep_cbor_round_trip_with_activity() {
    let drep = RegisteredDrep::new_active(500_000_000, None, EpochNo(99));
    let bytes = drep.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let restored = RegisteredDrep::decode_cbor(&mut dec).expect("decode");
    assert_eq!(restored, drep);
}

#[test]
fn test_registered_drep_cbor_round_trip_without_activity() {
    let drep = RegisteredDrep::new(500_000_000, None);
    let bytes = drep.to_cbor_bytes();
    let mut dec = Decoder::new(&bytes);
    let restored = RegisteredDrep::decode_cbor(&mut dec).expect("decode");
    assert_eq!(restored, drep);
    assert_eq!(restored.last_active_epoch(), None);
}

#[test]
fn test_registered_drep_cbor_backward_compat_2_element() {
    // Simulate a legacy 2-element array (no last_active_epoch).
    let mut enc = Encoder::new();
    enc.array(2);
    enc.null(); // no anchor
    enc.unsigned(500_000_000);
    let bytes = enc.into_bytes();

    let mut dec = Decoder::new(&bytes);
    let drep = RegisteredDrep::decode_cbor(&mut dec).expect("decode legacy");
    assert_eq!(drep.deposit(), 500_000_000);
    assert_eq!(drep.last_active_epoch(), None);
}

#[test]
fn test_drep_state_inactive_dreps() {
    let mut ds = DrepState::new();
    let d1 = DRep::KeyHash([0x01; 28]);
    let d2 = DRep::KeyHash([0x02; 28]);
    let d3 = DRep::ScriptHash([0x03; 28]);

    // d1: active epoch 80
    ds.register(d1, RegisteredDrep::new_active(1, None, EpochNo(80)));
    // d2: active epoch 95
    ds.register(d2, RegisteredDrep::new_active(1, None, EpochNo(95)));
    // d3: no activity epoch (legacy)
    ds.register(d3, RegisteredDrep::new(1, None));

    // drep_activity=10, epoch=100: d1 (80+10=90 < 100) is expired, d2 (95+10=105 >= 100) active
    let expired = ds.inactive_dreps(EpochNo(100), 10);
    assert_eq!(expired.len(), 1);
    assert!(expired.contains(&d1));
}

// ------------------------------------------------------------------
//  EnactState + enact_gov_action tests
// ------------------------------------------------------------------

fn sample_gov_action_id(tag: u8) -> crate::eras::conway::GovActionId {
    crate::eras::conway::GovActionId {
        transaction_id: [tag; 32],
        gov_action_index: tag as u16,
    }
}

fn sample_constitution(url: &str) -> crate::eras::conway::Constitution {
    crate::eras::conway::Constitution {
        anchor: crate::types::Anchor {
            url: url.to_owned(),
            data_hash: [0xAA; 32],
        },
        guardrails_script_hash: None,
    }
}

fn sample_reward_account(id: u8) -> RewardAccount {
    RewardAccount {
        network: 1,
        credential: crate::StakeCredential::AddrKeyHash([id; 28]),
    }
}

#[test]
fn test_enact_state_default_and_roundtrip() {
    let es = EnactState::default();
    assert!(es.prev_pparams_update().is_none());
    assert!(es.prev_hard_fork().is_none());
    assert!(es.prev_committee().is_none());
    assert!(es.prev_constitution().is_none());
    assert_eq!(es.committee_quorum().numerator, 0);
    assert_eq!(es.committee_quorum().denominator, 1);
    // CBOR round-trip
    let bytes = es.to_cbor_bytes();
    let decoded = EnactState::from_cbor_bytes(&bytes).unwrap();
    assert_eq!(es, decoded);
}

#[test]
fn test_enact_info_action_no_effect() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let outcome = enact_gov_action(
        &mut es,
        sample_gov_action_id(1),
        &GovAction::InfoAction,
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::NoEffect);
    // No lineage should be recorded.
    assert!(es.prev_pparams_update().is_none());
    assert!(es.prev_hard_fork().is_none());
    assert!(es.prev_committee().is_none());
    assert!(es.prev_constitution().is_none());
}

#[test]
fn test_enact_new_constitution() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(2);
    let new_const = sample_constitution("https://example.com/constitution");

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::NewConstitution {
            prev_action_id: None,
            constitution: new_const.clone(),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::ConstitutionUpdated);
    assert_eq!(es.constitution(), &new_const);
    assert_eq!(es.prev_constitution(), Some(&action_id));
    // Other lineages untouched.
    assert!(es.prev_pparams_update().is_none());
}

#[test]
fn test_enact_no_confidence() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let cred = crate::StakeCredential::AddrKeyHash([0x11; 28]);
    committee.register_with_term(cred, 100);
    assert_eq!(committee.len(), 1);

    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(3);

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::NoConfidence {
            prev_action_id: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::CommitteeRemoved);
    // Upstream: NoConfidence sets `ensCommittee = SNothing`, removing all
    // committeeMembers — but csCommitteeCreds entries are preserved.
    // In our combined model the entry survives with `expires_at = None`.
    assert_eq!(committee.len(), 1);
    assert!(committee.get(&cred).unwrap().expires_at().is_none());
    assert_eq!(es.prev_committee(), Some(&action_id));
    // Quorum reset to 0/1.
    assert_eq!(es.committee_quorum().numerator, 0);
}

#[test]
fn test_enact_update_committee() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let existing = crate::StakeCredential::AddrKeyHash([0x01; 28]);
    let to_remove = crate::StakeCredential::AddrKeyHash([0x02; 28]);
    let new_member = crate::StakeCredential::AddrKeyHash([0x03; 28]);
    committee.register_with_term(existing, 200);
    committee.register_with_term(to_remove, 200);
    assert_eq!(committee.len(), 2);

    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(4);

    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(new_member, 500); // term epoch

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![to_remove],
            members_to_add,
            quorum: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::CommitteeUpdated {
            members_removed: 1,
            members_added: 1,
        }
    );
    // Upstream: members_to_remove only clears committeeMembers — the
    // entry in csCommitteeCreds is preserved.  Combined model: entry
    // survives with expires_at = None.
    assert_eq!(committee.len(), 3); // existing + to_remove(cleared) + new_member
    assert!(committee.get(&existing).unwrap().expires_at().is_some());
    assert!(committee.get(&to_remove).unwrap().expires_at().is_none());
    assert!(committee.get(&new_member).unwrap().expires_at().is_some());
    assert_eq!(es.committee_quorum().numerator, 2);
    assert_eq!(es.committee_quorum().denominator, 3);
    assert_eq!(es.prev_committee(), Some(&action_id));
}

#[test]
fn test_enact_update_committee_preserves_member_expirations_verbatim() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let existing = crate::StakeCredential::AddrKeyHash([0x21; 28]);
    let add_past = crate::StakeCredential::AddrKeyHash([0x22; 28]);
    let add_now = crate::StakeCredential::AddrKeyHash([0x23; 28]);
    let add_future = crate::StakeCredential::AddrKeyHash([0x24; 28]);
    committee.register(existing);

    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(add_past, 9);
    members_to_add.insert(add_now, 10);
    members_to_add.insert(add_future, 11);

    let outcome = super::enact::enact_gov_action_at_epoch(
        &mut es,
        EpochNo(10),
        sample_gov_action_id(41),
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );

    assert_eq!(
        outcome,
        EnactOutcome::CommitteeUpdated {
            members_removed: 0,
            members_added: 3,
        }
    );
    assert!(committee.is_member(&add_past));
    assert!(committee.is_member(&add_now));
    assert!(committee.is_member(&add_future));
}

#[test]
fn test_enact_update_committee_does_not_filter_by_term_limit() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let existing = crate::StakeCredential::AddrKeyHash([0x31; 28]);
    let add_within_limit = crate::StakeCredential::AddrKeyHash([0x32; 28]);
    let add_beyond_limit = crate::StakeCredential::AddrKeyHash([0x33; 28]);
    committee.register(existing);

    let mut pp = Some(crate::protocol_params::ProtocolParameters {
        committee_term_limit: Some(2),
        ..Default::default()
    });
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(add_within_limit, 12); // epoch 10 + 2 => accepted
    members_to_add.insert(add_beyond_limit, 13); // beyond term limit => ignored

    let outcome = super::enact::enact_gov_action_at_epoch(
        &mut es,
        EpochNo(10),
        sample_gov_action_id(43),
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );

    assert_eq!(
        outcome,
        EnactOutcome::CommitteeUpdated {
            members_removed: 0,
            members_added: 2,
        }
    );
    assert!(committee.is_member(&add_within_limit));
    assert!(committee.is_member(&add_beyond_limit));
}

#[test]
fn test_enact_hard_fork_initiation() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = Some(crate::protocol_params::ProtocolParameters::alonzo_defaults());
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(5);

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::HardForkEnacted {
            new_version: (10, 0),
        }
    );
    assert_eq!(pp.unwrap().protocol_version, Some((10, 0)));
    assert_eq!(es.prev_hard_fork(), Some(&action_id));
}

#[test]
fn test_enact_hard_fork_initializes_protocol_params_when_missing() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(42);

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::HardForkEnacted {
            new_version: (10, 0),
        }
    );
    assert_eq!(pp.and_then(|p| p.protocol_version), Some((10, 0)));
    assert_eq!(es.prev_hard_fork(), Some(&action_id));
}

#[test]
fn test_enact_treasury_withdrawals() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let ra1 = sample_reward_account(1);
    let ra2 = sample_reward_account(2);
    let ra_unknown = sample_reward_account(99);
    let mut ra = RewardAccounts::new();
    ra.insert(ra1, RewardAccountState::new(1000, None));
    ra.insert(ra2, RewardAccountState::new(500, None));
    let mut acc = AccountingState {
        treasury: 5000,
        reserves: 100_000,
    };
    let action_id = sample_gov_action_id(6);

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra1, 200);
    withdrawals.insert(ra2, 100);
    withdrawals.insert(ra_unknown, 50); // unregistered — should be ignored

    let outcome = enact_gov_action(
        &mut es,
        action_id,
        &GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::TreasuryWithdrawn {
            total_withdrawn: 300,
        }
    );
    assert_eq!(ra.balance(&ra1), 1200); // 1000 + 200
    assert_eq!(ra.balance(&ra2), 600); // 500 + 100
    assert_eq!(acc.treasury, 4700); // 5000 - 300
    // No lineage tracked for treasury withdrawals.
    assert!(es.prev_pparams_update().is_none());
}

#[test]
fn test_enact_parameter_change_recorded() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(7);

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(500),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::ParameterChangeRecorded);
    assert_eq!(es.prev_pparams_update(), Some(&action_id));
    assert_eq!(pp.as_ref().map(|p| p.min_fee_a), Some(500));
}

#[test]
fn test_enact_lineage_chaining() {
    // Enact two constitutions in sequence — the second should
    // reference the first as prev_constitution.
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let id1 = sample_gov_action_id(10);
    let id2 = sample_gov_action_id(11);

    enact_gov_action(
        &mut es,
        id1.clone(),
        &GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("v1"),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(es.prev_constitution(), Some(&id1));

    enact_gov_action(
        &mut es,
        id2.clone(),
        &GovAction::NewConstitution {
            prev_action_id: Some(id1.clone()),
            constitution: sample_constitution("v2"),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(es.prev_constitution(), Some(&id2));
    assert_eq!(es.constitution().anchor.url, "v2");
}

#[test]
fn test_enact_state_cbor_round_trip_with_lineage() {
    let mut es = EnactState::new();
    es.constitution = sample_constitution("https://example.com");
    es.committee_quorum = UnitInterval {
        numerator: 2,
        denominator: 3,
    };
    es.prev_pparams_update = Some(sample_gov_action_id(1));
    es.prev_hard_fork = Some(sample_gov_action_id(2));
    es.prev_committee = None;
    es.prev_constitution = Some(sample_gov_action_id(4));

    let bytes = es.to_cbor_bytes();
    let decoded = EnactState::from_cbor_bytes(&bytes).unwrap();
    assert_eq!(es, decoded);
}

// ── Enactment edge-case tests ──────────────────────────────────

#[test]
fn test_enact_update_committee_remove_nonexistent_member() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let existing = crate::StakeCredential::AddrKeyHash([0xA1; 28]);
    let ghost = crate::StakeCredential::AddrKeyHash([0xA2; 28]);
    committee.register(existing);
    assert_eq!(committee.len(), 1);

    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let outcome = enact_gov_action(
        &mut es,
        sample_gov_action_id(50),
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![ghost],
            members_to_add: std::collections::BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::CommitteeUpdated {
            members_removed: 0,
            members_added: 0
        }
    );
    assert_eq!(committee.len(), 1);
    assert!(committee.is_member(&existing));
}

#[test]
fn test_enact_no_confidence_on_empty_committee() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    assert_eq!(committee.len(), 0);

    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(51);

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::NoConfidence {
            prev_action_id: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::CommitteeRemoved);
    assert_eq!(committee.len(), 0);
    assert_eq!(es.committee_quorum().numerator, 0);
    assert_eq!(es.committee_quorum().denominator, 1);
    assert_eq!(es.prev_committee(), Some(&action_id));
}

#[test]
fn test_enact_parameter_change_multi_field() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = Some(crate::protocol_params::ProtocolParameters {
        min_fee_a: 100,
        min_fee_b: 200,
        max_tx_size: 4096,
        ..Default::default()
    });
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();
    let action_id = sample_gov_action_id(52);

    let outcome = enact_gov_action(
        &mut es,
        action_id.clone(),
        &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(999),
                min_fee_b: Some(888),
                max_tx_size: Some(8192),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::ParameterChangeRecorded);
    let p = pp.unwrap();
    assert_eq!(p.min_fee_a, 999);
    assert_eq!(p.min_fee_b, 888);
    assert_eq!(p.max_tx_size, 8192);
    assert_eq!(es.prev_pparams_update(), Some(&action_id));
}

#[test]
fn test_enact_treasury_withdrawals_zero_amount_skipped() {
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let ra1 = sample_reward_account(10);
    let mut ra = RewardAccounts::new();
    ra.insert(ra1, RewardAccountState::new(500, None));
    let mut acc = AccountingState {
        treasury: 1000,
        reserves: 0,
    };

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra1, 0);

    let outcome = enact_gov_action(
        &mut es,
        sample_gov_action_id(53),
        &GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::TreasuryWithdrawn { total_withdrawn: 0 }
    );
    assert_eq!(ra.balance(&ra1), 500); // unchanged
    assert_eq!(acc.treasury, 1000); // unchanged
}

#[test]
fn test_enact_treasury_withdrawals_exceeds_treasury() {
    // When withdrawal amounts exceed treasury, saturating_sub
    // should bring treasury to 0 without panicking.
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let ra1 = sample_reward_account(20);
    let ra2 = sample_reward_account(21);
    let mut ra = RewardAccounts::new();
    ra.insert(ra1, RewardAccountState::new(0, None));
    ra.insert(ra2, RewardAccountState::new(0, None));
    let mut acc = AccountingState {
        treasury: 100,
        reserves: 0,
    };

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra1, 80);
    withdrawals.insert(ra2, 80);

    let outcome = enact_gov_action(
        &mut es,
        sample_gov_action_id(54),
        &GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::TreasuryWithdrawn {
            total_withdrawn: 160
        }
    );
    assert_eq!(ra.balance(&ra1), 80);
    assert_eq!(ra.balance(&ra2), 80);
    assert_eq!(acc.treasury, 0); // saturated to 0
}

#[test]
fn test_enact_update_committee_add_existing_member() {
    // Adding a member that already exists should NOT count as "added".
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let existing = crate::StakeCredential::AddrKeyHash([0xB1; 28]);
    committee.register(existing);

    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(existing, 100); // already exists

    let outcome = enact_gov_action(
        &mut es,
        sample_gov_action_id(55),
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome,
        EnactOutcome::CommitteeUpdated {
            members_removed: 0,
            members_added: 0
        }
    );
    assert_eq!(committee.len(), 1);
}

#[test]
fn test_enact_hard_fork_lineage_chain() {
    // Two sequential hard forks: v10 then v11.
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = Some(crate::protocol_params::ProtocolParameters::alonzo_defaults());
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let id1 = sample_gov_action_id(60);
    let id2 = sample_gov_action_id(61);

    let outcome1 = enact_gov_action(
        &mut es,
        id1.clone(),
        &GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome1,
        EnactOutcome::HardForkEnacted {
            new_version: (10, 0)
        }
    );
    assert_eq!(es.prev_hard_fork(), Some(&id1));

    let outcome2 = enact_gov_action(
        &mut es,
        id2.clone(),
        &GovAction::HardForkInitiation {
            prev_action_id: Some(id1),
            protocol_version: (11, 0),
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(
        outcome2,
        EnactOutcome::HardForkEnacted {
            new_version: (11, 0)
        }
    );
    assert_eq!(es.prev_hard_fork(), Some(&id2));
    assert_eq!(pp.unwrap().protocol_version, Some((11, 0)));
}

#[test]
fn test_enact_parameter_change_lineage_chain() {
    // Two sequential parameter changes — lineage advances.
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let id1 = sample_gov_action_id(70);
    let id2 = sample_gov_action_id(71);

    enact_gov_action(
        &mut es,
        id1.clone(),
        &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(100),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(es.prev_pparams_update(), Some(&id1));
    assert_eq!(pp.as_ref().unwrap().min_fee_a, 100);

    enact_gov_action(
        &mut es,
        id2.clone(),
        &GovAction::ParameterChange {
            prev_action_id: Some(id1),
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_b: Some(200),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(es.prev_pparams_update(), Some(&id2));
    let p = pp.unwrap();
    assert_eq!(p.min_fee_a, 100); // preserved from first
    assert_eq!(p.min_fee_b, 200); // applied from second
}

#[test]
fn test_enact_parameter_change_initializes_defaults_when_none() {
    // When protocol_params is None, ParameterChange should
    // initialize defaults then apply the update.
    let mut es = EnactState::new();
    let mut committee = CommitteeState::new();
    let mut pp: Option<crate::protocol_params::ProtocolParameters> = None;
    let mut ra = RewardAccounts::new();
    let mut acc = AccountingState::default();

    let outcome = enact_gov_action(
        &mut es,
        sample_gov_action_id(72),
        &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(42),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        &mut committee,
        &mut pp,
        &mut ra,
        &mut acc,
    );
    assert_eq!(outcome, EnactOutcome::ParameterChangeRecorded);
    assert!(pp.is_some());
    assert_eq!(pp.as_ref().unwrap().min_fee_a, 42);
    // Other fields retain their Default::default() values.
    let defaults = crate::protocol_params::ProtocolParameters::default();
    assert_eq!(pp.as_ref().unwrap().min_fee_b, defaults.min_fee_b);
}

#[test]
fn test_ledger_state_16_element_round_trip() {
    let mut ls = LedgerState::new(Era::Conway);
    ls.enact_state_mut().constitution = sample_constitution("test");
    ls.enact_state_mut().prev_hard_fork = Some(sample_gov_action_id(99));

    let bytes = ls.to_cbor_bytes();
    let restored = LedgerState::from_cbor_bytes(&bytes).unwrap();
    assert_eq!(restored.enact_state().constitution().anchor.url, "test");
    assert!(restored.enact_state().prev_hard_fork().is_some());
}

#[test]
fn test_ledger_state_15_element_backward_compat() {
    // Build a 15-element encoded LedgerState (pre-EnactState era)
    // and verify it decodes with default EnactState.
    let ls = LedgerState::new(Era::Shelley);
    // Encode with the old 15-element layout by manually encoding.
    let mut enc = Encoder::new();
    enc.array(15);
    ls.current_era.encode_cbor(&mut enc);
    ls.tip.encode_cbor(&mut enc);
    match ls.expected_network_id {
        Some(nid) => enc.unsigned(u64::from(nid)),
        None => enc.null(),
    };
    enc.map(0); // no governance actions
    ls.pool_state().encode_cbor(&mut enc);
    ls.stake_credentials().encode_cbor(&mut enc);
    ls.committee_state().encode_cbor(&mut enc);
    ls.drep_state().encode_cbor(&mut enc);
    ls.reward_accounts().encode_cbor(&mut enc);
    ls.multi_era_utxo().encode_cbor(&mut enc);
    ls.shelley_utxo.encode_cbor(&mut enc);
    enc.null(); // no protocol params
    ls.deposit_pot().encode_cbor(&mut enc);
    ls.accounting().encode_cbor(&mut enc);
    ls.current_epoch.encode_cbor(&mut enc);

    let bytes = enc.into_bytes();
    let decoded = LedgerState::from_cbor_bytes(&bytes).unwrap();
    // EnactState should be default when decoded from 15-element array.
    assert_eq!(decoded.enact_state(), &EnactState::default());
}

// ------------------------------------------------------------------
//  Enacted-root prev_action_id validation tests
// ------------------------------------------------------------------

fn sample_proposal(
    gov_action: GovAction,
    deposit: u64,
    ra_id: u8,
) -> crate::eras::conway::ProposalProcedure {
    use crate::eras::conway::ProposalProcedure;
    let ra = sample_reward_account(ra_id);
    ProposalProcedure {
        deposit,
        reward_account: ra.to_bytes().to_vec(),
        gov_action,
        anchor: crate::types::Anchor {
            url: "https://example.invalid".to_owned(),
            data_hash: [0xCC; 32],
        },
    }
}

fn sample_governance_actions_with(
    entries: Vec<(crate::eras::conway::GovActionId, GovAction)>,
) -> BTreeMap<crate::eras::conway::GovActionId, GovernanceActionState> {
    let mut map = BTreeMap::new();
    for (id, action) in entries {
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: 1,
            reward_account: sample_reward_account(1).to_bytes().to_vec(),
            gov_action: action,
            anchor: crate::types::Anchor {
                url: "https://example.invalid/stored".to_owned(),
                data_hash: [0xDD; 32],
            },
        };
        map.insert(id, GovernanceActionState::new(proposal));
    }
    map
}

fn empty_stake_creds_with(ra_id: u8) -> StakeCredentials {
    let mut sc = StakeCredentials::new();
    let ra = sample_reward_account(ra_id);
    sc.register(ra.credential);
    sc
}

#[test]
fn test_enacted_root_none_accepts_fresh_proposal_without_prev() {
    // EnactState has no enacted root for Committee purpose.
    // Proposal with prev_action_id = None should be accepted.
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::NoConfidence {
            prev_action_id: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_enacted_root_some_rejects_fresh_proposal_without_prev() {
    // EnactState has an enacted root for Committee purpose.
    // Proposal with prev_action_id = None should be rejected.
    let mut es = EnactState::default();
    es.prev_committee = Some(sample_gov_action_id(10));
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::NoConfidence {
            prev_action_id: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidPrevGovActionId(_))
    ));
}

#[test]
fn test_enacted_root_matching_prev_accepted() {
    // EnactState has an enacted root for Constitution purpose.
    // Proposal that references the enacted root should be accepted.
    let root_id = sample_gov_action_id(20);
    let mut es = EnactState::default();
    es.prev_constitution = Some(root_id.clone());
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::NewConstitution {
            prev_action_id: Some(root_id.clone()),
            constitution: sample_constitution("v3"),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_enacted_root_wrong_purpose_prev_rejected() {
    // EnactState has an enacted root for Constitution, but proposal
    // is ParameterChange referencing it — wrong purpose.
    let root_id = sample_gov_action_id(30);
    let mut es = EnactState::default();
    es.prev_constitution = Some(root_id.clone());
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: Some(root_id.clone()),
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidPrevGovActionId(_))
    ));
}

#[test]
fn test_enacted_root_pending_proposal_accepted() {
    // EnactState has enacted root for HardFork != prev, but a stored
    // pending proposal has the matching id and purpose.
    let enacted_id = sample_gov_action_id(40);
    let pending_id = sample_gov_action_id(41);
    let mut es = EnactState::default();
    es.prev_hard_fork = Some(enacted_id);
    let stake_creds = empty_stake_creds_with(1);
    let mut stored = sample_governance_actions_with(vec![(
        pending_id.clone(),
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (9, 1),
        },
    )]);
    let proposals = vec![sample_proposal(
        GovAction::HardForkInitiation {
            prev_action_id: Some(pending_id.clone()),
            protocol_version: (10, 0),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut stored,
        &stake_creds,
        Some((9, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_hard_fork_prev_enacted_root_requires_pv_follow() {
    let root_id = sample_gov_action_id(42);
    let mut es = EnactState::default();
    es.prev_hard_fork = Some(root_id.clone());
    let stake_creds = empty_stake_creds_with(1);

    let valid = vec![sample_proposal(
        GovAction::HardForkInitiation {
            prev_action_id: Some(root_id.clone()),
            protocol_version: (10, 1),
        },
        1,
        1,
    )];
    let valid_result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &valid,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(valid_result.is_ok());

    let invalid = vec![sample_proposal(
        GovAction::HardForkInitiation {
            prev_action_id: Some(root_id),
            protocol_version: (10, 2),
        },
        1,
        1,
    )];
    let invalid_result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &invalid,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        invalid_result,
        Err(LedgerError::ProposalCantFollow { .. })
    ));
}

/// Upstream `preceedingHardFork` safety guard: when the proposed major
/// version exceeds `succVersion(pvMajor current)` (i.e. jumps more than
/// one major version ahead of the live protocol), the check must compare
/// against the current PP version rather than following the stored
/// proposal chain, so `pvCanFollow` fails.
///
/// Example: current PP = 9.0, stored HardFork A = 10.0, new proposal B
/// referencing A with version 11.0 must be rejected.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Gov` — `preceedingHardFork`:
///   `Just (pvMajor newProtVer) > succVersion (pvMajor (pp ^. ppProtocolVersionL))`
#[test]
fn test_hard_fork_chain_rejects_major_version_jump() {
    let stored_id = sample_gov_action_id(70);
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut stored_actions = sample_governance_actions_with(vec![(
        stored_id.clone(),
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
    )]);

    // Proposal that chains off stored A: version 11.0 while current PP
    // is 9.0 — jumps more than one major version, must be rejected.
    let proposal = vec![sample_proposal(
        GovAction::HardForkInitiation {
            prev_action_id: Some(stored_id.clone()),
            protocol_version: (11, 0),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xBB; 32]),
        &proposal,
        EpochNo(0),
        &mut stored_actions,
        &stake_creds,
        Some((9, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        matches!(result, Err(LedgerError::ProposalCantFollow { .. })),
        "chained HardFork 9.0 → stored 10.0 → proposed 11.0 should be rejected: {:?}",
        result
    );

    // Same chain but with current PP 10.0 — version 11.0 is within
    // one major step, should be allowed.
    let mut stored_actions2 = sample_governance_actions_with(vec![(
        stored_id.clone(),
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
    )]);
    let ok_result = validate_conway_proposals(
        crate::types::TxId([0xBB; 32]),
        &proposal,
        EpochNo(0),
        &mut stored_actions2,
        &stake_creds,
        Some((10, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        ok_result.is_ok(),
        "chained HardFork 10.0 → stored 10.0 → proposed 11.0 should be accepted: {:?}",
        ok_result
    );
}

#[test]
fn test_enacted_root_unknown_prev_rejected() {
    // prev_action_id matches neither enacted root nor stored proposals.
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let unknown_id = sample_gov_action_id(99);
    let proposals = vec![sample_proposal(
        GovAction::NewConstitution {
            prev_action_id: Some(unknown_id),
            constitution: sample_constitution("orphan"),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidPrevGovActionId(_))
    ));
}

#[test]
fn test_enacted_root_treasury_and_info_skip_lineage() {
    // TreasuryWithdrawals and InfoAction have no lineage concept.
    // They should be accepted regardless of EnactState.
    let mut es = EnactState::default();
    es.prev_pparams_update = Some(sample_gov_action_id(50));
    es.prev_hard_fork = Some(sample_gov_action_id(51));
    es.prev_committee = Some(sample_gov_action_id(52));
    es.prev_constitution = Some(sample_gov_action_id(53));
    let stake_creds = empty_stake_creds_with(1);
    let mut withdrawals = std::collections::BTreeMap::new();
    let ra = sample_reward_account(1);
    withdrawals.insert(ra, 100);
    let proposals = vec![
        sample_proposal(
            GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            1,
            1,
        ),
        sample_proposal(GovAction::InfoAction, 1, 1),
    ];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_enacted_root_update_committee_shares_committee_purpose() {
    // UpdateCommittee and NoConfidence share the Committee purpose.
    // An enacted NoConfidence root should accept an UpdateCommittee
    // referencing it.
    let root_id = sample_gov_action_id(60);
    let mut es = EnactState::default();
    es.prev_committee = Some(root_id.clone());
    let stake_creds = empty_stake_creds_with(1);
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(
        crate::StakeCredential::AddrKeyHash([0x33; 28]),
        500, // term epoch
    );
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: Some(root_id.clone()),
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_hard_fork_rejects_when_current_protocol_version_missing() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        1,
        1,
    )];

    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::MissingProtocolVersionForHardFork(_))
    ));
}

#[test]
fn test_hard_fork_prev_enacted_root_rejects_when_current_protocol_version_missing() {
    let root_id = sample_gov_action_id(70);
    let mut es = EnactState::default();
    es.prev_hard_fork = Some(root_id.clone());
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::HardForkInitiation {
            prev_action_id: Some(root_id),
            protocol_version: (10, 1),
        },
        1,
        1,
    )];

    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::MissingProtocolVersionForHardFork(_))
    ));
}

#[test]
fn test_bootstrap_rejects_non_bootstrap_proposal_action() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("bootstrap-disallowed"),
        },
        1,
        1,
    )];

    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((9, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedProposalDuringBootstrap(_))
    ));
}

#[test]
fn test_bootstrap_allows_parameter_change_proposal_action() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];

    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((9, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_info_action_proposal() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(GovAction::InfoAction, 1, 1)];

    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((9, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_rejects_drep_vote_on_non_info_action() {
    let drep_voter = Voter::DRepKeyHash([0x66; 28]);
    let action_id = sample_gov_action_id(71);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(drep_voter.clone(), inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedVotesDuringBootstrap(ref entries))
            if entries == &vec![(drep_voter, action_id)]
    ));
}

#[test]
fn test_bootstrap_rejects_committee_vote_on_non_bootstrap_action() {
    let committee_voter = Voter::CommitteeKeyHash([0x67; 28]);
    let action_id = sample_gov_action_id(72);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("bootstrap-committee-disallowed"),
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(committee_voter.clone(), inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedVotesDuringBootstrap(ref entries))
            if entries == &vec![(committee_voter, action_id)]
    ));
}

#[test]
fn test_bootstrap_rejects_spo_vote_on_non_bootstrap_action() {
    let spo_voter = Voter::StakePool([0x68; 28]);
    let action_id = sample_gov_action_id(73);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::from([(sample_reward_account(7), 1)]),
            guardrails_script_hash: None,
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter.clone(), inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedVotesDuringBootstrap(ref entries))
            if entries == &vec![(spo_voter, action_id)]
    ));
}

#[test]
fn test_bootstrap_allows_drep_vote_on_info_action() {
    let drep_voter = Voter::DRepKeyHash([0x69; 28]);
    let action_id = sample_gov_action_id(74);
    let governance_actions =
        sample_governance_actions_with(vec![(action_id.clone(), GovAction::InfoAction)]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(drep_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_committee_vote_on_hard_fork_action() {
    let committee_voter = Voter::CommitteeKeyHash([0x6A; 28]);
    let action_id = sample_gov_action_id(75);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(committee_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_committee_vote_on_parameter_change_action() {
    let committee_voter = Voter::CommitteeKeyHash([0x6C; 28]);
    let action_id = sample_gov_action_id(77);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(committee_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_spo_vote_on_hard_fork_action() {
    let spo_voter = Voter::StakePool([0x6B; 28]);
    let action_id = sample_gov_action_id(76);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_spo_vote_on_parameter_change_action() {
    let spo_voter = Voter::StakePool([0x6D; 28]);
    let action_id = sample_gov_action_id(78);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_committee_vote_on_info_action() {
    let committee_voter = Voter::CommitteeKeyHash([0x6E; 28]);
    let action_id = sample_gov_action_id(79);
    let governance_actions =
        sample_governance_actions_with(vec![(action_id.clone(), GovAction::InfoAction)]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(committee_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

#[test]
fn test_bootstrap_allows_spo_vote_on_info_action() {
    let spo_voter = Voter::StakePool([0x6F; 28]);
    let action_id = sample_gov_action_id(80);
    let governance_actions =
        sample_governance_actions_with(vec![(action_id.clone(), GovAction::InfoAction)]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        Some((9, 0)),
    );
    assert!(result.is_ok());
}

// --- Post-bootstrap (non-bootstrap) voter permission tests ---

#[test]
fn test_post_bootstrap_spo_vote_allowed_on_security_group_parameter_change() {
    let spo_voter = Voter::StakePool([0xA0; 28]);
    let action_id = sample_gov_action_id(90);
    // min_fee_a is Economic + Security group, so SPO should be allowed
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(500),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id,
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter, inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        None, // post-bootstrap
    );
    assert!(result.is_ok());
}

#[test]
fn test_post_bootstrap_spo_vote_rejected_on_non_security_parameter_change() {
    let spo_voter = Voter::StakePool([0xA1; 28]);
    let action_id = sample_gov_action_id(91);
    // key_deposit is Economic only (no security group), so SPO should be rejected
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(2_000_000),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter.clone(), inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        None, // post-bootstrap
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedVoters(ref entries))
            if entries == &vec![(spo_voter, action_id)]
    ));
}

#[test]
fn test_post_bootstrap_spo_vote_rejected_on_new_constitution() {
    let spo_voter = Voter::StakePool([0xA2; 28]);
    let action_id = sample_gov_action_id(92);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("post-bootstrap-constitution"),
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(spo_voter.clone(), inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        None, // post-bootstrap
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedVoters(ref entries))
            if entries == &vec![(spo_voter, action_id)]
    ));
}

#[test]
fn test_post_bootstrap_committee_vote_rejected_on_no_confidence() {
    let committee_voter = Voter::CommitteeKeyHash([0xA3; 28]);
    let action_id = sample_gov_action_id(93);
    let governance_actions = sample_governance_actions_with(vec![(
        action_id.clone(),
        GovAction::NoConfidence {
            prev_action_id: None,
        },
    )]);

    let mut inner = BTreeMap::new();
    inner.insert(
        action_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );
    let voting_procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::from([(committee_voter.clone(), inner)]),
    };

    let result = validate_conway_voter_permissions(
        EpochNo(0),
        &voting_procedures,
        &governance_actions,
        None, // post-bootstrap
    );
    assert!(matches!(
        result,
        Err(LedgerError::DisallowedVoters(ref entries))
            if entries == &vec![(committee_voter, action_id)]
    ));
}

#[test]
fn test_parameter_change_rejects_malformed_unit_interval() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                price_mem: Some(UnitInterval {
                    numerator: 2,
                    denominator: 1,
                }),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

/// Upstream `ppuWellFormed` does not check cross-field relationships between
/// `max_tx_size` and `max_block_body_size` within the same update.
/// Reference: `Cardano.Ledger.Conway.PParams` — `ppuWellFormed`.
#[test]
fn test_parameter_change_accepts_tx_size_larger_than_block_body_size() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_block_body_size: Some(100),
                max_tx_size: Some(101),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "cross-field tx_size > block_body_size accepted (no upstream check)"
    );
}

/// Upstream `ppuWellFormed` does not merge proposed values with current
/// protocol parameters for cross-field consistency checks.
/// Reference: `Cardano.Ledger.Conway.PParams` — `ppuWellFormed`.
#[test]
fn test_parameter_change_accepts_tx_size_larger_than_current_block_body_size() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_tx_size: Some(501),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let protocol_params = crate::protocol_params::ProtocolParameters {
        max_block_body_size: 500,
        ..Default::default()
    };
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        Some(&protocol_params),
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "tx_size > current block_body_size accepted (no effective merge in upstream)"
    );
}

#[test]
fn test_parameter_change_rejects_protocol_version_update() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                protocol_version: Some((10, 0)),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

#[test]
fn test_parameter_change_rejects_zero_pool_and_gov_deposits() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let pool_zero = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                pool_deposit: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let gov_zero = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                gov_action_deposit: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];

    let pool_result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &pool_zero,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        pool_result,
        Err(LedgerError::MalformedProposal(_))
    ));

    let gov_result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &gov_zero,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(gov_result, Err(LedgerError::MalformedProposal(_))));
}

/// Upstream ppuWellFormed: `max_collateral_inputs == 0`, `min_committee_size == 0`,
/// and `drep_activity == 0` are NOT rejected. Only the exact upstream set of
/// zero-reject fields triggers `MalformedProposal`.
#[test]
fn test_ppu_well_formed_accepts_fields_not_in_upstream_zero_list() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);

    // max_collateral_inputs == 0 is ACCEPTED (not in upstream ppuWellFormed)
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_collateral_inputs: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok(), "max_collateral_inputs=0 should be accepted");

    // min_committee_size == 0 is ACCEPTED (not in upstream ppuWellFormed)
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_committee_size: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok(), "min_committee_size=0 should be accepted");

    // drep_activity == 0 is ACCEPTED (not in upstream ppuWellFormed)
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                drep_activity: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok(), "drep_activity=0 should be accepted");
}

/// Upstream ppuWellFormed: `coinsPerUTxOByte == 0` is rejected only outside
/// bootstrap phase (hardforkConwayBootstrapPhase pv == False, i.e. PV >= 10).
#[test]
fn test_ppu_well_formed_coins_per_utxo_byte_zero_post_bootstrap() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                coins_per_utxo_byte: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];

    // PV 10 (post-bootstrap): rejected
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        matches!(result, Err(LedgerError::MalformedProposal(_))),
        "coinsPerUTxOByte=0 rejected at PV 10 (post-bootstrap)"
    );

    // PV 9 (bootstrap): accepted
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((9, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "coinsPerUTxOByte=0 accepted at PV 9 (bootstrap phase)"
    );
}

/// Upstream ppuWellFormed: `nOpt == 0` is rejected only at PV >= 11.
#[test]
fn test_ppu_well_formed_n_opt_zero_pv11() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                n_opt: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];

    // PV 11: rejected
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((11, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        matches!(result, Err(LedgerError::MalformedProposal(_))),
        "nOpt=0 rejected at PV 11"
    );

    // PV 10: accepted
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)),
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok(), "nOpt=0 accepted at PV 10 (< 11)");
}

/// Upstream `ppuWellFormed` does NOT perform cross-field consistency
/// checks or merge proposed values with current protocol parameters.
/// A proposal that sets `max_tx_size` larger than the current
/// `max_block_body_size` is still accepted.
/// Reference: `Cardano.Ledger.Conway.PParams` — `ppuWellFormed`.
#[test]
fn test_ppu_well_formed_no_cross_field_consistency() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut params = crate::protocol_params::ProtocolParameters::default();
    params.max_block_body_size = 65536;
    params.max_tx_size = 16384;

    // Propose max_tx_size larger than current max_block_body_size.
    // Upstream ppuWellFormed accepts individual non-zero values
    // without checking cross-field relationships.
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_tx_size: Some(100_000),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)),
        None,
        None,
        Some(&params),
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "max_tx_size > max_block_body_size accepted (no cross-field check in upstream)"
    );
}

/// Upstream GOV rule does NOT have `ExpirationEpochTooLarge` — committee member
/// expiration epochs beyond the term limit are accepted.
#[test]
fn test_update_committee_accepts_expiration_beyond_term_limit() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut members_to_add = std::collections::BTreeMap::new();
    // Expiration 13 with current epoch 10 and term limit 2 => epoch 12 max
    // This would have been rejected by ExpirationEpochTooLarge, but upstream
    // GOV only checks ExpirationEpochTooSmall.
    members_to_add.insert(crate::StakeCredential::AddrKeyHash([0x44; 28]), 13);
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        1,
        1,
    )];
    let protocol_params = crate::protocol_params::ProtocolParameters {
        committee_term_limit: Some(2),
        ..Default::default()
    };
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(10),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        Some(&protocol_params),
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "committee term limit is not checked at GOV level — upstream parity"
    );
}

// -----------------------------------------------------------------------
// Ratification tally tests
// -----------------------------------------------------------------------

fn test_info_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::InfoAction,
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

fn test_hf_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

fn test_treasury_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

fn test_no_confidence_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::NoConfidence {
            prev_action_id: None,
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

/// Authorize a hot credential for a committee member in test setups.
///
/// In Conway, committee votes are keyed by HOT credentials (CDDL tags
/// 0/1).  Tests must authorize a hot credential for each cold member
/// before inserting votes, otherwise `tally_committee_votes` cannot
/// resolve votes.
fn authorize_cc_hot(cs: &mut CommitteeState, cold: StakeCredential, hot: StakeCredential) {
    cs.get_mut(&cold)
        .expect("cold credential not registered")
        .set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(hot)));
}

// -- VoteTally::meets_threshold ---

#[test]
fn tally_meets_threshold_exact() {
    let tally = VoteTally {
        yes: 67,
        no: 33,
        abstain: 0,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(tally.meets_threshold(&threshold));
}

#[test]
fn tally_below_threshold() {
    let tally = VoteTally {
        yes: 66,
        no: 34,
        abstain: 0,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn tally_above_threshold() {
    let tally = VoteTally {
        yes: 80,
        no: 20,
        abstain: 0,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(tally.meets_threshold(&threshold));
}

#[test]
fn tally_all_abstain_fails_positive_threshold() {
    // All abstain → active = 0 → upstream `%?` returns 0 → fails
    // any positive threshold (only `r == minBound` passes).
    let tally = VoteTally {
        yes: 0,
        no: 0,
        abstain: 100,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn tally_with_abstentions_excluded() {
    // 60 yes, 20 no, 20 abstain. Active = 80. 60/80 = 75% >= 67%.
    let tally = VoteTally {
        yes: 60,
        no: 20,
        abstain: 20,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(tally.meets_threshold(&threshold));
}

#[test]
fn tally_zero_total_fails_positive_threshold() {
    // Zero total → active = 0 → upstream `%?` returns 0 → fails.
    let tally = VoteTally {
        yes: 0,
        no: 0,
        abstain: 0,
        total: 0,
    };
    let threshold = UnitInterval {
        numerator: 1,
        denominator: 2,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn tally_zero_total_passes_zero_threshold() {
    // Zero total + zero threshold → upstream `r == minBound` → passes.
    let tally = VoteTally {
        yes: 0,
        no: 0,
        abstain: 0,
        total: 0,
    };
    let threshold = UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    assert!(tally.meets_threshold(&threshold));
}

// -- Committee tally ---

#[test]
fn committee_tally_unanimous_yes() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();

    let cold_a = StakeCredential::AddrKeyHash([1; 28]);
    let cold_b = StakeCredential::AddrKeyHash([2; 28]);
    let hot_a = StakeCredential::AddrKeyHash([11; 28]);
    let hot_b = StakeCredential::AddrKeyHash([12; 28]);
    cs.register_with_term(cold_a, 999);
    cs.register_with_term(cold_b, 999);
    authorize_cc_hot(&mut cs, cold_a, hot_a);
    authorize_cc_hot(&mut cs, cold_b, hot_b);

    // Both vote yes (votes keyed by HOT credential hash).
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([12; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(tally.yes, 2);
    assert_eq!(tally.no, 0);
    assert_eq!(tally.total, 2);
    let quorum = UnitInterval {
        numerator: 2,
        denominator: 3,
    };
    assert!(tally.meets_threshold(&quorum));
}

#[test]
fn committee_tally_resigned_excluded() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();

    let cold_a = StakeCredential::AddrKeyHash([1; 28]);
    let cold_b = StakeCredential::AddrKeyHash([2; 28]);
    let hot_a = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold_a, 999);
    cs.register_with_term(cold_b, 999);
    authorize_cc_hot(&mut cs, cold_a, hot_a);
    // Resign member B (no hot credential needed for resigned members).
    cs.get_mut(&cold_b)
        .unwrap()
        .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));

    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(tally.yes, 1);
    assert_eq!(tally.total, 1); // resigned excluded
}

#[test]
fn committee_tally_no_votes_fails_threshold() {
    let action = test_hf_action();
    let mut cs = CommitteeState::default();
    cs.register_with_term(StakeCredential::AddrKeyHash([1; 28]), 999);
    cs.register_with_term(StakeCredential::AddrKeyHash([2; 28]), 999);

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(tally.yes, 0);
    assert_eq!(tally.total, 2);
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };
    assert!(!tally.meets_threshold(&quorum));
}

// -- DRep tally ---

#[test]
fn drep_tally_weighted_by_stake() {
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep_a = DRep::KeyHash([1; 28]);
    let drep_b = DRep::KeyHash([2; 28]);
    drep_state.register(drep_a, RegisteredDrep::new_active(0, None, EpochNo(1)));
    drep_state.register(drep_b, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut stake = BTreeMap::new();
    stake.insert(drep_a, 700);
    stake.insert(drep_b, 300);

    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);
    action.votes.insert(Voter::DRepKeyHash([2; 28]), Vote::No);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.yes, 700);
    assert_eq!(tally.no, 300);
    assert_eq!(tally.total, 1000);
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(tally.meets_threshold(&threshold)); // 700/1000 = 70% >= 67%
}

#[test]
fn drep_tally_handles_always_abstain_and_no_confidence_without_panic() {
    // Regression guard for the `AlwaysAbstain` / `AlwaysNoConfidence`
    // paths through `tally_drep_votes`. The inner match at the bottom
    // of the loop previously used `unreachable!()` for these variants,
    // relying on the early filter at the top of the loop body to
    // short-circuit them. The refactor swapped `unreachable!()` for
    // `continue` as defensive-coding; this test pins the end-to-end
    // behavior at both the tally-correctness AND the no-panic level.
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep_regular = DRep::KeyHash([1; 28]);
    drep_state.register(
        drep_regular,
        RegisteredDrep::new_active(0, None, EpochNo(1)),
    );

    let mut stake = BTreeMap::new();
    // A registered DRep that votes Yes with stake 100.
    stake.insert(drep_regular, 100);
    // AlwaysAbstain stake — must NOT be added to `total`.
    stake.insert(DRep::AlwaysAbstain, 500);
    // AlwaysNoConfidence stake — added to `total`; counted as Yes
    // ONLY when `count_no_confidence_as_yes` is true.
    stake.insert(DRep::AlwaysNoConfidence, 200);

    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    // count_no_confidence_as_yes = false:
    //   total = regular (100) + AlwaysNoConfidence (200) = 300
    //   yes = regular's Yes (100) only
    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.total, 300, "AlwaysAbstain (500) must be excluded");
    assert_eq!(tally.yes, 100);

    // count_no_confidence_as_yes = true:
    //   total = 300 (same as above)
    //   yes = regular's Yes (100) + AlwaysNoConfidence (200) = 300
    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
    assert_eq!(tally.total, 300);
    assert_eq!(
        tally.yes, 300,
        "AlwaysNoConfidence stake counts as Yes when flag is set",
    );
}

#[test]
fn drep_tally_excludes_inactive() {
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep_a = DRep::KeyHash([1; 28]);
    let drep_b = DRep::KeyHash([2; 28]);
    // A: active epoch 90. Activity window 10. At epoch 105: 90+10=100 < 105 → inactive.
    drep_state.register(drep_a, RegisteredDrep::new_active(0, None, EpochNo(90)));
    // B: active epoch 100. 100+10=110 >= 105 → active.
    drep_state.register(drep_b, RegisteredDrep::new_active(0, None, EpochNo(100)));

    let mut stake = BTreeMap::new();
    stake.insert(drep_a, 500);
    stake.insert(drep_b, 500);

    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes); // inactive, excluded
    action.votes.insert(Voter::DRepKeyHash([2; 28]), Vote::Yes);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(105), 10, false);
    // Only DRep B counted (active). A is inactive and excluded.
    assert_eq!(tally.yes, 500);
    assert_eq!(tally.total, 500);
    let threshold = UnitInterval {
        numerator: 1,
        denominator: 2,
    };
    assert!(tally.meets_threshold(&threshold));
}

#[test]
fn drep_tally_unregistered_drep_excluded() {
    let action = test_hf_action();
    let drep_state = DrepState::new(); // empty — no DReps registered

    let mut stake = BTreeMap::new();
    stake.insert(DRep::KeyHash([1; 28]), 1000);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.total, 0); // no registered DReps
}

// -- SPO tally ---

#[test]
fn spo_tally_weighted_by_pool_stake() {
    let mut action = test_hf_action();

    let pool_a = [1u8; 28];
    let pool_b = [2u8; 28];

    // Build pool stake distribution manually.
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert(pool_a, 600u64);
    pool_stakes.insert(pool_b, 400u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    action.votes.insert(Voter::StakePool(pool_a), Vote::Yes);
    action.votes.insert(Voter::StakePool(pool_b), Vote::No);

    let tally = tally_spo_votes(
        &action,
        &pool_dist,
        false,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally.yes, 600);
    assert_eq!(tally.no, 400);
    assert_eq!(tally.total, 1000);
    let threshold = UnitInterval {
        numerator: 51,
        denominator: 100,
    };
    assert!(tally.meets_threshold(&threshold)); // 600/1000 = 60% >= 51%
}

// -- Parameter-group classification ---

#[test]
fn pparam_groups_empty_update_has_no_groups() {
    let update = crate::protocol_params::ProtocolParameterUpdate::default();
    let g = conway_modified_pparam_groups(&update);
    assert!(!g.network);
    assert!(!g.economic);
    assert!(!g.technical);
    assert!(!g.gov);
    assert!(!g.security);
    assert!(!g.has_drep_group());
}

#[test]
fn pparam_groups_min_fee_a_is_economic_plus_security() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        min_fee_a: Some(44),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(g.security);
    assert!(!g.network);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_min_fee_b_is_economic_plus_security() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        min_fee_b: Some(155381),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(g.security);
    assert!(!g.network);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_max_block_body_size_is_network_plus_security() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        max_block_body_size: Some(65536),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.network);
    assert!(g.security);
    assert!(!g.economic);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_max_tx_size_is_network_plus_security() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        max_tx_size: Some(16384),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.network);
    assert!(g.security);
}

#[test]
fn pparam_groups_key_deposit_is_economic_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        key_deposit: Some(2_000_000),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(!g.security);
    assert!(!g.network);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_pool_deposit_is_economic_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        pool_deposit: Some(500_000_000),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(!g.security);
}

#[test]
fn pparam_groups_n_opt_is_technical_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        n_opt: Some(500),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.technical);
    assert!(!g.security);
    assert!(!g.network);
    assert!(!g.economic);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_e_max_is_technical_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        e_max: Some(18),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.technical);
    assert!(!g.security);
}

#[test]
fn pparam_groups_collateral_percentage_is_technical_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        collateral_percentage: Some(150),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.technical);
    assert!(!g.security);
    assert!(!g.economic);
}

#[test]
fn pparam_groups_pool_voting_thresholds_is_gov_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        pool_voting_thresholds: Some(crate::protocol_params::PoolVotingThresholds::default()),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.gov);
    assert!(!g.security);
    assert!(!g.network);
    assert!(!g.economic);
    assert!(!g.technical);
}

#[test]
fn pparam_groups_drep_activity_is_gov_only() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        drep_activity: Some(20),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.gov);
    assert!(!g.security);
}

#[test]
fn pparam_groups_gov_action_deposit_is_gov_plus_security() {
    let update = crate::protocol_params::ProtocolParameterUpdate {
        gov_action_deposit: Some(100_000_000_000),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.gov);
    assert!(g.security);
    assert!(!g.network);
    assert!(!g.economic);
    assert!(!g.technical);
}

#[test]
fn pparam_groups_mixed_fields_combine_correctly() {
    // min_fee_a = economic+security, n_opt = technical, drep_activity = gov
    let update = crate::protocol_params::ProtocolParameterUpdate {
        min_fee_a: Some(44),
        n_opt: Some(500),
        drep_activity: Some(20),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(g.technical);
    assert!(g.gov);
    assert!(g.security);
    assert!(!g.network);
    assert!(g.has_drep_group());
}

#[test]
fn pparam_groups_security_only_update_has_no_drep_group() {
    // protocol_version is security-only in this implementation
    let update = crate::protocol_params::ProtocolParameterUpdate {
        protocol_version: Some((10, 0)),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.security);
    assert!(!g.has_drep_group());
    assert!(!g.network);
    assert!(!g.economic);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_coins_per_utxo_byte_is_economic_plus_security() {
    // Upstream: PPGroups 'EconomicGroup 'SecurityGroup
    let update = crate::protocol_params::ProtocolParameterUpdate {
        coins_per_utxo_byte: Some(4310),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(g.security);
    assert!(!g.network);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_min_fee_ref_script_cost_per_byte_is_economic_plus_security() {
    // Upstream: PPGroups 'EconomicGroup 'SecurityGroup
    let update = crate::protocol_params::ProtocolParameterUpdate {
        min_fee_ref_script_cost_per_byte: Some(UnitInterval {
            numerator: 15,
            denominator: 1,
        }),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(g.security);
    assert!(!g.network);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_max_tx_ex_units_is_network_only() {
    // Upstream: PPGroups 'NetworkGroup 'NoStakePoolGroup
    let update = crate::protocol_params::ProtocolParameterUpdate {
        max_tx_ex_units: Some(crate::eras::alonzo::ExUnits {
            mem: 14_000_000,
            steps: 10_000_000,
        }),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.network);
    assert!(!g.security);
    assert!(!g.economic);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_max_collateral_inputs_is_network_only() {
    // Upstream: PPGroups 'NetworkGroup 'NoStakePoolGroup
    let update = crate::protocol_params::ProtocolParameterUpdate {
        max_collateral_inputs: Some(3),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.network);
    assert!(!g.security);
    assert!(!g.economic);
    assert!(!g.technical);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_cost_models_is_technical_only() {
    // Upstream: PPGroups 'TechnicalGroup 'NoStakePoolGroup
    use std::collections::BTreeMap;
    let mut models = BTreeMap::new();
    models.insert(0, vec![0i64; 166]); // PlutusV1
    let update = crate::protocol_params::ProtocolParameterUpdate {
        cost_models: Some(models),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.technical);
    assert!(!g.security);
    assert!(!g.economic);
    assert!(!g.network);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_a0_is_technical_only() {
    // Upstream: PPGroups 'TechnicalGroup 'NoStakePoolGroup
    let update = crate::protocol_params::ProtocolParameterUpdate {
        a0: Some(UnitInterval {
            numerator: 3,
            denominator: 10,
        }),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.technical);
    assert!(!g.security);
    assert!(!g.economic);
    assert!(!g.network);
    assert!(!g.gov);
}

#[test]
fn pparam_groups_price_mem_is_economic_only() {
    // Upstream: PPGroups 'EconomicGroup 'NoStakePoolGroup (via Prices)
    let update = crate::protocol_params::ProtocolParameterUpdate {
        price_mem: Some(UnitInterval {
            numerator: 577,
            denominator: 10_000,
        }),
        ..Default::default()
    };
    let g = conway_modified_pparam_groups(&update);
    assert!(g.economic);
    assert!(!g.security);
    assert!(!g.network);
    assert!(!g.technical);
    assert!(!g.gov);
}

// -- Threshold dispatch ---

#[test]
fn drep_threshold_for_hard_fork() {
    let thresholds = DRepVotingThresholds::default();
    let t = drep_threshold_for_action(
        &GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        },
        true,
        &thresholds,
    );
    assert_eq!(t, Some(thresholds.hard_fork_initiation));
}

#[test]
fn drep_threshold_for_info_is_none() {
    let thresholds = DRepVotingThresholds::default();
    let t = drep_threshold_for_action(&GovAction::InfoAction, true, &thresholds);
    assert!(t.is_none());
}

#[test]
fn spo_threshold_for_constitution_is_none() {
    let thresholds = PoolVotingThresholds::default();
    let t = spo_threshold_for_action(
        &GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("c1"),
        },
        true,
        &thresholds,
    );
    assert!(t.is_none());
}

#[test]
fn spo_threshold_for_treasury_is_none() {
    let thresholds = PoolVotingThresholds::default();
    let t = spo_threshold_for_action(
        &GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        },
        true,
        &thresholds,
    );
    assert!(t.is_none());
}

#[test]
fn drep_threshold_for_no_confidence_uses_motion_threshold() {
    let thresholds = DRepVotingThresholds::default();
    let t = drep_threshold_for_action(
        &GovAction::NoConfidence {
            prev_action_id: None,
        },
        true,
        &thresholds,
    );
    assert_eq!(t, Some(thresholds.motion_no_confidence));
}

#[test]
fn spo_threshold_for_no_confidence_uses_motion_threshold() {
    let thresholds = PoolVotingThresholds::default();
    let t = spo_threshold_for_action(
        &GovAction::NoConfidence {
            prev_action_id: None,
        },
        true,
        &thresholds,
    );
    assert_eq!(t, Some(thresholds.motion_no_confidence));
}

#[test]
fn spo_threshold_for_parameter_change_requires_security_group() {
    let thresholds = PoolVotingThresholds::default();
    let non_security = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            n_opt: Some(99),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };
    let security = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            max_block_body_size: Some(123456),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };

    assert!(spo_threshold_for_action(&non_security, true, &thresholds).is_none());
    assert_eq!(
        spo_threshold_for_action(&security, true, &thresholds),
        Some(thresholds.pp_security_group)
    );
}

#[test]
fn drep_threshold_for_parameter_change_uses_max_modified_group_threshold() {
    let thresholds = DRepVotingThresholds {
        pp_network_group: UnitInterval {
            numerator: 1,
            denominator: 2,
        },
        pp_economic_group: UnitInterval {
            numerator: 2,
            denominator: 3,
        },
        pp_technical_group: UnitInterval {
            numerator: 3,
            denominator: 4,
        },
        pp_gov_group: UnitInterval {
            numerator: 4,
            denominator: 5,
        },
        ..DRepVotingThresholds::default()
    };
    let action = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            max_tx_size: Some(1024),
            n_opt: Some(42),
            gov_action_lifetime: Some(100),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };
    let selected = drep_threshold_for_action(&action, true, &thresholds);
    assert_eq!(selected, Some(thresholds.pp_gov_group));
}

#[test]
fn drep_threshold_for_security_only_parameter_change_returns_none() {
    let thresholds = DRepVotingThresholds {
        pp_network_group: UnitInterval {
            numerator: 1,
            denominator: 2,
        },
        pp_economic_group: UnitInterval {
            numerator: 2,
            denominator: 3,
        },
        pp_technical_group: UnitInterval {
            numerator: 3,
            denominator: 4,
        },
        pp_gov_group: UnitInterval {
            numerator: 4,
            denominator: 5,
        },
        ..DRepVotingThresholds::default()
    };
    // protocol_version is security-only — no DRep group, threshold should be None
    let action = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            protocol_version: Some((10, 0)),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };

    let selected = drep_threshold_for_action(&action, true, &thresholds);
    assert_eq!(selected, None);
}

#[test]
fn drep_threshold_for_single_economic_group_returns_economic_threshold() {
    let thresholds = DRepVotingThresholds {
        pp_network_group: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        pp_economic_group: UnitInterval {
            numerator: 2,
            denominator: 3,
        },
        pp_technical_group: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        pp_gov_group: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        ..DRepVotingThresholds::default()
    };
    // key_deposit is economic-only
    let action = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            key_deposit: Some(2_000_000),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };

    let selected = drep_threshold_for_action(&action, true, &thresholds);
    assert_eq!(selected, Some(thresholds.pp_economic_group));
}

#[test]
fn drep_threshold_for_update_committee_depends_on_committee_state() {
    let thresholds = DRepVotingThresholds::default();
    let action = GovAction::UpdateCommittee {
        prev_action_id: None,
        members_to_remove: vec![],
        members_to_add: BTreeMap::new(),
        quorum: UnitInterval {
            numerator: 1,
            denominator: 2,
        },
    };

    // has_committee = false → no committee seated → no-confidence threshold
    assert_eq!(
        drep_threshold_for_action(&action, false, &thresholds),
        Some(thresholds.committee_no_confidence)
    );
    // has_committee = true → committee seated → normal threshold
    assert_eq!(
        drep_threshold_for_action(&action, true, &thresholds),
        Some(thresholds.committee_normal)
    );
}

#[test]
fn spo_threshold_for_update_committee_depends_on_committee_state() {
    let thresholds = PoolVotingThresholds::default();
    let action = GovAction::UpdateCommittee {
        prev_action_id: None,
        members_to_remove: vec![],
        members_to_add: BTreeMap::new(),
        quorum: UnitInterval {
            numerator: 1,
            denominator: 2,
        },
    };

    // has_committee = false → no committee seated → no-confidence threshold
    assert_eq!(
        spo_threshold_for_action(&action, false, &thresholds),
        Some(thresholds.committee_no_confidence)
    );
    // has_committee = true → committee seated → normal threshold
    assert_eq!(
        spo_threshold_for_action(&action, true, &thresholds),
        Some(thresholds.committee_normal)
    );
}

#[test]
fn spo_voter_permission_for_parameter_change_requires_security_group() {
    let voter = Voter::StakePool([9; 28]);
    let non_security_action = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            drep_activity: Some(33),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };
    let security_action = GovAction::ParameterChange {
        prev_action_id: None,
        protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
            max_block_ex_units: Some(crate::eras::alonzo::ExUnits {
                mem: 100,
                steps: 100,
            }),
            ..Default::default()
        },
        guardrails_script_hash: None,
    };

    assert!(!conway_voter_is_allowed_for_action(
        &voter,
        &non_security_action
    ));
    assert!(conway_voter_is_allowed_for_action(&voter, &security_action));
}

// -- accepted_by_* predicates ---

#[test]
fn info_action_never_accepted_by_committee() {
    // InfoAction → NoVotingThreshold → committee never accepts.
    // Upstream: votingCommitteeThresholdInternal returns NoVotingThreshold
    // for InfoAction, which maps to SNothing → committeeAccepted = False.
    let action = test_info_action();
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(!accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        true
    ));
}

#[test]
fn no_confidence_always_passes_committee() {
    // NoConfidence → NoVotingAllowed → threshold 0 → always passes.
    // Upstream: votingCommitteeThresholdInternal returns NoVotingAllowed
    // which maps to SJust minBound → committeeAccepted = True.
    let action = test_no_confidence_action();
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        100,
        false,
        true
    ));
}

#[test]
fn no_committee_blocks_positive_threshold_action() {
    // After NoConfidence enactment, ensCommitteeL == SNothing.
    // Upstream: committeeAccepted returns False for any action that
    // requires a positive committee threshold (e.g. HardFork).
    // Reference: Cardano.Ledger.Conway.Rules.Ratify.committeeAccepted
    let action = test_hf_action();
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    // has_committee=false simulates post-NoConfidence state
    assert!(!accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        false
    ));
}

#[test]
fn no_committee_still_passes_no_confidence() {
    // Even without a committee, NoConfidence actions pass (NoVotingAllowed).
    let action = test_no_confidence_action();
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        false
    ));
}

#[test]
fn no_committee_still_passes_update_committee() {
    // UpdateCommittee still passes without a committee (NoVotingAllowed).
    let action = GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    });
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        false
    ));
}

#[test]
fn update_committee_always_passes_committee() {
    // UpdateCommittee → NoVotingAllowed → threshold 0 → always passes.
    let action = GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    });
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        100,
        false,
        true
    ));
}

#[test]
fn committee_below_min_size_rejects() {
    // Active committee < min_committee_size → rejected (not bootstrap).
    // Upstream: when activeCommitteeSize < ppCommitteeMinSizeL
    // and NOT hardforkConwayBootstrapPhase, returns NoVotingThreshold.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    // min_committee_size=2, active=1 → rejected
    assert!(!accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        2,
        false,
        true
    ));
}

#[test]
fn committee_at_min_size_accepts() {
    // Active committee == min_committee_size → accepted.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    // min_committee_size=1, active=1 → accepted
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        1,
        false,
        true
    ));
}

#[test]
fn committee_below_min_size_bootstrap_bypasses() {
    // Active committee < min_committee_size, but bootstrap phase → accepted.
    // Upstream: hardforkConwayBootstrapPhase skips minSize check.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    // min_committee_size=10, active=1, but bootstrap → accepted (1/1 >= 1/1)
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        10,
        true,
        true
    ));
}

#[test]
fn accepted_by_committee_happy_path() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold_a = StakeCredential::AddrKeyHash([1; 28]);
    let cold_b = StakeCredential::AddrKeyHash([2; 28]);
    let cold_c = StakeCredential::AddrKeyHash([3; 28]);
    let hot_a = StakeCredential::AddrKeyHash([11; 28]);
    let hot_b = StakeCredential::AddrKeyHash([12; 28]);
    let hot_c = StakeCredential::AddrKeyHash([13; 28]);
    cs.register_with_term(cold_a, 999);
    cs.register_with_term(cold_b, 999);
    cs.register_with_term(cold_c, 999);
    authorize_cc_hot(&mut cs, cold_a, hot_a);
    authorize_cc_hot(&mut cs, cold_b, hot_b);
    authorize_cc_hot(&mut cs, cold_c, hot_c);

    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([12; 28]), Vote::Yes);
    // 3 does not vote.

    let quorum = UnitInterval {
        numerator: 2,
        denominator: 3,
    };
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        true
    )); // 2/3 >= 2/3
}

#[test]
fn accepted_by_committee_rejected() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold_a = StakeCredential::AddrKeyHash([1; 28]);
    let cold_b = StakeCredential::AddrKeyHash([2; 28]);
    let cold_c = StakeCredential::AddrKeyHash([3; 28]);
    let hot_a = StakeCredential::AddrKeyHash([11; 28]);
    let hot_b = StakeCredential::AddrKeyHash([12; 28]);
    let hot_c = StakeCredential::AddrKeyHash([13; 28]);
    cs.register_with_term(cold_a, 999);
    cs.register_with_term(cold_b, 999);
    cs.register_with_term(cold_c, 999);
    authorize_cc_hot(&mut cs, cold_a, hot_a);
    authorize_cc_hot(&mut cs, cold_b, hot_b);
    authorize_cc_hot(&mut cs, cold_c, hot_c);

    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);
    // Only 1/3 yes.

    let quorum = UnitInterval {
        numerator: 2,
        denominator: 3,
    };
    assert!(!accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        true
    )); // 1/3 < 2/3
}

#[test]
fn accepted_by_dreps_treasury_action() {
    let mut action = test_treasury_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);

    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    let thresholds = DRepVotingThresholds::default();
    assert!(accepted_by_dreps(
        &action,
        true,
        &drep_state,
        &stake,
        EpochNo(5),
        100,
        &thresholds,
    )); // 100% yes >= 67%
}

// -- ratify_action combined ---

#[test]
fn ratify_info_action_never_ratified() {
    // Upstream: InfoAction → NoVotingThreshold for all three voter roles.
    // committeeAccepted = False ⇒ ratification always fails.
    // Reference: Cardano.Ledger.Conway.Rules.Ratify — InfoAction is
    // never enacted; it exists only to collect votes.
    let action = test_info_action();
    let cs = CommitteeState::default();
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    let drep_state = DrepState::new();
    let drep_stake = BTreeMap::new();
    let dvt = DRepVotingThresholds::default();
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(1),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_hf_rejected_when_dreps_insufficient() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([101; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([101; 28]), Vote::Yes);

    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    // DRep votes no.
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::No);

    let dvt = DRepVotingThresholds::default();
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let pvt = PoolVotingThresholds::default();
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_hf_accepted_when_all_roles_agree() {
    let mut action = test_hf_action();
    // CC: 1 member, votes yes.
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([101; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([101; 28]), Vote::Yes);

    // DRep: 1 drep, votes yes.
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([2; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    action.votes.insert(Voter::DRepKeyHash([2; 28]), Vote::Yes);

    // SPO: 1 pool, votes yes.
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([3u8; 28], 1000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);
    action.votes.insert(Voter::StakePool([3; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -- Protocol params threshold round-trip ---

#[test]
fn pool_voting_thresholds_cbor_round_trip() {
    let thresholds = PoolVotingThresholds::default();
    let bytes = thresholds.to_cbor_bytes();
    let decoded = PoolVotingThresholds::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(thresholds, decoded);
}

#[test]
fn drep_voting_thresholds_cbor_round_trip() {
    let thresholds = DRepVotingThresholds::default();
    let bytes = thresholds.to_cbor_bytes();
    let decoded = DRepVotingThresholds::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(thresholds, decoded);
}

#[test]
fn protocol_params_with_voting_thresholds_round_trip() {
    let mut params = ProtocolParameters::alonzo_defaults();
    params.pool_voting_thresholds = Some(PoolVotingThresholds::default());
    params.drep_voting_thresholds = Some(DRepVotingThresholds::default());
    params.min_committee_size = Some(7);
    params.committee_term_limit = Some(146);
    let bytes = params.to_cbor_bytes();
    let decoded = ProtocolParameters::from_cbor_bytes(&bytes).expect("round-trip");
    assert_eq!(params, decoded);
}

// -----------------------------------------------------------------------
// DRep inactivity boundary tests
// -----------------------------------------------------------------------

#[test]
fn drep_tally_boundary_active_when_sum_equals_current() {
    // last_active=90, drep_activity=10, current_epoch=100.
    // 90+10 = 100. Condition: 100 < 100 → false → DRep is ACTIVE.
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(90)));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(100), 10, false);
    assert_eq!(tally.total, 1000, "DRep should be active at exact boundary");
    assert_eq!(tally.yes, 1000);
}

#[test]
fn drep_tally_boundary_inactive_when_sum_less_than_current() {
    // last_active=90, drep_activity=10, current_epoch=101.
    // 90+10 = 100. Condition: 100 < 101 → true → DRep is INACTIVE.
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(90)));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(101), 10, false);
    assert_eq!(
        tally.total, 0,
        "DRep should be inactive one epoch past boundary"
    );
    assert_eq!(tally.yes, 0);
}

#[test]
fn drep_tally_no_last_active_epoch_is_active() {
    // DRep registered with no last_active_epoch (None) — should be counted.
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new(0, None));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 500);
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(999), 10, false);
    assert_eq!(
        tally.total, 500,
        "DRep with no last_active_epoch should be counted"
    );
    assert_eq!(tally.yes, 500);
}

#[test]
fn drep_tally_zero_activity_window() {
    // drep_activity=0. last_active=50, current=50. 50+0=50 < 50 → false → ACTIVE.
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(50)));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(50), 0, false);
    assert_eq!(
        tally.total, 1000,
        "DRep active when sum == current with zero window"
    );

    // current=51: 50+0=50 < 51 → true → INACTIVE.
    let tally2 = tally_drep_votes(&action, &drep_state, &stake, EpochNo(51), 0, false);
    assert_eq!(
        tally2.total, 0,
        "DRep inactive when sum < current with zero window"
    );
}

#[test]
fn drep_tally_saturating_add_no_overflow() {
    // Ensure saturating_add prevents overflow: large last_active + large activity.
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(
        drep,
        RegisteredDrep::new_active(0, None, EpochNo(u64::MAX - 5)),
    );

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    // (u64::MAX - 5) + 100 would overflow, saturates to u64::MAX.
    // u64::MAX < u64::MAX is false → DRep is ACTIVE.
    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(u64::MAX), 100, false);
    assert_eq!(tally.total, 1000, "saturating_add should prevent overflow");
}

// -----------------------------------------------------------------------
// DRep tally: AlwaysAbstain and AlwaysNoConfidence special DReps
// -----------------------------------------------------------------------

#[test]
fn drep_tally_always_abstain_excluded_from_active_vote() {
    // Stake delegated to AlwaysAbstain is not counted at all.
    let action = test_hf_action();
    let drep_state = DrepState::new();
    let mut stake = BTreeMap::new();
    stake.insert(DRep::AlwaysAbstain, 5000);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.total, 0, "AlwaysAbstain stake not counted");
}

#[test]
fn drep_tally_always_no_confidence_in_total_not_yes() {
    // AlwaysNoConfidence stake is included in total but NOT counted as
    // "Yes" for non-NoConfidence actions.
    let action = test_hf_action();
    let drep_state = DrepState::new();
    let mut stake = BTreeMap::new();
    stake.insert(DRep::AlwaysNoConfidence, 5000);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(
        tally.total, 5000,
        "AlwaysNoConfidence stake included in total"
    );
    assert_eq!(
        tally.yes, 0,
        "Not counted as Yes for non-NoConfidence action"
    );
}

#[test]
fn drep_tally_non_voting_drep_counted_in_total() {
    // A registered active DRep who does NOT vote is still in the total
    // (their stake counts against the denominator).
    let action = test_hf_action(); // no DRep votes
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.total, 1000, "non-voting DRep stake in total");
    assert_eq!(tally.yes, 0);
    assert_eq!(tally.no, 0);
    assert_eq!(tally.abstain, 0);
}

#[test]
fn drep_tally_abstain_vote_counted_as_abstain() {
    let mut action = test_hf_action();
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut stake = BTreeMap::new();
    stake.insert(drep, 1000);
    action
        .votes
        .insert(Voter::DRepKeyHash([1; 28]), Vote::Abstain);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.abstain, 1000);
    assert_eq!(tally.total, 1000);
    // All abstain → active = 0 → upstream `%?` returns 0 → fails
    // any positive threshold.
    let threshold = UnitInterval {
        numerator: 99,
        denominator: 100,
    };
    assert!(!tally.meets_threshold(&threshold));
}

// -----------------------------------------------------------------------
// AlwaysNoConfidence auto-yes for NoConfidence actions
// -----------------------------------------------------------------------

#[test]
fn drep_tally_always_no_confidence_auto_yes_for_no_confidence_action() {
    // AlwaysNoConfidence stake should count as auto-Yes for NoConfidence.
    let action = test_no_confidence_action();
    let drep_state = DrepState::new();
    let mut stake = BTreeMap::new();
    stake.insert(DRep::AlwaysNoConfidence, 5000);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
    assert_eq!(
        tally.total, 5000,
        "AlwaysNoConfidence stake included in total"
    );
    assert_eq!(tally.yes, 5000, "AlwaysNoConfidence stake counted as Yes");
}

#[test]
fn drep_tally_always_no_confidence_not_yes_for_other_actions() {
    // For non-NoConfidence actions, AlwaysNoConfidence is in total but NOT Yes.
    let action = test_hf_action();
    let drep_state = DrepState::new();
    let mut stake = BTreeMap::new();
    stake.insert(DRep::AlwaysNoConfidence, 3000);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, false);
    assert_eq!(tally.total, 3000);
    assert_eq!(tally.yes, 0, "Not auto-yes for non-NoConfidence action");
}

#[test]
fn drep_tally_always_no_confidence_mixed_with_regular_dreps() {
    // AlwaysNoConfidence + registered DReps together.
    let mut action = test_no_confidence_action();
    let mut drep_state = DrepState::new();
    let drep_a = DRep::KeyHash([1; 28]);
    drep_state.register(drep_a, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut stake = BTreeMap::new();
    stake.insert(DRep::AlwaysNoConfidence, 4000);
    stake.insert(drep_a, 6000);

    // DRep A votes No; AlwaysNoConfidence auto-yes.
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::No);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
    assert_eq!(tally.total, 10000);
    assert_eq!(tally.yes, 4000, "auto-yes from AlwaysNoConfidence");
    assert_eq!(tally.no, 6000, "explicit No from DRep A");

    // 4000/10000 = 40% vs threshold 67% → does NOT pass.
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn drep_tally_always_no_confidence_pushes_no_confidence_past_threshold() {
    // AlwaysNoConfidence stake tips the balance for a NoConfidence action.
    let mut action = test_no_confidence_action();
    let mut drep_state = DrepState::new();
    let drep_a = DRep::KeyHash([1; 28]);
    drep_state.register(drep_a, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut stake = BTreeMap::new();
    stake.insert(DRep::AlwaysNoConfidence, 5000);
    stake.insert(drep_a, 5000);

    // DRep A votes Yes; AlwaysNoConfidence also auto-yes → 10000/10000 = 100%.
    action.votes.insert(Voter::DRepKeyHash([1; 28]), Vote::Yes);

    let tally = tally_drep_votes(&action, &drep_state, &stake, EpochNo(5), 100, true);
    assert_eq!(tally.yes, 10000);
    assert_eq!(tally.total, 10000);
    let threshold = UnitInterval {
        numerator: 67,
        denominator: 100,
    };
    assert!(tally.meets_threshold(&threshold));
}

// -----------------------------------------------------------------------
// SPO tally edge cases
// -----------------------------------------------------------------------

#[test]
fn spo_tally_empty_pool_distribution_fails_positive_threshold() {
    let action = test_hf_action();
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let tally = tally_spo_votes(
        &action,
        &pool_dist,
        false,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally.total, 0);
    // Zero total → active = 0 → upstream `%?` returns 0 → fails
    // any positive threshold.
    let threshold = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn spo_tally_non_voting_pool_in_total() {
    let action = test_hf_action(); // no SPO votes
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([1u8; 28], 2000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 2000);

    let tally = tally_spo_votes(
        &action,
        &pool_dist,
        false,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally.total, 2000);
    assert_eq!(tally.yes, 0);
    // Non-voting pool means 0 yes out of 2000 → does NOT meet 51%.
    let threshold = UnitInterval {
        numerator: 51,
        denominator: 100,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn spo_tally_abstain_vote() {
    let mut action = test_hf_action();
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([1u8; 28], 1000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);
    action
        .votes
        .insert(Voter::StakePool([1; 28]), Vote::Abstain);

    let tally = tally_spo_votes(
        &action,
        &pool_dist,
        false,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally.abstain, 1000);
    assert_eq!(tally.total, 1000);
    // All abstain → active = 0 → fails positive threshold.
    let threshold = UnitInterval {
        numerator: 99,
        denominator: 100,
    };
    assert!(!tally.meets_threshold(&threshold));
}

// -----------------------------------------------------------------------
// Committee tally edge cases
// -----------------------------------------------------------------------

#[test]
fn committee_tally_empty_committee_fails_positive_threshold() {
    let action = test_hf_action();
    let cs = CommitteeState::default();

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(tally.total, 0);
    // Empty committee → active = 0 → upstream `%?` returns 0 → fails
    // any positive threshold.
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(!tally.meets_threshold(&quorum));
}

#[test]
fn committee_tally_all_resigned_is_vacuous() {
    let action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cred = StakeCredential::AddrKeyHash([1; 28]);
    cs.register_with_term(cred, 999);
    cs.get_mut(&cred)
        .unwrap()
        .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(
        tally.total, 0,
        "all-resigned committee has zero eligible members"
    );
}

#[test]
fn committee_tally_single_member_exact_quorum() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(tally.yes, 1);
    assert_eq!(tally.total, 1);
    // 1/1 >= 100% quorum.
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(tally.meets_threshold(&quorum));
}

#[test]
fn committee_member_votes_no() {
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::No);

    let tally = tally_committee_votes(&action, &cs, EpochNo(0));
    assert_eq!(tally.no, 1);
    assert_eq!(tally.yes, 0);
    assert_eq!(tally.total, 1);
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };
    assert!(!tally.meets_threshold(&quorum));
}

// -----------------------------------------------------------------------
// Committee tally: expired-member term filtering
// -----------------------------------------------------------------------

#[test]
fn committee_tally_expired_member_excluded() {
    // Member expires at epoch 10; tallied at epoch 11 → expired.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 10);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(11));
    assert_eq!(tally.total, 0, "expired member excluded from eligible");
    assert_eq!(tally.yes, 0, "expired member's vote not counted");
}

#[test]
fn committee_tally_member_active_at_expiry_boundary() {
    // Member expires at epoch 10; tallied at epoch 10 → still active.
    // Upstream: `currentEpoch <= expirationEpoch`.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 10);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(10));
    assert_eq!(tally.total, 1, "member active at boundary epoch");
    assert_eq!(tally.yes, 1);
}

#[test]
fn committee_tally_mix_expired_and_active() {
    // Two members: one expired, one active. Only active one counts.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold_expired = StakeCredential::AddrKeyHash([1; 28]);
    let cold_active = StakeCredential::AddrKeyHash([2; 28]);
    let hot_expired = StakeCredential::AddrKeyHash([11; 28]);
    let hot_active = StakeCredential::AddrKeyHash([12; 28]);
    cs.register_with_term(cold_expired, 5);
    cs.register_with_term(cold_active, 100);
    authorize_cc_hot(&mut cs, cold_expired, hot_expired);
    authorize_cc_hot(&mut cs, cold_active, hot_active);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([12; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(10));
    assert_eq!(tally.total, 1, "only active member in eligible");
    assert_eq!(tally.yes, 1);
}

#[test]
fn committee_tally_no_term_means_never_expires() {
    // Members registered with a far-future term are always active.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([1; 28]);
    let hot = StakeCredential::AddrKeyHash([11; 28]);
    cs.register_with_term(cold, 1_000_000);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([11; 28]), Vote::Yes);

    let tally = tally_committee_votes(&action, &cs, EpochNo(999_999));
    assert_eq!(tally.total, 1, "far-future-term member still active");
    assert_eq!(tally.yes, 1);
}

#[test]
fn accepted_by_committee_expired_members_affect_quorum() {
    // 3 members, 2 expired. Only 1 active and votes yes → 1/1 >= 2/3.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold_a = StakeCredential::AddrKeyHash([1; 28]);
    let cold_b = StakeCredential::AddrKeyHash([2; 28]);
    let cold_c = StakeCredential::AddrKeyHash([3; 28]);
    let hot_a = StakeCredential::AddrKeyHash([11; 28]);
    let hot_b = StakeCredential::AddrKeyHash([12; 28]);
    let hot_c = StakeCredential::AddrKeyHash([13; 28]);
    cs.register_with_term(cold_a, 5);
    cs.register_with_term(cold_b, 5);
    cs.register_with_term(cold_c, 100);
    authorize_cc_hot(&mut cs, cold_a, hot_a);
    authorize_cc_hot(&mut cs, cold_b, hot_b);
    authorize_cc_hot(&mut cs, cold_c, hot_c);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([13; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 2,
        denominator: 3,
    };
    assert!(
        accepted_by_committee(&action, &cs, &quorum, EpochNo(10), 0, false, true),
        "expired members reduce eligible count, so 1/1 >= 2/3"
    );
}

#[test]
fn committee_all_expired_fails_positive_threshold() {
    // All members expired → total=0 → fails positive threshold.
    let action = test_hf_action();
    let mut cs = CommitteeState::default();
    cs.register_with_term(StakeCredential::AddrKeyHash([1; 28]), 1);
    cs.register_with_term(StakeCredential::AddrKeyHash([2; 28]), 1);

    let tally = tally_committee_votes(&action, &cs, EpochNo(10));
    assert_eq!(tally.total, 0, "all expired");
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(!tally.meets_threshold(&quorum));
}

// -----------------------------------------------------------------------
// accepted_by_spo: actions that don't require SPO votes
// -----------------------------------------------------------------------

#[test]
fn accepted_by_spo_treasury_always_true() {
    // TreasuryWithdrawals doesn't require SPO vote → always accepted.
    let action = test_treasury_action();
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let pvt = PoolVotingThresholds::default();
    assert!(accepted_by_spo(
        &action,
        true,
        &pool_dist,
        &pvt,
        false,
        &PoolState::new(),
        &StakeCredentials::new()
    ));
}

#[test]
fn accepted_by_spo_new_constitution_always_true() {
    let action = GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("test"),
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    });
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let pvt = PoolVotingThresholds::default();
    assert!(accepted_by_spo(
        &action,
        true,
        &pool_dist,
        &pvt,
        false,
        &PoolState::new(),
        &StakeCredentials::new()
    ));
}

#[test]
fn accepted_by_dreps_info_always_true() {
    // InfoAction has no DRep threshold → always accepted.
    let action = test_info_action();
    let drep_state = DrepState::new();
    let drep_stake = BTreeMap::new();
    let dvt = DRepVotingThresholds::default();
    assert!(accepted_by_dreps(
        &action,
        true,
        &drep_state,
        &drep_stake,
        EpochNo(1),
        100,
        &dvt
    ));
}

// -----------------------------------------------------------------------
// Ratification: NoConfidence (CC + DRep + SPO all required)
// -----------------------------------------------------------------------

fn test_param_change_security_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                max_block_body_size: Some(65536), // network+security group
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

fn test_param_change_economic_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(2_000_000), // economic group only
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

fn test_update_committee_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

fn test_new_constitution_action() -> GovernanceActionState {
    GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
        deposit: 0,
        reward_account: vec![],
        gov_action: GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("ratify-test"),
        },
        anchor: crate::types::Anchor {
            url: String::new(),
            data_hash: [0; 32],
        },
    })
}

/// Helper: minimal committee with one member who votes yes.
fn setup_cc_one_yes(action: &mut GovernanceActionState) -> (CommitteeState, UnitInterval) {
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([0xCC; 28]);
    let hot = StakeCredential::AddrKeyHash([0xDC; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([0xDC; 28]), Vote::Yes);
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };
    (cs, quorum)
}

/// Helper: one DRep with given stake who votes yes.
fn setup_drep_one_yes(
    action: &mut GovernanceActionState,
    drep_id: u8,
    stake_amount: u64,
) -> (DrepState, BTreeMap<DRep, u64>) {
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([drep_id; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut stake = BTreeMap::new();
    stake.insert(drep, stake_amount);
    action
        .votes
        .insert(Voter::DRepKeyHash([drep_id; 28]), Vote::Yes);
    (drep_state, stake)
}

/// Helper: one pool with given stake that votes yes.
fn setup_spo_one_yes(
    action: &mut GovernanceActionState,
    pool_id: u8,
    pool_stake: u64,
) -> crate::stake::PoolStakeDistribution {
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([pool_id; 28], pool_stake);
    action
        .votes
        .insert(Voter::StakePool([pool_id; 28]), Vote::Yes);
    crate::stake::PoolStakeDistribution::from_raw(pool_stakes, pool_stake)
}

#[test]
fn ratify_no_confidence_accepted_when_all_agree() {
    let mut action = test_no_confidence_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_no_confidence_rejected_when_dreps_vote_no() {
    let mut action = test_no_confidence_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    // DRep votes no
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([0xD1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_no_confidence_rejected_when_spo_vote_no() {
    let mut action = test_no_confidence_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    // SPO votes no
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([0xA1; 28], 1000u64);
    action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::No);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_no_confidence_passes_despite_committee_no_vote() {
    // Upstream: NoConfidence → NoVotingAllowed for committee.
    // Committee vote is irrelevant. DRep + SPO must still meet thresholds.
    let mut action = test_no_confidence_action();
    // CC member votes no — but committee is bypassed for NoConfidence.
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([0xCC; 28]);
    let hot = StakeCredential::AddrKeyHash([0xDC; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([0xDC; 28]), Vote::No);
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };

    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// Ratification: ParameterChange
// -----------------------------------------------------------------------

#[test]
fn ratify_param_change_security_accepted() {
    // Security-group change: requires CC + DRep + SPO.
    let mut action = test_param_change_security_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_param_change_security_rejected_without_spo() {
    // Security-group change requires SPO. If SPO votes no → rejected.
    let mut action = test_param_change_security_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    // SPO votes no.
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([0xA1; 28], 1000u64);
    action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::No);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_param_change_economic_no_spo_needed() {
    // Economic-only change: CC + DRep required, SPO NOT required.
    let mut action = test_param_change_economic_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    // No SPO votes, empty pool dist — should still pass.
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_param_change_rejected_when_dreps_insufficient() {
    let mut action = test_param_change_economic_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    // DRep votes no.
    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([0xD1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// Ratification: TreasuryWithdrawals (CC + DRep, no SPO)
// -----------------------------------------------------------------------

#[test]
fn ratify_treasury_accepted_cc_and_drep() {
    let mut action = test_treasury_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    // No SPO needed for treasury.
    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_treasury_rejected_when_dreps_vote_no() {
    let mut action = test_treasury_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([0xD1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_treasury_rejected_when_committee_fails() {
    let mut action = test_treasury_action();
    // CC votes no.
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([0xCC; 28]);
    let hot = StakeCredential::AddrKeyHash([0xDC; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);
    action
        .votes
        .insert(Voter::CommitteeKeyHash([0xDC; 28]), Vote::No);
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 2,
    };

    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// Ratification: NewConstitution (CC + DRep, no SPO)
// -----------------------------------------------------------------------

#[test]
fn ratify_new_constitution_accepted() {
    let mut action = test_new_constitution_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_new_constitution_rejected_when_dreps_vote_no() {
    let mut action = test_new_constitution_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([0xD1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);

    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// Ratification: UpdateCommittee (DRep + SPO, CC not required for
// committee changes — actually CC IS required per accepted_by_committee)
// -----------------------------------------------------------------------

#[test]
fn ratify_update_committee_accepted_all_agree() {
    let mut action = test_update_committee_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_update_committee_rejected_when_spo_votes_no() {
    let mut action = test_update_committee_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    // SPO votes no.
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([0xA1; 28], 1000u64);
    action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::No);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// Ratification: DRep inactivity affects ratification outcome
// -----------------------------------------------------------------------

#[test]
fn ratify_hf_rejected_when_only_drep_is_inactive() {
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    // DRep registered at epoch 10, activity window 10, current epoch 25.
    // 10 + 10 = 20 < 25 → inactive. No active DReps = vacuous → passes.
    // BUT: let's add a second DRep that is active and votes No.
    let mut drep_state = DrepState::new();
    let drep_inactive = DRep::KeyHash([0xD1; 28]);
    drep_state.register(
        drep_inactive,
        RegisteredDrep::new_active(0, None, EpochNo(10)),
    );
    let drep_active = DRep::KeyHash([0xD2; 28]);
    drep_state.register(
        drep_active,
        RegisteredDrep::new_active(0, None, EpochNo(20)),
    );

    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep_inactive, 1000);
    drep_stake.insert(drep_active, 1000);

    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes); // inactive, excluded
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD2; 28]), Vote::No); // active, counted

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    // Only active DRep voted No → 0/1000 yes → fails DRep threshold.
    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(25),
        10,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_hf_accepted_when_inactive_dreps_excluded_and_active_vote_yes() {
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    // Inactive DRep with large NO stake is excluded; active DRep votes yes.
    let mut drep_state = DrepState::new();
    let drep_inactive = DRep::KeyHash([0xD1; 28]);
    drep_state.register(
        drep_inactive,
        RegisteredDrep::new_active(0, None, EpochNo(10)),
    );
    let drep_active = DRep::KeyHash([0xD2; 28]);
    drep_state.register(
        drep_active,
        RegisteredDrep::new_active(0, None, EpochNo(20)),
    );

    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep_inactive, 9000);
    drep_stake.insert(drep_active, 1000);

    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No); // inactive, excluded
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD2; 28]), Vote::Yes);

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    // Active DRep: 1000 yes / 1000 total = 100% >= 67%. Passes.
    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(25),
        10,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// Ratification: Multi-voter edge cases
// -----------------------------------------------------------------------

#[test]
fn ratify_hf_rejected_partial_drep_support() {
    // Two DReps: 40% yes, 60% no → fails 67% threshold.
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    let mut drep_state = DrepState::new();
    let drep_a = DRep::KeyHash([0xD1; 28]);
    let drep_b = DRep::KeyHash([0xD2; 28]);
    drep_state.register(drep_a, RegisteredDrep::new_active(0, None, EpochNo(1)));
    drep_state.register(drep_b, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep_a, 400);
    drep_stake.insert(drep_b, 600);

    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD2; 28]), Vote::No);

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    // 400/1000 = 40% < 67% → fails.
    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_hf_accepted_with_abstentions_raising_effective_ratio() {
    // One DRep yes (500), one DRep abstain (500). Active = 500.
    // 500/500 = 100% >= 67%.
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    let mut drep_state = DrepState::new();
    let drep_a = DRep::KeyHash([0xD1; 28]);
    let drep_b = DRep::KeyHash([0xD2; 28]);
    drep_state.register(drep_a, RegisteredDrep::new_active(0, None, EpochNo(1)));
    drep_state.register(drep_b, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep_a, 500);
    drep_stake.insert(drep_b, 500);

    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD2; 28]), Vote::Abstain);

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_empty_committee_fails_positive_quorum() {
    // No CC members → accepted_by_committee fails (positive quorum
    // threshold with zero active members → upstream `%?` returns 0).
    let mut action = test_hf_action();
    let cs = CommitteeState::default(); // empty
    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);
    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_all_dreps_abstain_fails_positive_threshold() {
    // All DReps abstain → active = 0 → fails positive DRep threshold.
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([0xD1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));
    let mut drep_stake = BTreeMap::new();
    drep_stake.insert(drep, 1000);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Abstain);

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_no_dreps_registered_fails_positive_threshold() {
    // No registered DReps → total=0 → active=0 → fails positive
    // threshold.
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);

    let drep_state = DrepState::new();
    let drep_stake = BTreeMap::new();

    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_no_pools_registered_fails_positive_threshold() {
    // HF requires SPO vote. No pools → total=0 → active=0 → fails
    // positive threshold.
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let (drep_state, drep_stake) = setup_drep_one_yes(&mut action, 0xD1, 1000);

    let pool_dist = crate::stake::PoolStakeDistribution::default();
    let dvt = DRepVotingThresholds::default();
    let pvt = PoolVotingThresholds::default();

    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

#[test]
fn ratify_bootstrap_drep_thresholds_zeroed() {
    // During Conway bootstrap phase (PV 9), upstream zeros all DRep
    // thresholds so any non-InfoAction passes the DRep gate.
    // Reference: votingDRepThresholdInternal uses `def` (= minBound).
    //
    // Here: a HardFork action with NO DRep votes at all. With real
    // thresholds (67/100) it would fail. With bootstrap zeroing it passes
    // the DRep gate (0/0 >= 0 via minBound short-circuit).
    let mut action = test_hf_action();
    let (cs, quorum) = setup_cc_one_yes(&mut action);
    let pool_dist = setup_spo_one_yes(&mut action, 0xA1, 1000);

    // NO drep votes — empty DRep state.
    let drep_state = DrepState::new();
    let drep_stake = BTreeMap::new();
    let dvt = DRepVotingThresholds::default(); // real thresholds (67/100 etc)
    let pvt = PoolVotingThresholds::default();

    // Without bootstrap: fails because 0/0 doesn't meet 67/100.
    assert!(!ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        false,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));

    // With bootstrap (is_bootstrap_phase=true): DRep thresholds zeroed → passes.
    assert!(ratify_action(
        &action,
        &cs,
        &quorum,
        &drep_state,
        &drep_stake,
        EpochNo(5),
        100,
        &dvt,
        &pool_dist,
        &pvt,
        0,
        true,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    ));
}

// -----------------------------------------------------------------------
// defaultStakePoolVote — post-bootstrap SPO default vote from DRep delegation
// Reference: Cardano.Ledger.Conway.Governance.defaultStakePoolVote
// -----------------------------------------------------------------------

#[test]
fn default_spo_vote_always_no_confidence_counts_yes_on_no_confidence() {
    // Pool whose reward account delegates to AlwaysNoConfidence.
    // Post-bootstrap, non-voting pool should count as Yes on NoConfidence.
    let pool_hash = [0xB1; 28];
    let reward_cred = StakeCredential::AddrKeyHash([0xC1; 28]);
    let reward_account = crate::types::RewardAccount {
        network: 0,
        credential: reward_cred,
    };
    let params = crate::types::PoolParams {
        operator: pool_hash,
        vrf_keyhash: [0; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account,
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    };
    let mut pool_state = PoolState::new();
    pool_state.register(params);
    let mut stake_creds = StakeCredentials::new();
    stake_creds.register(reward_cred);
    stake_creds
        .get_mut(&reward_cred)
        .unwrap()
        .set_delegated_drep(Some(DRep::AlwaysNoConfidence));

    let action = test_no_confidence_action();
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert(pool_hash, 1000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    // Post-bootstrap (is_bootstrap_phase=false): default vote = NoConfidence → Yes.
    let tally = tally_spo_votes(&action, &pool_dist, false, &pool_state, &stake_creds);
    assert_eq!(
        tally.yes, 1000,
        "AlwaysNoConfidence should auto-yes on NoConfidence"
    );
    assert_eq!(tally.abstain, 0);
}

#[test]
fn default_spo_vote_always_no_confidence_no_effect_on_hard_fork() {
    // AlwaysNoConfidence delegation has no effect on HardFork proposals —
    // non-voting is always implicit No for HardFork.
    let pool_hash = [0xB2; 28];
    let reward_cred = StakeCredential::AddrKeyHash([0xC2; 28]);
    let reward_account = crate::types::RewardAccount {
        network: 0,
        credential: reward_cred,
    };
    let params = crate::types::PoolParams {
        operator: pool_hash,
        vrf_keyhash: [0; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account,
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    };
    let mut pool_state = PoolState::new();
    pool_state.register(params);
    let mut stake_creds = StakeCredentials::new();
    stake_creds.register(reward_cred);
    stake_creds
        .get_mut(&reward_cred)
        .unwrap()
        .set_delegated_drep(Some(DRep::AlwaysNoConfidence));

    let action = test_hf_action();
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert(pool_hash, 1000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let tally = tally_spo_votes(&action, &pool_dist, false, &pool_state, &stake_creds);
    assert_eq!(tally.yes, 0, "HardFork non-voting is always implicit No");
    assert_eq!(tally.abstain, 0);
}

#[test]
fn default_spo_vote_always_abstain_excludes_from_denominator() {
    // Pool whose reward account delegates to AlwaysAbstain.
    // Post-bootstrap, non-voting pool should count as Abstain (excluded from active).
    let pool_hash = [0xB3; 28];
    let reward_cred = StakeCredential::AddrKeyHash([0xC3; 28]);
    let reward_account = crate::types::RewardAccount {
        network: 0,
        credential: reward_cred,
    };
    let params = crate::types::PoolParams {
        operator: pool_hash,
        vrf_keyhash: [0; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account,
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    };
    let mut pool_state = PoolState::new();
    pool_state.register(params);
    let mut stake_creds = StakeCredentials::new();
    stake_creds.register(reward_cred);
    stake_creds
        .get_mut(&reward_cred)
        .unwrap()
        .set_delegated_drep(Some(DRep::AlwaysAbstain));

    let action = test_no_confidence_action();
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert(pool_hash, 1000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let tally = tally_spo_votes(&action, &pool_dist, false, &pool_state, &stake_creds);
    assert_eq!(tally.abstain, 1000, "AlwaysAbstain should count as Abstain");
    assert_eq!(tally.yes, 0);
}

#[test]
fn default_spo_vote_no_confidence_implicit_no_on_non_no_confidence_action() {
    // AlwaysNoConfidence delegation: on ParameterChange (not NoConfidence),
    // the default vote is just No (counted in total, not in yes/abstain).
    let pool_hash = [0xB4; 28];
    let reward_cred = StakeCredential::AddrKeyHash([0xC4; 28]);
    let reward_account = crate::types::RewardAccount {
        network: 0,
        credential: reward_cred,
    };
    let params = crate::types::PoolParams {
        operator: pool_hash,
        vrf_keyhash: [0; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account,
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    };
    let mut pool_state = PoolState::new();
    pool_state.register(params);
    let mut stake_creds = StakeCredentials::new();
    stake_creds.register(reward_cred);
    stake_creds
        .get_mut(&reward_cred)
        .unwrap()
        .set_delegated_drep(Some(DRep::AlwaysNoConfidence));

    // Security-group parameter change (SPOs can vote on these)
    let action = test_param_change_security_action();
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert(pool_hash, 1000u64);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let tally = tally_spo_votes(&action, &pool_dist, false, &pool_state, &stake_creds);
    assert_eq!(
        tally.yes, 0,
        "NoConfidence default should not auto-yes on ParameterChange"
    );
    assert_eq!(
        tally.abstain, 0,
        "NoConfidence default should not count as abstain"
    );
    assert_eq!(tally.total, 1000, "Stake still in total");
}

// -----------------------------------------------------------------------
// SPO bootstrap abstain + committee threshold selection
// -----------------------------------------------------------------------

#[test]
fn spo_tally_bootstrap_non_voting_counts_as_abstain() {
    // Upstream spoAcceptedRatio: during bootstrap, non-voting SPOs count
    // as Abstain (excluded from denominator), not implicit No.
    let mut action = test_no_confidence_action();
    // One pool votes yes, one pool does NOT vote.
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([0xA1; 28], 500);
    pool_stakes.insert([0xA2; 28], 500);
    action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::Yes);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    // Without bootstrap: non-voting A2 is in denominator → yes=500/(1000-0) = 50%.
    let tally_normal = tally_spo_votes(
        &action,
        &pool_dist,
        false,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally_normal.yes, 500);
    assert_eq!(tally_normal.abstain, 0);
    assert_eq!(tally_normal.total, 1000);

    // With bootstrap: non-voting A2 counts as abstain → yes=500/(1000-500) = 100%.
    let tally_bootstrap = tally_spo_votes(
        &action,
        &pool_dist,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally_bootstrap.yes, 500);
    assert_eq!(tally_bootstrap.abstain, 500);
    assert_eq!(tally_bootstrap.total, 1000);
    // Effective ratio: 500 / (1000-500) = 100%.
    assert!(tally_bootstrap.meets_threshold(&UnitInterval {
        numerator: 1,
        denominator: 1
    }));
}

#[test]
fn spo_tally_bootstrap_hard_fork_non_voting_still_no() {
    // Upstream: HardForkInitiation non-voting SPOs are always No,
    // even during bootstrap.
    let mut action = test_hf_action();
    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert([0xA1; 28], 500);
    pool_stakes.insert([0xA2; 28], 500); // doesn't vote
    action.votes.insert(Voter::StakePool([0xA1; 28]), Vote::Yes);
    let pool_dist = crate::stake::PoolStakeDistribution::from_raw(pool_stakes, 1000);

    let tally = tally_spo_votes(
        &action,
        &pool_dist,
        true,
        &PoolState::new(),
        &StakeCredentials::new(),
    );
    assert_eq!(tally.yes, 500);
    assert_eq!(tally.abstain, 0); // NOT counted as abstain for HF
    assert_eq!(tally.total, 1000);
}

#[test]
fn threshold_selection_uses_has_committee_not_member_state() {
    // Upstream uses `isSJust ensCommitteeL` for threshold selection,
    // NOT whether committee members are actively serving.
    // has_committee=true → committee_normal threshold, even if all resigned.
    // has_committee=false → committee_no_confidence threshold.
    let thresholds = DRepVotingThresholds::default();

    let uc_action = GovAction::UpdateCommittee {
        prev_action_id: None,
        members_to_remove: vec![],
        members_to_add: BTreeMap::new(),
        quorum: UnitInterval {
            numerator: 1,
            denominator: 2,
        },
    };

    // has_committee=true → normal threshold
    let t_normal = drep_threshold_for_action(&uc_action, true, &thresholds);
    assert_eq!(t_normal, Some(thresholds.committee_normal));

    // has_committee=false → no-confidence threshold
    let t_no_conf = drep_threshold_for_action(&uc_action, false, &thresholds);
    assert_eq!(t_no_conf, Some(thresholds.committee_no_confidence));
}

// -----------------------------------------------------------------------
// VoteTally threshold edge cases
// -----------------------------------------------------------------------

#[test]
fn tally_fractional_threshold_cross_multiply() {
    // Verify cross-multiplication works for non-trivial fractions.
    // 3 yes out of 7 active. Threshold 2/5. 3*5 = 15 >= 2*7 = 14 → passes.
    let tally = VoteTally {
        yes: 3,
        no: 4,
        abstain: 0,
        total: 7,
    };
    let threshold = UnitInterval {
        numerator: 2,
        denominator: 5,
    };
    assert!(tally.meets_threshold(&threshold));
}

#[test]
fn tally_fractional_threshold_just_below() {
    // 2 yes out of 7 active. Threshold 2/5. 2*5 = 10 < 2*7 = 14 → fails.
    let tally = VoteTally {
        yes: 2,
        no: 5,
        abstain: 0,
        total: 7,
    };
    let threshold = UnitInterval {
        numerator: 2,
        denominator: 5,
    };
    assert!(!tally.meets_threshold(&threshold));
}

#[test]
fn tally_100_percent_threshold_requires_unanimity() {
    let tally = VoteTally {
        yes: 99,
        no: 1,
        abstain: 0,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(!tally.meets_threshold(&threshold));

    let tally_unanimous = VoteTally {
        yes: 100,
        no: 0,
        abstain: 0,
        total: 100,
    };
    assert!(tally_unanimous.meets_threshold(&threshold));
}

#[test]
fn tally_zero_numerator_threshold_always_passes() {
    // 0% threshold → 0 yes suffices.
    let tally = VoteTally {
        yes: 0,
        no: 100,
        abstain: 0,
        total: 100,
    };
    let threshold = UnitInterval {
        numerator: 0,
        denominator: 1,
    };
    assert!(tally.meets_threshold(&threshold));
}

// -----------------------------------------------------------------------
// Proposal validation: ParameterChange edge cases
// -----------------------------------------------------------------------

#[test]
fn proposal_rejects_empty_parameter_change() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate::default(),
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

#[test]
fn proposal_rejects_zero_drep_deposit() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                drep_deposit: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

/// Upstream ppuWellFormed does NOT reject `min_committee_size == 0`.
#[test]
fn proposal_accepts_zero_min_committee_size() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_committee_size: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "min_committee_size=0 is accepted (upstream parity)"
    );
}

#[test]
fn proposal_rejects_zero_gov_action_lifetime() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                gov_action_lifetime: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

/// Upstream ppuWellFormed does NOT reject `drep_activity == 0`.
#[test]
fn proposal_accepts_zero_drep_activity() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                drep_activity: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "drep_activity=0 is accepted (upstream parity)"
    );
}

#[test]
fn proposal_rejects_zero_committee_term_limit() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                committee_term_limit: Some(0),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

#[test]
fn proposal_rejects_malformed_pool_voting_thresholds() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                pool_voting_thresholds: Some(crate::protocol_params::PoolVotingThresholds {
                    // numerator > denominator → invalid
                    motion_no_confidence: UnitInterval {
                        numerator: 3,
                        denominator: 2,
                    },
                    ..crate::protocol_params::PoolVotingThresholds::default()
                }),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

#[test]
fn proposal_rejects_malformed_drep_voting_thresholds() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                drep_voting_thresholds: Some(DRepVotingThresholds {
                    // zero denominator → invalid
                    treasury_withdrawal: UnitInterval {
                        numerator: 0,
                        denominator: 0,
                    },
                    ..DRepVotingThresholds::default()
                }),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(result, Err(LedgerError::MalformedProposal(_))));
}

#[test]
fn proposal_accepts_valid_parameter_change() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(2_000_000),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

// -----------------------------------------------------------------------
// Proposal validation: deposit and reward account checks
// -----------------------------------------------------------------------

#[test]
fn proposal_rejects_incorrect_deposit() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(GovAction::InfoAction, 500, 1)];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        Some(1000), // expected deposit = 1000
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::ProposalDepositIncorrect {
            supplied: 500,
            expected: 1000
        })
    ));
}

#[test]
fn proposal_rejects_unregistered_return_account() {
    let es = EnactState::default();
    // Return account for ra_id=1 but only register ra_id=2.
    let stake_creds = empty_stake_creds_with(2);
    let proposals = vec![sample_proposal(GovAction::InfoAction, 1, 1)];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::ProposalReturnAccountDoesNotExist(_))
    ));
}

#[test]
fn proposal_bootstrap_allows_unregistered_return_account() {
    // During Conway bootstrap (PV 9), ProposalReturnAccountDoesNotExist
    // is NOT enforced.  Only checked post-bootstrap (PV ≥ 10).
    // Reference: Cardano.Ledger.Conway.Rules.Gov — conwayGovTransition
    //   `unless (hardforkConwayBootstrapPhase ...) $ do ...`
    let es = EnactState::default();
    // Return account ra_id=1 is NOT registered — only register ra_id=2.
    let stake_creds = empty_stake_creds_with(2);
    let proposals = vec![sample_proposal(GovAction::InfoAction, 1, 1)];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((9, 0)), // bootstrap phase
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "bootstrap should skip return-account registration check: {result:?}"
    );
}

#[test]
fn proposal_bootstrap_allows_parameter_change_unregistered_return_account() {
    // ParameterChange is a bootstrap action; during bootstrap the return-
    // account registration check is skipped.
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(2); // ra_id=1 NOT registered
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                min_fee_a: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((9, 0)), // bootstrap phase
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        result.is_ok(),
        "bootstrap ParameterChange should skip return-account check: {result:?}"
    );
}

#[test]
fn proposal_post_bootstrap_rejects_unregistered_return_account() {
    // Post-bootstrap (PV 10), ProposalReturnAccountDoesNotExist IS enforced.
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(2); // ra_id=1 NOT registered
    let proposals = vec![sample_proposal(GovAction::InfoAction, 1, 1)];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)), // post-bootstrap
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(
        matches!(
            result,
            Err(LedgerError::ProposalReturnAccountDoesNotExist(_))
        ),
        "post-bootstrap should reject unregistered return account: {result:?}",
    );
}

// -----------------------------------------------------------------------
// Proposal validation: TreasuryWithdrawals edge cases
// -----------------------------------------------------------------------

#[test]
fn proposal_rejects_zero_treasury_withdrawals() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(sample_reward_account(1), 0);
    let proposals = vec![sample_proposal(
        GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        Some((10, 0)), // post-bootstrap: ZeroTreasuryWithdrawals enforced
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::ZeroTreasuryWithdrawals(_))
    ));
}

#[test]
fn proposal_rejects_treasury_withdrawal_to_unregistered_account() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut withdrawals = BTreeMap::new();
    // Withdrawal target ra_id=2 is not registered.
    withdrawals.insert(sample_reward_account(2), 1_000_000);
    let proposals = vec![sample_proposal(
        GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::TreasuryWithdrawalReturnAccountsDoNotExist(_))
    ));
}

#[test]
fn proposal_rejects_treasury_withdrawal_network_mismatch() {
    let es = EnactState::default();
    let mut stake_creds = StakeCredentials::new();
    // Register the return account credential (ra_id=1).
    stake_creds.register(crate::StakeCredential::AddrKeyHash([1; 28]));
    // Register the treasury withdrawal target credential.
    let cred = crate::StakeCredential::AddrKeyHash([0x77; 28]);
    stake_creds.register(cred);
    let ra = RewardAccount {
        network: 0,
        credential: cred,
    };

    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(ra, 1_000_000);

    // Use return account with network=1 (matches expected_network).
    let proposals = vec![sample_proposal(
        GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        Some(1), // expected network = 1
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::TreasuryWithdrawalsNetworkIdMismatch { .. })
    ));
}

// -----------------------------------------------------------------------
// Proposal validation: InvalidGuardrailsScriptHash
// -----------------------------------------------------------------------

#[test]
fn proposal_accepts_parameter_change_matching_guardrails_hash() {
    let guardrails_hash = [0xAB; 28];
    let mut es = EnactState::default();
    es.constitution.guardrails_script_hash = Some(guardrails_hash);

    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: {
                let mut u = crate::protocol_params::ProtocolParameterUpdate::default();
                u.min_fee_a = Some(100);
                u
            },
            guardrails_script_hash: Some(guardrails_hash),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn proposal_rejects_parameter_change_mismatched_guardrails_hash() {
    let constitution_hash = [0xAB; 28];
    let proposal_hash = [0xCD; 28];
    let mut es = EnactState::default();
    es.constitution.guardrails_script_hash = Some(constitution_hash);

    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: {
                let mut u = crate::protocol_params::ProtocolParameterUpdate::default();
                u.min_fee_a = Some(100);
                u
            },
            guardrails_script_hash: Some(proposal_hash),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidGuardrailsScriptHash {
            proposal_hash: Some(_),
            constitution_hash: Some(_),
        })
    ));
}

#[test]
fn proposal_rejects_parameter_change_none_vs_some_guardrails() {
    let constitution_hash = [0xAB; 28];
    let mut es = EnactState::default();
    es.constitution.guardrails_script_hash = Some(constitution_hash);

    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: {
                let mut u = crate::protocol_params::ProtocolParameterUpdate::default();
                u.min_fee_a = Some(100);
                u
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidGuardrailsScriptHash {
            proposal_hash: None,
            constitution_hash: Some(_),
        })
    ));
}

#[test]
fn proposal_rejects_parameter_change_some_vs_none_guardrails() {
    // Constitution has no guardrails but proposal supplies one.
    let es = EnactState::default(); // guardrails_script_hash = None
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: {
                let mut u = crate::protocol_params::ProtocolParameterUpdate::default();
                u.min_fee_a = Some(100);
                u
            },
            guardrails_script_hash: Some([0xAB; 28]),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidGuardrailsScriptHash {
            proposal_hash: Some(_),
            constitution_hash: None,
        })
    ));
}

#[test]
fn proposal_accepts_parameter_change_both_none_guardrails() {
    // Both constitution and proposal have no guardrails — should pass.
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: {
                let mut u = crate::protocol_params::ProtocolParameterUpdate::default();
                u.min_fee_a = Some(100);
                u
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn proposal_rejects_treasury_withdrawal_mismatched_guardrails_hash() {
    let constitution_hash = [0xAB; 28];
    let proposal_hash = [0xCD; 28];
    let mut es = EnactState::default();
    es.constitution.guardrails_script_hash = Some(constitution_hash);

    let stake_creds = empty_stake_creds_with(1);
    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(sample_reward_account(1), 1_000_000);
    let proposals = vec![sample_proposal(
        GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: Some(proposal_hash),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidGuardrailsScriptHash {
            proposal_hash: Some(_),
            constitution_hash: Some(_),
        })
    ));
}

#[test]
fn proposal_accepts_treasury_withdrawal_matching_guardrails_hash() {
    let guardrails_hash = [0xAB; 28];
    let mut es = EnactState::default();
    es.constitution.guardrails_script_hash = Some(guardrails_hash);

    let stake_creds = empty_stake_creds_with(1);
    let mut withdrawals = BTreeMap::new();
    withdrawals.insert(sample_reward_account(1), 1_000_000);
    let proposals = vec![sample_proposal(
        GovAction::TreasuryWithdrawals {
            withdrawals,
            guardrails_script_hash: Some(guardrails_hash),
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

// -----------------------------------------------------------------------
// Proposal validation: UpdateCommittee edge cases
// -----------------------------------------------------------------------

#[test]
fn proposal_rejects_conflicting_committee_update() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let conflicting_cred = crate::StakeCredential::AddrKeyHash([0x99; 28]);
    let mut members_to_add = BTreeMap::new();
    members_to_add.insert(conflicting_cred, 100);
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![conflicting_cred],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::ConflictingCommitteeUpdate(_))
    ));
}

#[test]
fn proposal_rejects_committee_member_expiring_at_current_epoch() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut members_to_add = BTreeMap::new();
    // Epoch 10 — member expiring at epoch 10 is not strictly after.
    members_to_add.insert(crate::StakeCredential::AddrKeyHash([0xAA; 28]), 10);
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(10),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::ExpirationEpochTooSmall(_))
    ));
}

// -----------------------------------------------------------------------
// Proposal validation: forward self-reference
// -----------------------------------------------------------------------

#[test]
fn proposal_rejects_forward_self_reference() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let tx_id = crate::types::TxId([0xBB; 32]);
    // Proposal at index 0 references gov_action_index 0 in same tx → forward self-ref.
    let proposals = vec![sample_proposal(
        GovAction::ParameterChange {
            prev_action_id: Some(crate::eras::conway::GovActionId {
                transaction_id: tx_id.0,
                gov_action_index: 0,
            }),
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(2_000_000),
                ..Default::default()
            },
            guardrails_script_hash: None,
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        tx_id,
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidPrevGovActionId(_))
    ));
}

#[test]
fn proposal_rejects_forward_reference_later_in_same_tx() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let tx_id = crate::types::TxId([0xBB; 32]);
    // Proposal at index 0 referencing index 1 (forward ref).
    let proposals = vec![
        sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: Some(crate::eras::conway::GovActionId {
                    transaction_id: tx_id.0,
                    gov_action_index: 1,
                }),
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    key_deposit: Some(2_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        ),
        sample_proposal(
            GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                    key_deposit: Some(3_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            1,
            1,
        ),
    ];
    let result = validate_conway_proposals(
        tx_id,
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::InvalidPrevGovActionId(_))
    ));
}

// -----------------------------------------------------------------------
// WellFormedUnitIntervalRatification — quorum validation
// -----------------------------------------------------------------------

#[test]
fn proposal_rejects_update_committee_quorum_zero_denominator() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut members = BTreeMap::new();
    members.insert(crate::StakeCredential::AddrKeyHash([0xAA; 28]), 100);
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: members,
            quorum: UnitInterval {
                numerator: 1,
                denominator: 0,
            },
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::WellFormedUnitIntervalRatification { .. })
    ));
}

#[test]
fn proposal_rejects_update_committee_quorum_numerator_exceeds_denominator() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut members = BTreeMap::new();
    members.insert(crate::StakeCredential::AddrKeyHash([0xAA; 28]), 100);
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: members,
            quorum: UnitInterval {
                numerator: 5,
                denominator: 3,
            },
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(matches!(
        result,
        Err(LedgerError::WellFormedUnitIntervalRatification { .. })
    ));
}

#[test]
fn proposal_accepts_update_committee_quorum_valid_unit_interval() {
    let es = EnactState::default();
    let stake_creds = empty_stake_creds_with(1);
    let mut members = BTreeMap::new();
    members.insert(crate::StakeCredential::AddrKeyHash([0xAA; 28]), 100);
    let proposals = vec![sample_proposal(
        GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: members,
            quorum: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        },
        1,
        1,
    )];
    let result = validate_conway_proposals(
        crate::types::TxId([0xAA; 32]),
        &proposals,
        EpochNo(0),
        &mut BTreeMap::new(),
        &stake_creds,
        None,
        None,
        None,
        None,
        &es,
        None,
    );
    assert!(result.is_ok());
}

// -----------------------------------------------------------------------
// CC tally hot/cold credential resolution
// -----------------------------------------------------------------------

#[test]
fn committee_tally_resolves_hot_credential_distinct_from_cold() {
    // Cold credential ≠ hot credential — vote is keyed by HOT.
    // Verify tally correctly resolves cold→hot.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([0x50; 28]);
    let hot = StakeCredential::AddrKeyHash([0x60; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);

    // Vote is stored under the HOT credential hash (per Conway CDDL).
    action
        .votes
        .insert(Voter::CommitteeKeyHash([0x60; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    assert!(accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        true
    ));
}

#[test]
fn committee_tally_vote_under_cold_hash_not_found() {
    // If someone mistakenly inserts a vote keyed by the COLD hash,
    // the tally should NOT find it when the member has a distinct hot.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([0x50; 28]);
    let hot = StakeCredential::AddrKeyHash([0x60; 28]);
    cs.register_with_term(cold, 999);
    authorize_cc_hot(&mut cs, cold, hot);

    // Incorrectly keyed by cold hash.
    action
        .votes
        .insert(Voter::CommitteeKeyHash([0x50; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    // Should fail — the vote is under the wrong key.
    assert!(!accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        true
    ));
}

#[test]
fn committee_tally_unauthorized_member_vote_ignored() {
    // Member with no hot credential authorization — vote cannot be found.
    let mut action = test_hf_action();
    let mut cs = CommitteeState::default();
    let cold = StakeCredential::AddrKeyHash([0x50; 28]);
    cs.register_with_term(cold, 999);
    // No hot credential authorized.

    // Vote under cold hash (the only hash available).
    action
        .votes
        .insert(Voter::CommitteeKeyHash([0x50; 28]), Vote::Yes);

    let quorum = UnitInterval {
        numerator: 1,
        denominator: 1,
    };
    // Unauthorized member — vote not counted.
    assert!(!accepted_by_committee(
        &action,
        &cs,
        &quorum,
        EpochNo(0),
        0,
        false,
        true
    ));
}

// -----------------------------------------------------------------------
// Vote recasting and DRep vote removal on unregistration
// -----------------------------------------------------------------------

#[test]
fn vote_recast_overwrites_previous_vote() {
    let gov_id = crate::eras::conway::GovActionId {
        transaction_id: [0x01; 32],
        gov_action_index: 0,
    };
    let mut governance_actions = BTreeMap::new();
    governance_actions.insert(gov_id.clone(), test_info_action());

    let voter = Voter::DRepKeyHash([0xD1; 28]);
    let mut drep_state = DrepState::new();
    drep_state.register(
        DRep::KeyHash([0xD1; 28]),
        RegisteredDrep::new_active(0, None, EpochNo(1)),
    );

    // First vote: Yes
    let mut procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::new(),
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        gov_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures.procedures.insert(voter.clone(), votes);
    apply_conway_votes(
        &procedures,
        &mut governance_actions,
        &mut drep_state,
        EpochNo(5),
        0,
        false,
    );
    assert_eq!(
        governance_actions[&gov_id].votes.get(&voter),
        Some(&Vote::Yes),
    );

    // Second vote: changes to No → overwrites.
    let mut votes2 = BTreeMap::new();
    votes2.insert(
        gov_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::No,
            anchor: None,
        },
    );
    let mut procedures2 = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::new(),
    };
    procedures2.procedures.insert(voter.clone(), votes2);
    apply_conway_votes(
        &procedures2,
        &mut governance_actions,
        &mut drep_state,
        EpochNo(5),
        0,
        false,
    );
    assert_eq!(
        governance_actions[&gov_id].votes.get(&voter),
        Some(&Vote::No),
    );
}

#[test]
fn vote_casting_touches_drep_activity() {
    let gov_id = crate::eras::conway::GovActionId {
        transaction_id: [0x01; 32],
        gov_action_index: 0,
    };
    let mut governance_actions = BTreeMap::new();
    governance_actions.insert(gov_id.clone(), test_info_action());

    let mut drep_state = DrepState::new();
    let drep = DRep::KeyHash([0xD1; 28]);
    drep_state.register(drep, RegisteredDrep::new_active(0, None, EpochNo(1)));

    let mut procedures = crate::eras::conway::VotingProcedures {
        procedures: BTreeMap::new(),
    };
    let mut votes = BTreeMap::new();
    votes.insert(
        gov_id.clone(),
        crate::eras::conway::VotingProcedure {
            vote: Vote::Yes,
            anchor: None,
        },
    );
    procedures
        .procedures
        .insert(Voter::DRepKeyHash([0xD1; 28]), votes);
    apply_conway_votes(
        &procedures,
        &mut governance_actions,
        &mut drep_state,
        EpochNo(42),
        0,
        false,
    );

    assert_eq!(
        drep_state.get(&drep).unwrap().last_active_epoch(),
        Some(EpochNo(42)),
    );
}

#[test]
fn drep_unregistration_removes_stored_votes() {
    let gov_id = crate::eras::conway::GovActionId {
        transaction_id: [0x01; 32],
        gov_action_index: 0,
    };
    let mut governance_actions = BTreeMap::new();
    let mut action = test_info_action();
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    action
        .votes
        .insert(Voter::DRepKeyHash([0xD2; 28]), Vote::No);
    governance_actions.insert(gov_id.clone(), action);

    // Simulate DRep [D1] unregistering.
    let unregistered = vec![Voter::DRepKeyHash([0xD1; 28])];
    remove_conway_drep_votes(&unregistered, &mut governance_actions);

    // D1's vote removed, D2's vote preserved.
    assert!(
        !governance_actions[&gov_id]
            .votes
            .contains_key(&Voter::DRepKeyHash([0xD1; 28]))
    );
    assert_eq!(
        governance_actions[&gov_id]
            .votes
            .get(&Voter::DRepKeyHash([0xD2; 28])),
        Some(&Vote::No),
    );
}

#[test]
fn drep_unregistration_removes_votes_across_multiple_actions() {
    let gov_id_1 = crate::eras::conway::GovActionId {
        transaction_id: [1; 32],
        gov_action_index: 0,
    };
    let gov_id_2 = crate::eras::conway::GovActionId {
        transaction_id: [2; 32],
        gov_action_index: 0,
    };
    let mut governance_actions = BTreeMap::new();

    let mut action_1 = test_info_action();
    action_1
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::Yes);
    governance_actions.insert(gov_id_1.clone(), action_1);

    let mut action_2 = test_hf_action();
    action_2
        .votes
        .insert(Voter::DRepKeyHash([0xD1; 28]), Vote::No);
    governance_actions.insert(gov_id_2.clone(), action_2);

    let unregistered = vec![Voter::DRepKeyHash([0xD1; 28])];
    remove_conway_drep_votes(&unregistered, &mut governance_actions);

    assert!(
        !governance_actions[&gov_id_1]
            .votes
            .contains_key(&Voter::DRepKeyHash([0xD1; 28]))
    );
    assert!(
        !governance_actions[&gov_id_2]
            .votes
            .contains_key(&Voter::DRepKeyHash([0xD1; 28]))
    );
}

#[test]
fn collect_unregistered_drep_voters_from_certs() {
    let certificates = vec![
        DCert::DrepUnregistration(StakeCredential::AddrKeyHash([0xD1; 28]), 0),
        DCert::DrepUnregistration(StakeCredential::ScriptHash([0xD2; 28]), 0),
    ];
    let unregistered = collect_conway_unregistered_drep_voters(Some(&certificates));
    assert_eq!(unregistered.len(), 2);
    assert!(unregistered.contains(&Voter::DRepKeyHash([0xD1; 28])));
    assert!(unregistered.contains(&Voter::DRepScript([0xD2; 28])));
}

#[test]
fn collect_unregistered_drep_voters_deduplicates() {
    // Same DRep unregistered twice but only one entry.
    let certificates = vec![
        DCert::DrepUnregistration(StakeCredential::AddrKeyHash([0xD1; 28]), 0),
        DCert::DrepUnregistration(StakeCredential::AddrKeyHash([0xD1; 28]), 0),
    ];
    let unregistered = collect_conway_unregistered_drep_voters(Some(&certificates));
    assert_eq!(unregistered.len(), 1);
}

// -----------------------------------------------------------------------
// Voter existence checks
// -----------------------------------------------------------------------

#[test]
fn voter_exists_drep_script_hash() {
    let pool_state = PoolState::new();
    let committee_state = CommitteeState::default();
    let mut drep_state = DrepState::new();
    drep_state.register(DRep::ScriptHash([0xAB; 28]), RegisteredDrep::new(0, None));

    let voter = Voter::DRepScript([0xAB; 28]);
    assert!(conway_voter_exists(
        &voter,
        &pool_state,
        &committee_state,
        &drep_state
    ));

    let unknown_voter = Voter::DRepScript([0xCD; 28]);
    assert!(!conway_voter_exists(
        &unknown_voter,
        &pool_state,
        &committee_state,
        &drep_state
    ));
}

#[test]
fn voter_exists_committee_script_hash() {
    let pool_state = PoolState::new();
    let mut committee_state = CommitteeState::default();
    let cold_cred = StakeCredential::AddrKeyHash([0x01; 28]);
    committee_state.register(cold_cred);
    // Authorize hot key as a script hash.
    committee_state
        .get_mut(&cold_cred)
        .unwrap()
        .set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(
            StakeCredential::ScriptHash([0xEE; 28]),
        )));
    let drep_state = DrepState::new();

    let voter = Voter::CommitteeScript([0xEE; 28]);
    assert!(conway_voter_exists(
        &voter,
        &pool_state,
        &committee_state,
        &drep_state
    ));

    let unknown_voter = Voter::CommitteeScript([0xFF; 28]);
    assert!(!conway_voter_exists(
        &unknown_voter,
        &pool_state,
        &committee_state,
        &drep_state
    ));
}

#[test]
fn voter_exists_spo() {
    let mut pool_state = PoolState::new();
    pool_state.register(crate::types::PoolParams {
        operator: [0x01; 28],
        vrf_keyhash: [0; 32],
        pledge: 0,
        cost: 0,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: sample_reward_account(1),
        pool_owners: vec![],
        relays: vec![],
        pool_metadata: None,
    });
    let committee_state = CommitteeState::default();
    let drep_state = DrepState::new();

    let voter = Voter::StakePool([0x01; 28]);
    assert!(conway_voter_exists(
        &voter,
        &pool_state,
        &committee_state,
        &drep_state
    ));

    let unknown_voter = Voter::StakePool([0x02; 28]);
    assert!(!conway_voter_exists(
        &unknown_voter,
        &pool_state,
        &committee_state,
        &drep_state
    ));
}

// -----------------------------------------------------------------------
// Post-bootstrap voter permission matrix (complete)
// -----------------------------------------------------------------------

#[test]
fn post_bootstrap_spo_rejected_on_treasury_withdrawals() {
    let voter = Voter::StakePool([9; 28]);
    let action = GovAction::TreasuryWithdrawals {
        withdrawals: BTreeMap::new(),
        guardrails_script_hash: None,
    };
    assert!(!conway_voter_is_allowed_for_action(&voter, &action));
}

#[test]
fn post_bootstrap_spo_accepted_on_no_confidence() {
    let voter = Voter::StakePool([9; 28]);
    let action = GovAction::NoConfidence {
        prev_action_id: None,
    };
    assert!(conway_voter_is_allowed_for_action(&voter, &action));
}

#[test]
fn post_bootstrap_spo_accepted_on_hard_fork() {
    let voter = Voter::StakePool([9; 28]);
    let action = GovAction::HardForkInitiation {
        prev_action_id: None,
        protocol_version: (11, 0),
    };
    assert!(conway_voter_is_allowed_for_action(&voter, &action));
}

#[test]
fn post_bootstrap_spo_accepted_on_update_committee() {
    let voter = Voter::StakePool([9; 28]);
    let action = GovAction::UpdateCommittee {
        prev_action_id: None,
        members_to_remove: vec![],
        members_to_add: BTreeMap::new(),
        quorum: UnitInterval {
            numerator: 1,
            denominator: 2,
        },
    };
    assert!(conway_voter_is_allowed_for_action(&voter, &action));
}

#[test]
fn post_bootstrap_spo_rejected_on_new_constitution() {
    let voter = Voter::StakePool([9; 28]);
    let action = GovAction::NewConstitution {
        prev_action_id: None,
        constitution: sample_constitution("spo-test"),
    };
    assert!(!conway_voter_is_allowed_for_action(&voter, &action));
}

#[test]
fn post_bootstrap_committee_accepted_on_most_actions() {
    let voter = Voter::CommitteeKeyHash([9; 28]);
    // Committee can vote on everything except NoConfidence per Conway rules.
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::InfoAction
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (11, 0),
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("cc"),
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        }
    ));
}

#[test]
fn post_bootstrap_committee_rejected_on_no_confidence() {
    let voter = Voter::CommitteeKeyHash([9; 28]);
    let action = GovAction::NoConfidence {
        prev_action_id: None,
    };
    assert!(!conway_voter_is_allowed_for_action(&voter, &action));
}

#[test]
fn post_bootstrap_drep_accepted_on_all_actions() {
    let voter = Voter::DRepKeyHash([9; 28]);
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::InfoAction
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::NoConfidence {
            prev_action_id: None,
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (11, 0),
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::NewConstitution {
            prev_action_id: None,
            constitution: sample_constitution("drep"),
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval {
                numerator: 1,
                denominator: 2
            },
        }
    ));
    assert!(conway_voter_is_allowed_for_action(
        &voter,
        &GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: crate::protocol_params::ProtocolParameterUpdate {
                key_deposit: Some(1),
                ..Default::default()
            },
            guardrails_script_hash: None,
        }
    ));
}

// -----------------------------------------------------------------------
// conway_unit_interval_well_formed
// -----------------------------------------------------------------------

#[test]
fn unit_interval_well_formed_valid() {
    assert!(conway_unit_interval_well_formed(&UnitInterval {
        numerator: 0,
        denominator: 1
    }));
    assert!(conway_unit_interval_well_formed(&UnitInterval {
        numerator: 1,
        denominator: 1
    }));
    assert!(conway_unit_interval_well_formed(&UnitInterval {
        numerator: 2,
        denominator: 3
    }));
}

#[test]
fn unit_interval_well_formed_invalid() {
    // Zero denominator.
    assert!(!conway_unit_interval_well_formed(&UnitInterval {
        numerator: 0,
        denominator: 0
    }));
    // Numerator > denominator.
    assert!(!conway_unit_interval_well_formed(&UnitInterval {
        numerator: 2,
        denominator: 1
    }));
}

// ── Certificate processing unit tests ──────────────────────────

/// Helper: default CertificateValidationContext for cert unit tests.
fn sample_cert_ctx() -> CertificateValidationContext {
    CertificateValidationContext {
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        min_pool_cost: 170_000_000,
        e_max: 18,
        current_epoch: EpochNo(100),
        expected_network_id: Some(1),
        drep_deposit: Some(500_000),
        is_conway: false,
        bootstrap_phase: false,
        post_pv10: false,
    }
}

/// Helper: Conway-era CertificateValidationContext for cert unit tests
/// that exercise Conway-specific validation.
fn sample_conway_cert_ctx() -> CertificateValidationContext {
    CertificateValidationContext {
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        min_pool_cost: 170_000_000,
        e_max: 18,
        current_epoch: EpochNo(100),
        expected_network_id: Some(1),
        drep_deposit: Some(500_000),
        is_conway: true,
        bootstrap_phase: false,
        post_pv10: false,
    }
}

#[test]
fn test_cert_account_registration_deposit() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();
    let cred = crate::StakeCredential::AddrKeyHash([0xC1; 28]);

    let certs = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
    assert!(sc.is_registered(&cred));
    assert_eq!(dp.key_deposits, 2_000_000);
}

/// Conway DELEG rule: `checkStakeKeyNotRegistered` —
/// `AccountRegistrationDeposit` must reject if credential is already registered.
/// Reference: `Cardano.Ledger.Conway.Rules.Deleg` — `StakeKeyRegisteredDELEG`.
#[test]
fn test_cert_conway_reregistration_rejected() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xC1; 28]);
    // Pre-register the credential so re-registration should be rejected.
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // AccountRegistrationDeposit (tag 7) — must fail.
    let certs = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    let res = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    );
    assert!(matches!(
        res,
        Err(LedgerError::StakeCredentialAlreadyRegistered(_))
    ));
}

/// Conway DELEG rule: `checkStakeKeyNotRegistered` for
/// `AccountRegistrationDelegationToStakePool` (tag 9).
#[test]
fn test_cert_conway_reg_deleg_pool_reregistration_rejected() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xE1; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xE1; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xE1; 28]),
        },
        pool_owners: vec![[0xE1; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xE2; 28]);
    // Pre-register so re-registration via reg+deleg cert should fail.
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::AccountRegistrationDelegationToStakePool(
        cred, operator, 2_000_000,
    )];
    let res = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    );
    assert!(matches!(
        res,
        Err(LedgerError::StakeCredentialAlreadyRegistered(_))
    ));
}

#[test]
fn test_cert_account_unregistration_deposit() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xC2; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 2_000_000,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::AccountUnregistrationDeposit(cred, 2_000_000)];
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
    assert!(!sc.is_registered(&cred));
    assert_eq!(dp.key_deposits, 0);
}

#[test]
fn test_cert_delegation_to_stake_pool_and_drep() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xD1; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xD1; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xD1; 28]),
        },
        pool_owners: vec![[0xD1; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xD2; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    // Register a DRep for delegation target.
    let _drep_cred = crate::StakeCredential::AddrKeyHash([0xD3; 28]);
    let drep = DRep::KeyHash([0xD3; 28]);
    ds.register(drep, RegisteredDrep::new(0, None));
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::DelegationToStakePoolAndDrep(cred, operator, drep)];
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
    let sc_state = sc.get(&cred).unwrap();
    assert_eq!(sc_state.delegated_pool(), Some(operator));
    assert_eq!(sc_state.delegated_drep(), Some(drep));
}

#[test]
fn test_cert_account_reg_delegation_to_stake_pool() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xE1; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xE1; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xE1; 28]),
        },
        pool_owners: vec![[0xE1; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();
    let cred = crate::StakeCredential::AddrKeyHash([0xE2; 28]);

    let certs = vec![DCert::AccountRegistrationDelegationToStakePool(
        cred, operator, 2_000_000,
    )];
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
    assert!(sc.is_registered(&cred));
    assert_eq!(sc.get(&cred).unwrap().delegated_pool(), Some(operator));
    assert_eq!(dp.key_deposits, 2_000_000);
}

#[test]
fn test_cert_account_reg_delegation_to_drep() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let drep = DRep::AlwaysAbstain;
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();
    let cred = crate::StakeCredential::AddrKeyHash([0xE3; 28]);

    let certs = vec![DCert::AccountRegistrationDelegationToDrep(
        cred, drep, 2_000_000,
    )];
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
    assert!(sc.is_registered(&cred));
    assert_eq!(sc.get(&cred).unwrap().delegated_drep(), Some(drep));
    assert_eq!(dp.key_deposits, 2_000_000);
}

#[test]
fn test_cert_account_reg_delegation_to_pool_and_drep() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xF1; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xF1; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xF1; 28]),
        },
        pool_owners: vec![[0xF1; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let drep = DRep::AlwaysNoConfidence;
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();
    let cred = crate::StakeCredential::AddrKeyHash([0xF2; 28]);

    let certs = vec![DCert::AccountRegistrationDelegationToStakePoolAndDrep(
        cred, operator, drep, 2_000_000,
    )];
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
    assert!(sc.is_registered(&cred));
    assert_eq!(sc.get(&cred).unwrap().delegated_pool(), Some(operator));
    assert_eq!(sc.get(&cred).unwrap().delegated_drep(), Some(drep));
    assert_eq!(dp.key_deposits, 2_000_000);
}

#[test]
fn test_cert_drep_registration_and_unregistration() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();
    let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);

    // Register DRep.
    let reg_certs = vec![DCert::DrepRegistration(cred, 500_000, None)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&reg_certs),
        None,
    )
    .unwrap();
    let drep = DRep::KeyHash([0xA0; 28]);
    assert!(ds.is_registered(&drep));
    assert_eq!(dp.drep_deposits, 500_000);

    // Unregister DRep.
    let unreg_certs = vec![DCert::DrepUnregistration(cred, 500_000)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&unreg_certs),
        None,
    )
    .unwrap();
    assert!(!ds.is_registered(&drep));
    assert_eq!(dp.drep_deposits, 0);
}

#[test]
fn test_cert_drep_update() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();
    let cred = crate::StakeCredential::AddrKeyHash([0xA1; 28]);

    // Register first.
    let reg_certs = vec![DCert::DrepRegistration(cred, 500_000, None)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&reg_certs),
        None,
    )
    .unwrap();

    // Update with anchor.
    let anchor = Some(Anchor {
        url: "https://drep.example".to_string(),
        data_hash: [0xBB; 32],
    });
    let upd_certs = vec![DCert::DrepUpdate(cred, anchor.clone())];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&upd_certs),
        None,
    )
    .unwrap();
    let drep = DRep::KeyHash([0xA1; 28]);
    assert!(ds.is_registered(&drep));
}

#[test]
fn test_cert_pool_registration() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    // Register the pool owner as a stake credential (not required by
    // upstream POOL rule, but useful for reward claiming in tests).
    sc.register(StakeCredential::AddrKeyHash([0xAA; 28]));

    let params = PoolParams {
        operator: [0xAA; 28],
        vrf_keyhash: [0xAA; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xAA; 28]),
        },
        pool_owners: vec![[0xAA; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![DCert::PoolRegistration(params.clone())];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert!(pool.is_registered(&[0xAA; 28]));
    assert_eq!(dp.pool_deposits, 500_000_000);
}

/// Upstream POOL rule does not enforce pool-owner registration as a
/// stake credential. Pool registration must succeed even when owners
/// are unregistered. Reference: `Cardano.Ledger.Shelley.Rules.Pool`.
#[test]
fn test_cert_pool_registration_unregistered_owner_accepted() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    // Intentionally do NOT register the owner as a stake credential.
    let params = PoolParams {
        operator: [0xBB; 28],
        vrf_keyhash: [0xBB; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xBB; 28]),
        },
        pool_owners: vec![[0xBB; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![DCert::PoolRegistration(params)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert!(pool.is_registered(&[0xBB; 28]));
}

#[test]
fn test_cert_pool_registration_duplicate_owner() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    sc.register(StakeCredential::AddrKeyHash([0xAA; 28]));

    let params = PoolParams {
        operator: [0xAA; 28],
        vrf_keyhash: [0xAA; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xAA; 28]),
        },
        pool_owners: vec![[0xAA; 28], [0xAA; 28]], // duplicate owner
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![DCert::PoolRegistration(params)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::DuplicatePoolOwner { .. }));
}

#[test]
fn test_cert_pool_registration_cost_too_low() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let params = PoolParams {
        operator: [0xBB; 28],
        vrf_keyhash: [0xBB; 32],
        pledge: 1_000,
        cost: 1_000, // below min_pool_cost (170_000_000)
        margin: UnitInterval {
            numerator: 1,
            denominator: 10,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xBB; 28]),
        },
        pool_owners: vec![[0xBB; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![DCert::PoolRegistration(params)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolCostTooLow { .. }));
}

#[test]
fn test_cert_pool_registration_invalid_margin() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let params = PoolParams {
        operator: [0xBC; 28],
        vrf_keyhash: [0xBC; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 2,
            denominator: 1,
        }, // invalid: num > denom
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xBC; 28]),
        },
        pool_owners: vec![[0xBC; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![DCert::PoolRegistration(params)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolMarginInvalid { .. }));
}

#[test]
fn test_cert_pool_registration_reward_network_mismatch() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // expected_network_id = Some(1)

    let params = PoolParams {
        operator: [0xBD; 28],
        vrf_keyhash: [0xBD; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 0,
            credential: crate::StakeCredential::AddrKeyHash([0xBD; 28]),
        }, // network 0 != 1
        pool_owners: vec![[0xBD; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![DCert::PoolRegistration(params)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::PoolRewardAccountNetworkMismatch { .. }
    ));
}

#[test]
fn test_cert_pool_registration_metadata_url_too_long() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let params = PoolParams {
        operator: [0xBE; 28],
        vrf_keyhash: [0xBE; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xBE; 28]),
        },
        pool_owners: vec![[0xBE; 28]],
        relays: vec![],
        pool_metadata: Some(crate::types::PoolMetadata {
            url: "x".repeat(65), // 65 bytes > 64
            metadata_hash: [0; 32],
        }),
    };
    let certs = vec![DCert::PoolRegistration(params)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolMetadataUrlTooLong { .. }));
}

#[test]
fn test_cert_pool_retirement() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xCC; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xCC; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xCC; 28]),
        },
        pool_owners: vec![[0xCC; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 500_000_000,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // current_epoch=100, e_max=18

    // Retire at epoch 110 (within 100+18=118).
    let certs = vec![DCert::PoolRetirement(operator, EpochNo(110))];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
}

#[test]
fn test_cert_pool_retirement_epoch_too_far() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xCD; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xCD; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xCD; 28]),
        },
        pool_owners: vec![[0xCD; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 500_000_000,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // current_epoch=100, e_max=18

    // Retire at epoch 200 — beyond 100+18=118.
    let certs = vec![DCert::PoolRetirement(operator, EpochNo(200))];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolRetirementTooFar { .. }));
}

#[test]
fn test_cert_pool_retirement_not_registered() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let certs = vec![DCert::PoolRetirement([0xDE; 28], EpochNo(110))];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolNotRegistered(_)));
}

#[test]
fn test_cert_pool_retirement_epoch_too_early() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xCF; 28];
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xCF; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xCF; 28]),
        },
        pool_owners: vec![[0xCF; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 500_000_000,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // current_epoch=100

    // Retire at current epoch (100) — upstream requires cEpoch < e (strictly future).
    let certs = vec![DCert::PoolRetirement(operator, EpochNo(100))];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolRetirementTooEarly { .. }));
}

#[test]
fn test_cert_genesis_delegation_valid() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    // Pre-populate genesis delegate mapping.
    gd.insert(
        [0xA0; 28],
        GenesisDelegationState {
            delegate: [0xB0; 28],
            vrf: [0xC0; 32],
        },
    );
    let ctx = sample_cert_ctx();

    let certs = vec![DCert::GenesisDelegation([0xA0; 28], [0xB1; 28], [0xC1; 32])];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert_eq!(gd[&[0xA0; 28]].delegate, [0xB1; 28]);
    assert_eq!(gd[&[0xA0; 28]].vrf, [0xC1; 32]);
}

#[test]
fn test_cert_genesis_delegation_scheduled_and_adopted() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut future_gd = std::collections::BTreeMap::new();
    gd.insert(
        [0xA0; 28],
        GenesisDelegationState {
            delegate: [0xB0; 28],
            vrf: [0xC0; 32],
        },
    );
    let ctx = sample_cert_ctx();
    let certs = vec![DCert::GenesisDelegation([0xA0; 28], [0xB1; 28], [0xC1; 32])];

    apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut future_gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        None,
    )
    .unwrap();

    // Not yet adopted before activation slot.
    assert_eq!(gd[&[0xA0; 28]].delegate, [0xB0; 28]);
    apply_scheduled_genesis_delegations(&mut gd, &mut future_gd, 104);
    assert_eq!(gd[&[0xA0; 28]].delegate, [0xB0; 28]);

    // Adopt at activation slot.
    apply_scheduled_genesis_delegations(&mut gd, &mut future_gd, 105);
    assert_eq!(gd[&[0xA0; 28]].delegate, [0xB1; 28]);
    assert!(future_gd.is_empty());
}

#[test]
fn test_cert_genesis_delegation_duplicate_checks_future_map() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut future_gd = std::collections::BTreeMap::new();
    gd.insert(
        [0xA0; 28],
        GenesisDelegationState {
            delegate: [0xB0; 28],
            vrf: [0xC0; 32],
        },
    );
    gd.insert(
        [0xA1; 28],
        GenesisDelegationState {
            delegate: [0xB1; 28],
            vrf: [0xC1; 32],
        },
    );
    schedule_future_genesis_delegation(
        &mut future_gd,
        120,
        [0xA1; 28],
        GenesisDelegationState {
            delegate: [0xB9; 28],
            vrf: [0xC9; 32],
        },
    );

    let ctx = sample_cert_ctx();
    let certs = vec![DCert::GenesisDelegation([0xA0; 28], [0xB9; 28], [0xCA; 32])];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut future_gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::DuplicateGenesisDelegate { .. }));
}

#[test]
fn test_cert_genesis_delegation_unknown_genesis_key() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    // Genesis key [0xA1..] is NOT in the delegate mapping.
    let certs = vec![DCert::GenesisDelegation([0xA1; 28], [0xB1; 28], [0xC1; 32])];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::GenesisKeyNotInMapping { .. }));
}

#[test]
fn test_cert_genesis_delegation_duplicate_delegate() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    gd.insert(
        [0xA0; 28],
        GenesisDelegationState {
            delegate: [0xB0; 28],
            vrf: [0xC0; 32],
        },
    );
    gd.insert(
        [0xA1; 28],
        GenesisDelegationState {
            delegate: [0xB1; 28],
            vrf: [0xC1; 32],
        },
    );
    let ctx = sample_cert_ctx();

    // Try to delegate [0xA1..] to [0xB0..] which is already used by [0xA0..].
    let certs = vec![DCert::GenesisDelegation([0xA1; 28], [0xB0; 28], [0xC9; 32])];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::DuplicateGenesisDelegate { .. }));
}

#[test]
fn test_cert_genesis_delegation_duplicate_vrf() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    gd.insert(
        [0xA0; 28],
        GenesisDelegationState {
            delegate: [0xB0; 28],
            vrf: [0xC0; 32],
        },
    );
    gd.insert(
        [0xA1; 28],
        GenesisDelegationState {
            delegate: [0xB1; 28],
            vrf: [0xC1; 32],
        },
    );
    let ctx = sample_cert_ctx();

    // Try to delegate [0xA1..] with VRF [0xC0..] which is already used by [0xA0..].
    let certs = vec![DCert::GenesisDelegation([0xA1; 28], [0xB9; 28], [0xC0; 32])];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::DuplicateGenesisVrf { .. }));
}

#[test]
fn test_conway_cert_rejected_in_pre_conway_era() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // is_conway = false

    // All Conway-only cert variants (CDDL tags 7–18) must be rejected.
    let conway_certs: Vec<DCert> = vec![
        DCert::AccountRegistrationDeposit(StakeCredential::AddrKeyHash([0x01; 28]), 2_000_000),
        DCert::AccountUnregistrationDeposit(StakeCredential::AddrKeyHash([0x02; 28]), 2_000_000),
        DCert::DelegationToDrep(
            StakeCredential::AddrKeyHash([0x03; 28]),
            DRep::AlwaysAbstain,
        ),
        DCert::DelegationToStakePoolAndDrep(
            StakeCredential::AddrKeyHash([0x04; 28]),
            [0x00; 28],
            DRep::AlwaysAbstain,
        ),
        DCert::AccountRegistrationDelegationToStakePool(
            StakeCredential::AddrKeyHash([0x05; 28]),
            [0x00; 28],
            2_000_000,
        ),
        DCert::AccountRegistrationDelegationToDrep(
            StakeCredential::AddrKeyHash([0x06; 28]),
            DRep::AlwaysAbstain,
            2_000_000,
        ),
        DCert::AccountRegistrationDelegationToStakePoolAndDrep(
            StakeCredential::AddrKeyHash([0x07; 28]),
            [0x00; 28],
            DRep::AlwaysAbstain,
            2_000_000,
        ),
        DCert::CommitteeAuthorization(
            StakeCredential::AddrKeyHash([0x08; 28]),
            StakeCredential::AddrKeyHash([0x09; 28]),
        ),
        DCert::CommitteeResignation(StakeCredential::AddrKeyHash([0x0A; 28]), None),
        DCert::DrepRegistration(StakeCredential::AddrKeyHash([0x0B; 28]), 500_000, None),
        DCert::DrepUnregistration(StakeCredential::AddrKeyHash([0x0C; 28]), 500_000),
        DCert::DrepUpdate(StakeCredential::AddrKeyHash([0x0D; 28]), None),
    ];

    for cert in &conway_certs {
        let single = vec![cert.clone()];
        let err = apply_certificates_and_withdrawals(
            &mut pool,
            &mut sc,
            &mut cs,
            &mut ds,
            &mut ra,
            &mut dp,
            &mut gd,
            &std::collections::BTreeMap::new(),
            &ctx,
            Some(&single),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err, LedgerError::UnsupportedCertificate(msg) if msg.contains("Conway")),
            "Expected UnsupportedCertificate for {:?}, got {:?}",
            cert,
            err,
        );
    }
}

#[test]
fn test_pre_conway_cert_rejected_in_conway_era() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx(); // is_conway = true

    // GenesisDelegation (tag 5) must be rejected in Conway.
    let certs = vec![DCert::GenesisDelegation([0xA0; 28], [0xB0; 28], [0xC0; 32])];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::UnsupportedCertificate(msg) if msg.contains("pre-Conway")),);

    // MoveInstantaneousReward (tag 6) must be rejected in Conway.
    let certs2 = vec![DCert::MoveInstantaneousReward(
        crate::types::MirPot::Reserves,
        crate::types::MirTarget::StakeCredentials(std::collections::BTreeMap::new()),
    )];
    let err2 = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs2),
        None,
    )
    .unwrap_err();
    assert!(matches!(err2, LedgerError::UnsupportedCertificate(msg) if msg.contains("pre-Conway")),);
}

#[test]
fn test_universal_certs_accepted_in_both_eras() {
    // Tags 0–4 (AccountRegistration, AccountUnregistration,
    // DelegationToStakePool, PoolRegistration, PoolRetirement)
    // must be accepted in both Shelley and Conway contexts.
    let pre_conway = sample_cert_ctx();
    let conway = sample_conway_cert_ctx();

    for ctx in [&pre_conway, &conway] {
        let mut pool = PoolState::new();
        let mut sc = StakeCredentials::new();
        let mut cs = CommitteeState::new();
        let mut ds = DrepState::new();
        let mut ra = RewardAccounts::new();
        let mut dp = DepositPot {
            key_deposits: 0,
            pool_deposits: 0,
            drep_deposits: 0,
            proposal_deposits: 0,
        };
        let mut gd = std::collections::BTreeMap::new();

        // Tag 0: AccountRegistration.
        let cred = StakeCredential::AddrKeyHash([0x01; 28]);
        let certs = vec![DCert::AccountRegistration(cred)];
        apply_certificates_and_withdrawals(
            &mut pool,
            &mut sc,
            &mut cs,
            &mut ds,
            &mut ra,
            &mut dp,
            &mut gd,
            &std::collections::BTreeMap::new(),
            ctx,
            Some(&certs),
            None,
        )
        .unwrap();
        assert!(sc.is_registered(&cred));
    }
}

#[test]
fn test_cert_committee_authorization() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xDA; 28]);
    let hot = crate::StakeCredential::AddrKeyHash([0xDB; 28]);
    cs.register_with_term(cold, 200);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::CommitteeAuthorization(cold, hot)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    let ms = cs.get(&cold).unwrap();
    assert!(!ms.is_resigned());
}

#[test]
fn test_cert_committee_authorization_unknown_member() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xDC; 28]);
    let hot = crate::StakeCredential::AddrKeyHash([0xDD; 28]);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::CommitteeAuthorization(cold, hot)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::CommitteeIsUnknown(_)));
}

#[test]
fn test_cert_committee_resignation() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xEA; 28]);
    cs.register_with_term(cold, 200);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::CommitteeResignation(cold, None)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    assert!(cs.get(&cold).unwrap().is_resigned());
}

#[test]
fn test_cert_committee_resignation_already_resigned() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xEB; 28]);
    cs.register_with_term(cold, 200);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // First resign.
    let certs1 = vec![DCert::CommitteeResignation(cold, None)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs1),
        None,
    )
    .unwrap();

    // Second resign should fail.
    let certs2 = vec![DCert::CommitteeResignation(cold, None)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs2),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::CommitteeHasPreviouslyResigned(_)
    ));
}

// -----------------------------------------------------------------------
// Gap #18: Committee unconditional membership check
// (upstream `checkAndOverwriteCommitteeMemberState`)
// -----------------------------------------------------------------------

#[test]
fn test_committee_auth_auto_registered_stale_entry_rejected() {
    // A credential was auto-registered via `is_potential_future_member`
    // (register() without term), but the pending proposal expired.
    // Now the credential is in CommitteeState but is NOT a real member.
    // Authorization must fail with `CommitteeIsUnknown`.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xD1; 28]);
    let hot = crate::StakeCredential::AddrKeyHash([0xD2; 28]);
    // Simulate stale auto-registration (no term epoch).
    cs.register(cold);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // No governance actions → credential is NOT a future member either.
    let certs = vec![DCert::CommitteeAuthorization(cold, hot)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::CommitteeIsUnknown(_)));
}

#[test]
fn test_committee_resign_auto_registered_stale_entry_rejected() {
    // Same as above but for resignation.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xD3; 28]);
    cs.register(cold);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::CommitteeResignation(cold, None)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::CommitteeIsUnknown(_)));
}

#[test]
fn test_committee_auth_enacted_member_succeeds() {
    // A properly enacted member (with term epoch) can authorize.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xD4; 28]);
    let hot = crate::StakeCredential::AddrKeyHash([0xD5; 28]);
    cs.register_with_term(cold, 200);
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::CommitteeAuthorization(cold, hot)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    let ms = cs.get(&cold).unwrap();
    assert!(matches!(
        ms.authorization(),
        Some(CommitteeAuthorization::CommitteeHotCredential(h)) if *h == hot
    ));
}

#[test]
fn test_committee_auth_potential_future_member_succeeds() {
    // A credential that is NOT in CommitteeState but IS a potential
    // future member (appears in a pending UpdateCommittee proposal).
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let cold = crate::StakeCredential::AddrKeyHash([0xD6; 28]);
    let hot = crate::StakeCredential::AddrKeyHash([0xD7; 28]);
    // No register — credential not in CommitteeState.
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // Seed a pending UpdateCommittee action that lists this credential.
    let mut members_to_add = std::collections::BTreeMap::new();
    members_to_add.insert(cold, 300u64);
    let action_id = crate::eras::conway::GovActionId {
        transaction_id: [0xA0; 32],
        gov_action_index: 0,
    };
    let mut gov = std::collections::BTreeMap::new();
    gov.insert(
        action_id,
        GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![0x00],
            gov_action: crate::eras::conway::GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }),
    );

    let certs = vec![DCert::CommitteeAuthorization(cold, hot)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &gov,
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap();
    // Credential was auto-registered in CommitteeState.
    assert!(cs.is_member(&cold));
}

// -----------------------------------------------------------------------
// Gap #20: RefundIncorrectDELEG PV split
// (upstream `hardforkConwayDELEGIncorrectDepositsAndRefunds`)
// -----------------------------------------------------------------------

#[test]
fn test_refund_incorrect_deleg_post_bootstrap() {
    // PV > 10 (PV 11+) uses RefundIncorrectDELEG.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xE2; 28]);

    // Register first.
    let mut ctx = sample_conway_cert_ctx();
    ctx.post_pv10 = true; // PV 11+: new error variant
    let reg = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&reg),
        None,
    )
    .unwrap();

    // Attempt wrong refund.
    let unreg = vec![DCert::AccountUnregistrationDeposit(cred, 9_999_999)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&unreg),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::RefundIncorrectDELEG {
            supplied: 9_999_999,
            expected: 2_000_000,
        }
    ));
}

#[test]
fn test_refund_incorrect_deleg_bootstrap_phase() {
    // PV < 10 (bootstrap) uses legacy IncorrectKeyDepositRefund.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xE3; 28]);

    let mut ctx = sample_conway_cert_ctx();
    ctx.bootstrap_phase = true; // PV 9

    let reg = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&reg),
        None,
    )
    .unwrap();

    let unreg = vec![DCert::AccountUnregistrationDeposit(cred, 7_777_777)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&unreg),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::IncorrectKeyDepositRefund {
            supplied: 7_777_777,
            expected: 2_000_000,
        }
    ));
}

#[test]
fn test_cert_stake_credential_already_registered() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xFA; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let certs = vec![DCert::AccountRegistration(cred)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::StakeCredentialAlreadyRegistered(_)
    ));
}

#[test]
fn test_cert_stake_credential_unregister_not_registered() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xFB; 28]);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let certs = vec![DCert::AccountUnregistration(cred)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::StakeCredentialNotRegistered(_)));
}

#[test]
fn test_cert_delegate_to_unregistered_pool_shelley_rejects() {
    // Upstream Shelley DELEG checks DelegateeNotRegisteredDELEG for ALL
    // eras (Shelley through Babbage): `Map.member stakePool
    // (psStakePools ..) ?! DelegateeNotRegisteredDELEG stakePool`.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xFC; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // is_conway: false

    let certs = vec![DCert::DelegationToStakePool(cred, [0x00; 28])]; // pool not registered
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolNotRegistered(_)));
}

#[test]
fn test_cert_delegate_to_unregistered_pool_conway_rejects() {
    // Upstream Conway DELEG added `DelegateeStakePoolNotRegisteredDELEG`.
    // Reference: `Cardano.Ledger.Conway.Rules.Deleg` — `checkStakeDelegateeRegistered`.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xFC; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx(); // is_conway: true

    let certs = vec![DCert::DelegationToStakePool(cred, [0x00; 28])]; // pool not registered
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::PoolNotRegistered(_)));
}

#[test]
fn test_conway_pool_registration_duplicate_vrf_key_rejected_pv11() {
    // Upstream Conway POOL rule: `VRFKeyHashAlreadyRegistered`.
    // Two pools cannot register with the same VRF key at PV > 10.
    let mut pool_state = PoolState::new();
    let mut sc = StakeCredentials::new();
    // Register owners for both pools.
    let owner_a = StakeCredential::AddrKeyHash([0xA0; 28]);
    let owner_b = StakeCredential::AddrKeyHash([0xB0; 28]);
    sc.register(owner_a);
    sc.register(owner_b);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut ctx = sample_conway_cert_ctx();
    ctx.post_pv10 = true; // PV 11+: VRF key uniqueness enforced

    let shared_vrf: [u8; 32] = [0xCC; 32];
    let pool_a = PoolParams {
        operator: [0xA1; 28],
        vrf_keyhash: shared_vrf,
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA0; 28]),
        },
        pool_owners: vec![[0xA0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let pool_b = PoolParams {
        operator: [0xB1; 28],
        vrf_keyhash: shared_vrf, // same VRF key
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xB0; 28]),
        },
        pool_owners: vec![[0xB0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    // Register pool A first, then try pool B with same VRF key.
    let certs = vec![
        DCert::PoolRegistration(pool_a),
        DCert::PoolRegistration(pool_b),
    ];
    let err = apply_certificates_and_withdrawals(
        &mut pool_state,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::VrfKeyAlreadyRegistered { .. }));
}

#[test]
fn test_conway_pool_reregistration_same_vrf_key_accepted() {
    // Re-registering a pool with its own VRF key should succeed.
    let mut pool_state = PoolState::new();
    let mut sc = StakeCredentials::new();
    let owner = StakeCredential::AddrKeyHash([0xA0; 28]);
    sc.register(owner);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut ctx = sample_conway_cert_ctx();
    ctx.post_pv10 = true; // PV 11+: VRF key uniqueness enforced

    let params = PoolParams {
        operator: [0xA1; 28],
        vrf_keyhash: [0xDD; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA0; 28]),
        },
        pool_owners: vec![[0xA0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    // Register pool, then re-register with same params (same VRF key).
    let certs = vec![
        DCert::PoolRegistration(params.clone()),
        DCert::PoolRegistration(params),
    ];
    let result = apply_certificates_and_withdrawals(
        &mut pool_state,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    );
    assert!(
        result.is_ok(),
        "re-registration with same VRF key should succeed: {result:?}"
    );
}

#[test]
fn test_conway_bootstrap_duplicate_vrf_key_accepted() {
    // Conway bootstrap (PV 9) and PV 10: duplicate VRF keys are allowed.
    // Upstream: `hardforkConwayDisallowDuplicatedVRFKeys pv = pvMajor pv > natVersion @10`
    // — only active at PV 11+.
    let mut pool_state = PoolState::new();
    let mut sc = StakeCredentials::new();
    let owner_a = StakeCredential::AddrKeyHash([0xA0; 28]);
    let owner_b = StakeCredential::AddrKeyHash([0xB0; 28]);
    sc.register(owner_a);
    sc.register(owner_b);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx(); // post_pv10: false (PV 9/10)

    let shared_vrf: [u8; 32] = [0xCC; 32];
    let pool_a = PoolParams {
        operator: [0xA1; 28],
        vrf_keyhash: shared_vrf,
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA0; 28]),
        },
        pool_owners: vec![[0xA0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let pool_b = PoolParams {
        operator: [0xB1; 28],
        vrf_keyhash: shared_vrf, // same VRF key — allowed at PV <= 10
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xB0; 28]),
        },
        pool_owners: vec![[0xB0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![
        DCert::PoolRegistration(pool_a),
        DCert::PoolRegistration(pool_b),
    ];
    let result = apply_certificates_and_withdrawals(
        &mut pool_state,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    );
    assert!(
        result.is_ok(),
        "Conway PV9/10 should allow duplicate VRF keys: {result:?}"
    );
}

#[test]
fn test_pool_reregistration_stages_future_params() {
    // Re-registering an existing pool should NOT change current params;
    // new params are staged in future_params.
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xFA; 28];
    let original = PoolParams {
        operator,
        vrf_keyhash: [0x01; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xFA; 28]),
        },
        pool_owners: vec![[0xFA; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let updated = PoolParams {
        pledge: 5_000,
        vrf_keyhash: [0x02; 32],
        ..original.clone()
    };
    pool.register_with_deposit(original.clone(), 500_000_000);
    pool.register_with_deposit(updated.clone(), 0); // deposit ignored for re-reg

    // Current params unchanged.
    assert_eq!(pool.get(&operator).unwrap().params.pledge, 1_000);
    assert_eq!(pool.get(&operator).unwrap().params.vrf_keyhash, [0x01; 32]);
    // Future params staged.
    assert!(pool.future_params().contains_key(&operator));
    assert_eq!(pool.future_params()[&operator].pledge, 5_000);
    assert_eq!(pool.future_params()[&operator].vrf_keyhash, [0x02; 32]);
}

#[test]
fn test_adopt_future_params_applies_staged_and_clears() {
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xFB; 28];
    let original = PoolParams {
        operator,
        vrf_keyhash: [0x01; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xFB; 28]),
        },
        pool_owners: vec![[0xFB; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let updated = PoolParams {
        pledge: 9_000,
        ..original.clone()
    };
    pool.register_with_deposit(original, 500_000_000);
    pool.register_with_deposit(updated, 0); // stage re-registration

    pool.adopt_future_params();

    // New params adopted, deposit preserved.
    assert_eq!(pool.get(&operator).unwrap().params.pledge, 9_000);
    assert_eq!(pool.get(&operator).unwrap().deposit, 500_000_000);
    // Future set cleared.
    assert!(pool.future_params().is_empty());
}

#[test]
fn test_pool_state_cbor_round_trip_with_future_params() {
    let mut pool = PoolState::new();
    let op1: [u8; 28] = [0xAA; 28];
    let op2: [u8; 28] = [0xBB; 28];
    let mk_params = |op: [u8; 28], pledge: u64| PoolParams {
        operator: op,
        vrf_keyhash: [op[0]; 32],
        pledge,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash(op),
        },
        pool_owners: vec![op],
        relays: vec![],
        pool_metadata: None,
    };
    pool.register_with_deposit(mk_params(op1, 100), 500_000_000);
    pool.register_with_deposit(mk_params(op2, 200), 500_000_000);
    // Stage re-registration for op1.
    pool.register_with_deposit(mk_params(op1, 999), 0);

    let mut enc = crate::cbor::Encoder::new();
    pool.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();

    let mut dec = crate::cbor::Decoder::new(&bytes);
    let decoded = PoolState::decode_cbor(&mut dec).unwrap();

    assert_eq!(decoded.get(&op1).unwrap().params.pledge, 100); // current
    assert_eq!(decoded.future_params()[&op1].pledge, 999); // staged
    assert_eq!(decoded.get(&op2).unwrap().params.pledge, 200);
    assert!(decoded.future_params().get(&op2).is_none());
}

#[test]
fn test_pool_retirement_clears_future_params() {
    // If a pool is retired while it has staged future params,
    // process_retirements MUST clear both entries and future_params.
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xFC; 28];
    let original = PoolParams {
        operator,
        vrf_keyhash: [0x01; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xFC; 28]),
        },
        pool_owners: vec![[0xFC; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    pool.register_with_deposit(original.clone(), 500_000_000);
    // Stage re-registration.
    pool.register_with_deposit(
        PoolParams {
            pledge: 9_999,
            ..original
        },
        0,
    );
    assert!(pool.future_params().contains_key(&operator));
    // Schedule retirement.
    pool.retire(operator, EpochNo(5));
    let retired = pool.process_retirements(EpochNo(5));
    assert_eq!(retired, vec![operator]);
    assert!(!pool.is_registered(&operator));
    assert!(pool.future_params().is_empty());
}

#[test]
fn test_reregistration_clears_retirement() {
    // Re-registering a pool that is scheduled for retirement should
    // clear the retirement flag (upstream `psRetiring` deletion).
    let mut pool = PoolState::new();
    let operator: [u8; 28] = [0xFD; 28];
    let params = PoolParams {
        operator,
        vrf_keyhash: [0x01; 32],
        pledge: 1_000,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xFD; 28]),
        },
        pool_owners: vec![[0xFD; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    pool.register_with_deposit(params.clone(), 500_000_000);
    pool.retire(operator, EpochNo(10));
    assert!(pool.get(&operator).unwrap().retiring_epoch.is_some());

    // Re-register → retirement should be cleared.
    pool.register_with_deposit(
        PoolParams {
            pledge: 2_000,
            ..params
        },
        0,
    );
    assert!(pool.get(&operator).unwrap().retiring_epoch.is_none());
}

#[test]
fn test_shelley_pool_registration_duplicate_vrf_key_accepted() {
    // Pre-Conway: duplicate VRF keys are allowed.
    let mut pool_state = PoolState::new();
    let mut sc = StakeCredentials::new();
    let owner_a = StakeCredential::AddrKeyHash([0xA0; 28]);
    let owner_b = StakeCredential::AddrKeyHash([0xB0; 28]);
    sc.register(owner_a);
    sc.register(owner_b);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // is_conway: false

    let shared_vrf: [u8; 32] = [0xEE; 32];
    let pool_a = PoolParams {
        operator: [0xA1; 28],
        vrf_keyhash: shared_vrf,
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xA0; 28]),
        },
        pool_owners: vec![[0xA0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let pool_b = PoolParams {
        operator: [0xB1; 28],
        vrf_keyhash: shared_vrf, // same VRF key — should be allowed pre-Conway
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xB0; 28]),
        },
        pool_owners: vec![[0xB0; 28]],
        relays: vec![],
        pool_metadata: None,
    };
    let certs = vec![
        DCert::PoolRegistration(pool_a),
        DCert::PoolRegistration(pool_b),
    ];
    let result = apply_certificates_and_withdrawals(
        &mut pool_state,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    );
    assert!(
        result.is_ok(),
        "Shelley-era duplicate VRF keys should be allowed: {result:?}"
    );
}

#[test]
fn test_cert_drep_already_registered() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xFD; 28]);
    let drep = DRep::KeyHash([0xFD; 28]);
    ds.register(drep, RegisteredDrep::new(500_000, None));
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    let certs = vec![DCert::DrepRegistration(cred, 500_000, None)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::DrepAlreadyRegistered(_)));
}

#[test]
fn test_cert_delegate_to_unregistered_drep() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xFE; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // Delegate to a DRep that is NOT registered and NOT a built-in.
    let drep = DRep::KeyHash([0x99; 28]);
    let certs = vec![DCert::DelegationToDrep(cred, drep)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(err, LedgerError::DelegateeDRepNotRegistered(_)));
}

#[test]
fn test_cert_withdrawals_credited_correctly() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xAB; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let ra_key = RewardAccount {
        network: 1,
        credential: cred,
    };
    let mut ra = RewardAccounts::new();
    ra.insert(ra_key, RewardAccountState::new(100, None));
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra_key, 100); // withdraw entire balance

    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        None,
        Some(&withdrawals),
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 100);
    assert_eq!(ra.balance(&ra_key), 0);
}

#[test]
fn test_cert_withdrawals_resolve_account_by_credential() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::ScriptHash([0xCD; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let stored_key = RewardAccount {
        network: 1,
        credential: cred,
    };
    let withdrawal_key = RewardAccount {
        network: 0,
        credential: cred,
    };
    let mut ra = RewardAccounts::new();
    ra.insert(stored_key, RewardAccountState::new(77, None));
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(withdrawal_key, 77);

    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        None,
        Some(&withdrawals),
    )
    .unwrap();

    assert_eq!(cert_adj.withdrawal_total, 77);
    assert_eq!(ra.balance(&stored_key), 0);
}

#[test]
fn test_stake_registration_creates_zero_reward_account() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::ScriptHash([0xCE; 28]);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();
    let reg = vec![DCert::AccountRegistration(cred)];

    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&reg),
        None,
    )
    .unwrap();

    let account = RewardAccount {
        network: 1,
        credential: cred,
    };
    assert_eq!(cert_adj.total_deposits, 2_000_000);
    assert!(sc.is_registered(&cred));
    assert_eq!(ra.balance(&account), 0);

    let withdrawals = std::collections::BTreeMap::from([(account, 0)]);
    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        None,
        Some(&withdrawals),
    )
    .unwrap();
    assert_eq!(cert_adj.withdrawal_total, 0);
}

/// Upstream Conway CERTS rule drains reward-account withdrawals BEFORE
/// processing certificates.  A transaction that withdraws from a reward
/// account AND unregisters the same credential must succeed because the
/// balance is already zero when `unregister_stake_credential` checks
/// `StakeCredentialHasRewards`.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Certs` —
/// `conwayCertsTransition` base case `Empty` drains accounts, then the
/// inductive step processes individual certificates.
#[test]
fn test_same_tx_withdraw_then_unregister_succeeds() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xCC; 28]);
    sc.register_with_deposit(cred, 2_000_000);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let ra_key = RewardAccount {
        network: 1,
        credential: cred,
    };
    let mut ra = RewardAccounts::new();
    ra.insert(ra_key, RewardAccountState::new(500_000, None));
    let mut dp = DepositPot {
        key_deposits: 2_000_000,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx();

    // Withdraw the entire balance AND unregister in a single call.
    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra_key, 500_000);
    let certs = vec![DCert::AccountUnregistration(cred)];

    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        Some(&withdrawals),
    )
    .expect("same-tx withdraw + unregister must succeed (upstream CERTS base-case ordering)");

    assert_eq!(cert_adj.withdrawal_total, 500_000);
    // Credential is now unregistered.
    assert!(!sc.is_registered(&cred));
    // Reward account entry was removed.
    assert_eq!(ra.get(&ra_key), None);
}

/// Conway variant: `AccountUnregistrationDeposit` (tag 8) with a
/// same-tx withdrawal should also succeed.
#[test]
fn test_same_tx_withdraw_then_conway_unregister_deposit_succeeds() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xDD; 28]);
    sc.register_with_deposit(cred, 2_000_000);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let ra_key = RewardAccount {
        network: 1,
        credential: cred,
    };
    let mut ra = RewardAccounts::new();
    ra.insert(ra_key, RewardAccountState::new(300_000, None));
    let mut dp = DepositPot {
        key_deposits: 2_000_000,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut ctx = sample_cert_ctx();
    ctx.is_conway = true;

    let mut withdrawals = std::collections::BTreeMap::new();
    withdrawals.insert(ra_key, 300_000);
    let certs = vec![DCert::AccountUnregistrationDeposit(cred, 2_000_000)];

    let cert_adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        Some(&withdrawals),
    )
    .expect("same-tx withdraw + Conway unregister must succeed");

    assert_eq!(cert_adj.withdrawal_total, 300_000);
    assert_eq!(cert_adj.total_refunds, 2_000_000);
    assert!(!sc.is_registered(&cred));
}

// -----------------------------------------------------------------------
// conway_pv_can_follow
// -----------------------------------------------------------------------

#[test]
fn pv_can_follow_major_increment() {
    assert!(conway_pv_can_follow((9, 0), (10, 0)));
}

#[test]
fn pv_can_follow_minor_increment() {
    assert!(conway_pv_can_follow((9, 0), (9, 1)));
}

#[test]
fn pv_can_follow_rejects_downgrade() {
    assert!(!conway_pv_can_follow((10, 0), (9, 0)));
}

#[test]
fn pv_can_follow_rejects_same_version() {
    assert!(!conway_pv_can_follow((10, 0), (10, 0)));
}

#[test]
fn pv_can_follow_rejects_major_jump() {
    // Major +2 is not allowed (per upstream pvCanFollow).
    assert!(!conway_pv_can_follow((9, 0), (11, 0)));
}

#[test]
fn pv_can_follow_rejects_identity_at_u64_max_minor_boundary() {
    // A prior `saturating_add(1)` implementation would collapse at
    // `u64::MAX` and silently treat `(M, u64::MAX) → (M, u64::MAX)`
    // as an increment, accepting identity proposals at that
    // boundary. The `checked_add` form rejects it correctly since
    // the overflow branch becomes `None`.
    assert!(!conway_pv_can_follow((10, u64::MAX), (10, u64::MAX)));
    // But `(10, u64::MAX - 1) → (10, u64::MAX)` is still a valid
    // minor increment and must stay accepted.
    assert!(conway_pv_can_follow((10, u64::MAX - 1), (10, u64::MAX)));
}

#[test]
fn pv_can_follow_rejects_major_overflow() {
    // `(u64::MAX, 5) → (0, 0)` would wrap in unchecked arithmetic.
    // `checked_add` on the major branch returns `None`, so this is
    // rejected — no overflow sneaks through.
    assert!(!conway_pv_can_follow((u64::MAX, 5), (0, 0)));
}

// ── validate_alonzo_plus_tx: mandatory collateral for redeemers ────

#[test]
fn alonzo_plus_tx_missing_collateral_with_redeemers() {
    let params = ProtocolParameters::alonzo_defaults();
    let utxo = MultiEraUtxo::new();
    let outputs = vec![];
    // has_redeemers = true, collateral_inputs = None → must fail
    let result = validate_alonzo_plus_tx(
        &params, &utxo, 200, 200_000, &outputs, None, None, None, None, None, None, true, 0, false,
    );
    assert!(matches!(
        result,
        Err(LedgerError::MissingCollateralForScripts)
    ));
}

#[test]
fn alonzo_plus_tx_empty_collateral_with_redeemers() {
    let params = ProtocolParameters::alonzo_defaults();
    let utxo = MultiEraUtxo::new();
    let outputs = vec![];
    // has_redeemers = true, collateral_inputs = Some(&[]) → must fail
    let result = validate_alonzo_plus_tx(
        &params,
        &utxo,
        200,
        200_000,
        &outputs,
        None,
        Some(&[]),
        None,
        None,
        None,
        None,
        true,
        0,
        false,
    );
    assert!(matches!(
        result,
        Err(LedgerError::MissingCollateralForScripts)
    ));
}

#[test]
fn alonzo_plus_tx_no_redeemers_skips_collateral() {
    let params = ProtocolParameters::alonzo_defaults();
    let utxo = MultiEraUtxo::new();
    let outputs = vec![];
    // has_redeemers = false, collateral_inputs = None → ok (no scripts)
    let result = validate_alonzo_plus_tx(
        &params, &utxo, 200, 200_000, &outputs, None, None, None, None, None, None, false, 0, false,
    );
    assert!(result.is_ok());
}

#[test]
fn alonzo_plus_tx_no_redeemers_skips_collateral_validation_even_if_present() {
    let params = ProtocolParameters::alonzo_defaults();
    let mut utxo = MultiEraUtxo::new();
    let collateral_in = crate::eras::shelley::ShelleyTxIn {
        transaction_id: [1u8; 32],
        index: 0,
    };
    // Script-locked enterprise address (`0x70` + 28-byte script hash).
    // Upstream only validates collateral when redeemers are present, so
    // this should pass when `has_redeemers = false`.
    let mut script_addr = vec![0x70];
    script_addr.extend_from_slice(&[2u8; 28]);
    utxo.insert(
        collateral_in.clone(),
        MultiEraTxOut::Shelley(crate::eras::shelley::ShelleyTxOut {
            address: script_addr,
            amount: 5_000_000,
        }),
    );
    let collateral_inputs = [collateral_in];
    let outputs = vec![];
    let result = validate_alonzo_plus_tx(
        &params,
        &utxo,
        200,
        200_000,
        &outputs,
        None,
        Some(&collateral_inputs),
        None,
        None,
        None,
        None,
        false,
        0,
        false,
    );
    assert!(result.is_ok());
}

#[test]
fn collateral_return_checked_for_output_too_big() {
    // Upstream `allSizedOutputsTxBodyF` includes collateral_return in
    // output-size validation. A collateral_return whose value exceeds
    // max_val_size must trigger OutputTooBig.
    let mut params = ProtocolParameters::alonzo_defaults();
    params.max_val_size = Some(10); // very small limit
    let utxo = MultiEraUtxo::new();
    let outputs = vec![]; // regular outputs are fine (empty)
    // Build a collateral_return with a multi-asset value that
    // serializes to more than 10 bytes.
    let mut ma = std::collections::BTreeMap::new();
    let policy_id = [0xAA; 28];
    let mut assets = std::collections::BTreeMap::new();
    assets.insert(b"longtokenname".to_vec(), 100);
    ma.insert(policy_id, assets);
    let big_value = crate::eras::mary::Value::CoinAndAssets(5_000_000, ma);
    let cr = MultiEraTxOut::Babbage(crate::eras::babbage::BabbageTxOut {
        address: vec![0x01; 57], // base address
        amount: big_value,
        datum_option: None,
        script_ref: None,
    });
    let result = validate_alonzo_plus_tx(
        &params,
        &utxo,
        200,
        200_000,
        &outputs,
        None,
        None,
        None,
        Some(&cr),
        None,
        None,
        false,
        0,
        false,
    );
    assert!(
        matches!(result, Err(LedgerError::OutputTooBig { .. })),
        "collateral_return must be validated for max_val_size"
    );
}

#[test]
fn babbage_plus_enforces_max_collateral_inputs_without_redeemers() {
    let mut params = ProtocolParameters::alonzo_defaults();
    params.max_collateral_inputs = Some(3);
    let mut utxo = MultiEraUtxo::new();
    let outputs = vec![];
    let mut inputs = Vec::new();
    for i in 0..4u16 {
        let txin = crate::eras::shelley::ShelleyTxIn {
            transaction_id: [3u8; 32],
            index: i,
        };
        utxo.insert(
            txin.clone(),
            MultiEraTxOut::Shelley(crate::eras::shelley::ShelleyTxOut {
                address: vec![0x60; 29],
                amount: 2_000_000,
            }),
        );
        inputs.push(txin);
    }

    let result = validate_alonzo_plus_tx(
        &params,
        &utxo,
        200,
        200_000,
        &outputs,
        None,
        Some(&inputs),
        None,
        None,
        None,
        None,
        false,
        0,
        true,
    );

    assert!(matches!(
        result,
        Err(LedgerError::TooManyCollateralInputs { count: 4, max: 3 })
    ));
}

// ── Network validation tests ───────────────────────────────────────

#[test]
fn shelley_address_network_id_extracts_correctly() {
    // Base address, network 1 (mainnet): header byte = 0x01
    assert_eq!(shelley_address_network_id(&[0x01]), Some(1));
    // Enterprise address, network 0 (testnet): header byte = 0x60
    assert_eq!(shelley_address_network_id(&[0x60]), Some(0));
    // Reward address, network 1: header byte = 0xe1
    assert_eq!(shelley_address_network_id(&[0xe1]), Some(1));
    // Pointer address, network 0: header byte = 0x40
    assert_eq!(shelley_address_network_id(&[0x40]), Some(0));
}

#[test]
fn shelley_address_network_id_returns_none_for_byron() {
    // Byron addresses have type nibble >= 8
    assert_eq!(shelley_address_network_id(&[0x82]), None);
    assert_eq!(shelley_address_network_id(&[0x83]), None);
    // Empty slice
    assert_eq!(shelley_address_network_id(&[]), None);
}

#[test]
fn validate_output_network_ids_accepts_matching() {
    // Mainnet (network=1) base address
    let mut addr_bytes = vec![0x01u8]; // header: type=0, net=1
    addr_bytes.extend_from_slice(&[0xaa; 56]); // 28+28 bytes
    let output = MultiEraTxOut::Shelley(ShelleyTxOut {
        address: addr_bytes,
        amount: 1_000_000,
    });
    assert!(validate_output_network_ids(1, &[output]).is_ok());
}

#[test]
fn validate_output_network_ids_rejects_mismatch() {
    // Testnet output (network=0) when mainnet (1) expected
    let mut addr_bytes = vec![0x00u8]; // header: type=0, net=0
    addr_bytes.extend_from_slice(&[0xaa; 56]);
    let output = MultiEraTxOut::Shelley(ShelleyTxOut {
        address: addr_bytes,
        amount: 1_000_000,
    });
    let result = validate_output_network_ids(1, &[output]);
    assert!(matches!(
        result,
        Err(LedgerError::WrongNetwork {
            expected: 1,
            found: 0,
        })
    ));
}

#[test]
fn validate_output_network_ids_skips_byron() {
    // Byron address (starts 0x82) — no network ID
    let output = MultiEraTxOut::Shelley(ShelleyTxOut {
        address: vec![0x82, 0xd8, 0x18, 0x58, 0x20],
        amount: 1_000_000,
    });
    assert!(validate_output_network_ids(1, &[output]).is_ok());
}

#[test]
fn validate_withdrawal_network_ids_accepts_matching() {
    let withdrawals = std::collections::BTreeMap::from([(
        RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xbb; 28]),
        },
        50_000u64,
    )]);
    assert!(validate_withdrawal_network_ids(1, &withdrawals).is_ok());
}

#[test]
fn validate_withdrawal_network_ids_rejects_mismatch() {
    let withdrawals = std::collections::BTreeMap::from([(
        RewardAccount {
            network: 0,
            credential: crate::StakeCredential::AddrKeyHash([0xbb; 28]),
        },
        50_000u64,
    )]);
    let result = validate_withdrawal_network_ids(1, &withdrawals);
    assert!(matches!(
        result,
        Err(LedgerError::WrongNetworkWithdrawal {
            expected: 1,
            found: 0,
        })
    ));
}

#[test]
fn validate_tx_body_network_id_accepts_matching() {
    assert!(validate_tx_body_network_id(1, Some(1)).is_ok());
    assert!(validate_tx_body_network_id(0, Some(0)).is_ok());
}

#[test]
fn validate_tx_body_network_id_accepts_absent() {
    // None means the tx body doesn't declare a network_id — always OK
    assert!(validate_tx_body_network_id(1, None).is_ok());
}

#[test]
fn validate_tx_body_network_id_rejects_mismatch() {
    let result = validate_tx_body_network_id(1, Some(0));
    assert!(matches!(
        result,
        Err(LedgerError::WrongNetworkInTxBody {
            expected: 1,
            found: 0,
        })
    ));
}

#[test]
fn validate_output_network_ids_mixed_valid_and_invalid() {
    // Two outputs: first matching (net=1), second mismatching (net=0)
    let mut good_addr = vec![0x01u8];
    good_addr.extend_from_slice(&[0xaa; 56]);
    let mut bad_addr = vec![0x00u8];
    bad_addr.extend_from_slice(&[0xbb; 56]);
    let outputs = vec![
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: good_addr,
            amount: 1_000_000,
        }),
        MultiEraTxOut::Shelley(ShelleyTxOut {
            address: bad_addr,
            amount: 2_000_000,
        }),
    ];
    let result = validate_output_network_ids(1, &outputs);
    assert!(matches!(
        result,
        Err(LedgerError::WrongNetwork {
            expected: 1,
            found: 0,
        })
    ));
}

// -----------------------------------------------------------------------
// CommitteeMemberState CBOR round-trip
// -----------------------------------------------------------------------

#[test]
fn committee_member_state_cbor_round_trip_with_term() {
    let mut member = CommitteeMemberState::with_term(100);
    member.set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(
        StakeCredential::AddrKeyHash([0xaa; 28]),
    )));

    let mut enc = Encoder::new();
    member.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = CommitteeMemberState::decode_cbor(&mut dec).unwrap();
    assert_eq!(decoded, member);
    assert_eq!(decoded.expires_at(), Some(100));
}

#[test]
fn committee_member_state_cbor_round_trip_no_auth_with_term() {
    let member = CommitteeMemberState::with_term(50);

    let mut enc = Encoder::new();
    member.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = CommitteeMemberState::decode_cbor(&mut dec).unwrap();
    assert_eq!(decoded, member);
    assert_eq!(decoded.expires_at(), Some(50));
    assert!(decoded.authorization().is_none());
}

#[test]
fn committee_member_state_cbor_round_trip_no_term() {
    let member = CommitteeMemberState::new();

    let mut enc = Encoder::new();
    member.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = CommitteeMemberState::decode_cbor(&mut dec).unwrap();
    assert_eq!(decoded, member);
    assert_eq!(decoded.expires_at(), None);
}

#[test]
fn committee_member_state_legacy_null_decode() {
    // Legacy format: bare null → no auth, no term.
    let mut enc = Encoder::new();
    enc.null();
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = CommitteeMemberState::decode_cbor(&mut dec).unwrap();
    assert_eq!(decoded.authorization(), None);
    assert_eq!(decoded.expires_at(), None);
}

#[test]
fn committee_member_state_legacy_auth_decode() {
    // Legacy format: [0, credential] → has auth, no term.
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0);
    StakeCredential::AddrKeyHash([0xcc; 28]).encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = CommitteeMemberState::decode_cbor(&mut dec).unwrap();
    assert!(decoded.authorization().is_some());
    assert_eq!(decoded.expires_at(), None);
}

#[test]
fn committee_member_is_expired_boundary() {
    let member = CommitteeMemberState::with_term(10);
    assert!(!member.is_expired(EpochNo(9))); // before term end
    assert!(!member.is_expired(EpochNo(10))); // at boundary (inclusive)
    assert!(member.is_expired(EpochNo(11))); // past expiry
}

// ----- Per-credential deposit tracking (upstream rdDeposit) -----

#[test]
fn test_credential_stores_deposit_at_registration() {
    // Register a stake credential — the stored deposit should match
    // the key_deposit at registration time.
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xE0; 28]);
    sc.register_with_deposit(cred, 2_000_000);
    let state = sc.get(&cred).unwrap();
    assert_eq!(state.deposit(), 2_000_000);
}

#[test]
fn test_credential_deposit_round_trips_through_cbor() {
    // StakeCredentialState with deposit survives CBOR encode/decode.
    let original = StakeCredentialState::new_with_deposit(None, None, 5_000_000);
    let mut enc = crate::cbor::Encoder::new();
    original.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = crate::cbor::Decoder::new(&bytes);
    let decoded = StakeCredentialState::decode_cbor(&mut dec).unwrap();
    assert_eq!(decoded.deposit(), 5_000_000);
    assert_eq!(decoded, original);
}

#[test]
fn test_credential_deposit_backward_compat_2_element_decode() {
    // Legacy 2-element CBOR (no deposit) decodes with deposit=0.
    let mut enc = crate::cbor::Encoder::new();
    enc.array(2);
    enc.null(); // no delegated_pool
    enc.null(); // no delegated_drep
    let bytes = enc.into_bytes();
    let mut dec = crate::cbor::Decoder::new(&bytes);
    let decoded = StakeCredentialState::decode_cbor(&mut dec).unwrap();
    assert_eq!(decoded.deposit(), 0);
}

#[test]
fn test_conway_unreg_validates_against_stored_deposit_not_current_param() {
    // Register a credential with deposit 2M. Then change key_deposit
    // to 3M and attempt Conway unregistration with refund=3M (current
    // param). Should FAIL because the stored deposit is 2M.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();

    let cred = crate::StakeCredential::AddrKeyHash([0xE1; 28]);

    // Step 1: Register with deposit=2M (matches key_deposit at the time).
    let reg_ctx = sample_conway_cert_ctx(); // key_deposit = 2_000_000
    let reg_certs = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &reg_ctx,
        Some(&reg_certs),
        None,
    )
    .unwrap();
    assert_eq!(sc.get(&cred).unwrap().deposit(), 2_000_000);

    // Step 2: Simulate key_deposit changing to 3M.
    let mut unreg_ctx = sample_conway_cert_ctx();
    unreg_ctx.key_deposit = 3_000_000;
    unreg_ctx.post_pv10 = true; // PV 11+: new error variant

    // Step 3: Attempt unregistration with refund=3M (current param).
    // Should fail: stored deposit is 2M.
    let unreg_certs = vec![DCert::AccountUnregistrationDeposit(cred, 3_000_000)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &unreg_ctx,
        Some(&unreg_certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::RefundIncorrectDELEG {
            supplied: 3_000_000,
            expected: 2_000_000,
        }
    ));
}

#[test]
fn test_conway_unreg_succeeds_with_stored_deposit() {
    // Register with deposit=2M, change key_deposit to 3M, then
    // unregister with refund=2M (matching stored deposit). Should succeed.
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();

    let cred = crate::StakeCredential::AddrKeyHash([0xE2; 28]);

    // Register with deposit=2M.
    let reg_ctx = sample_conway_cert_ctx();
    let reg_certs = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &reg_ctx,
        Some(&reg_certs),
        None,
    )
    .unwrap();

    // Change key_deposit to 3M — should not matter.
    let mut unreg_ctx = sample_conway_cert_ctx();
    unreg_ctx.key_deposit = 3_000_000;

    // Unregister with refund=2M (stored deposit). Should succeed.
    let unreg_certs = vec![DCert::AccountUnregistrationDeposit(cred, 2_000_000)];
    let adj = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &unreg_ctx,
        Some(&unreg_certs),
        None,
    )
    .unwrap();
    assert_eq!(adj.total_refunds, 2_000_000);
    assert!(!sc.is_registered(&cred));
}

// ------------------------------------------------------------------
// Conway re-registration rejection tests
// Reference: Cardano.Ledger.Conway.Rules.Deleg — `checkStakeKeyNotRegistered`
// Upstream rejects re-registration with `StakeKeyRegisteredDELEG`.
// ------------------------------------------------------------------

#[test]
fn conway_tag7_re_registration_is_rejected() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xA1; 28]);
    // Pre-register with the specific deposit amount.
    sc.register_with_deposit(cred, 2_000_000);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 2_000_000,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // Conway tag 7: AccountRegistrationDeposit on already-registered cred.
    let certs = vec![DCert::AccountRegistrationDeposit(cred, 2_000_000)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();

    // Upstream: `StakeKeyRegisteredDELEG` — re-registration is rejected.
    assert!(matches!(
        err,
        LedgerError::StakeCredentialAlreadyRegistered(_)
    ));
    // Deposit pot unchanged.
    assert_eq!(dp.key_deposits, 2_000_000);
}

#[test]
fn shelley_tag0_re_registration_still_errors() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xA2; 28]);
    sc.register(cred);
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // is_conway = false

    // Shelley tag 0: AccountRegistration on already-registered cred.
    let certs = vec![DCert::AccountRegistration(cred)];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        LedgerError::StakeCredentialAlreadyRegistered(_)
    ));
}

#[test]
fn conway_tag11_re_registration_rejected() {
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let cred = crate::StakeCredential::AddrKeyHash([0xA3; 28]);
    let pool_hash: [u8; 28] = [0xBB; 28];
    // Pre-register credential and register pool.
    sc.register_with_deposit(cred, 2_000_000);
    pool.register(PoolParams {
        operator: pool_hash,
        vrf_keyhash: [0xBB; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xBB; 28]),
        },
        pool_owners: vec![pool_hash],
        relays: vec![],
        pool_metadata: None,
    });
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 2_000_000,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_conway_cert_ctx();

    // Conway tag 11: AccountRegistrationDelegationToStakePool on already-registered cred.
    let certs = vec![DCert::AccountRegistrationDelegationToStakePool(
        cred, pool_hash, 2_000_000,
    )];
    let err = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    )
    .unwrap_err();

    // Upstream: `StakeKeyRegisteredDELEG` — re-registration is rejected.
    assert!(matches!(
        err,
        LedgerError::StakeCredentialAlreadyRegistered(_)
    ));
    assert_eq!(dp.key_deposits, 2_000_000); // unchanged
}

// ── Atomicity bug regression tests ────────────────────────────────

/// `StakeCredentials::register` must not overwrite an existing entry
/// when the credential is already registered.
///
/// Upstream: duplicate `AccountRegistration` in Shelley returns
/// `StakeKeyAlreadyRegisteredDELEG` without mutating the registry.
#[test]
fn stake_register_does_not_overwrite_existing_entry() {
    let mut sc = StakeCredentials::new();
    let cred = StakeCredential::AddrKeyHash([0xAA; 28]);
    assert!(sc.register(cred));
    // Set a delegation target on the existing entry.
    sc.get_mut(&cred)
        .unwrap()
        .set_delegated_pool(Some([0x11; 28]));
    // Attempt duplicate registration — must return false AND preserve
    // the existing delegation target.
    assert!(!sc.register(cred));
    assert_eq!(
        sc.get(&cred).unwrap().delegated_pool(),
        Some([0x11; 28]),
        "existing entry must not be overwritten by duplicate register",
    );
}

/// `StakeCredentials::register_with_deposit` must not overwrite deposit
/// or delegation state on a duplicate registration.
#[test]
fn stake_register_with_deposit_does_not_overwrite_existing_entry() {
    let mut sc = StakeCredentials::new();
    let cred = StakeCredential::AddrKeyHash([0xBB; 28]);
    assert!(sc.register_with_deposit(cred, 2_000_000));
    sc.get_mut(&cred)
        .unwrap()
        .set_delegated_pool(Some([0x22; 28]));
    // Attempt duplicate registration with a different deposit.
    assert!(!sc.register_with_deposit(cred, 5_000_000));
    // Original deposit and delegation must be preserved.
    assert_eq!(
        sc.get(&cred).unwrap().deposit(),
        2_000_000,
        "original deposit must not be overwritten",
    );
    assert_eq!(
        sc.get(&cred).unwrap().delegated_pool(),
        Some([0x22; 28]),
        "existing delegation must not be overwritten",
    );
}

/// `DrepState::register` must not overwrite an existing entry when the
/// DRep is already registered.
///
/// Upstream: Conway `DRepAlreadyRegisteredForEpoch` does not mutate
/// the DRep registry on failure.
#[test]
fn drep_register_does_not_overwrite_existing_entry() {
    let mut ds = DrepState::new();
    let drep = DRep::KeyHash([0xCC; 28]);
    let state1 = RegisteredDrep::new(7_000_000, None);
    assert!(ds.register(drep, state1));
    // Attempt duplicate registration with a different deposit.
    let state2 = RegisteredDrep::new(9_000_000, None);
    assert!(!ds.register(drep, state2));
    // Original deposit must be preserved.
    assert_eq!(
        ds.get(&drep).unwrap().deposit(),
        7_000_000,
        "existing DRep deposit must not be overwritten by duplicate register",
    );
}

/// Pool retirement epoch bounds must be checked BEFORE mutating pool
/// state.  A too-early retirement epoch must not corrupt the pool's
/// `retiring_epoch`.
#[test]
fn pool_retirement_too_early_does_not_mutate_state() {
    let operator: [u8; 28] = [0xDD; 28];
    let mut pool = PoolState::new();
    pool.register(PoolParams {
        operator,
        vrf_keyhash: [0xDD; 32],
        pledge: 0,
        cost: 170_000_000,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: crate::StakeCredential::AddrKeyHash([0xDD; 28]),
        },
        pool_owners: vec![[0xDD; 28]],
        relays: vec![],
        pool_metadata: None,
    });
    // The pool should have no retiring_epoch initially.
    assert!(pool.get(&operator).unwrap().retiring_epoch.is_none());
    // Attempt retirement at current epoch (100) — must fail.
    let certs = vec![DCert::PoolRetirement(operator, EpochNo(100))];
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 500_000_000,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let ctx = sample_cert_ctx(); // current_epoch=100, e_max=18
    let result = apply_certificates_and_withdrawals(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
    );
    assert!(result.is_err(), "retirement at current epoch must fail");
    // Crucially: the pool's retiring_epoch must NOT have been mutated.
    assert!(
        pool.get(&operator).unwrap().retiring_epoch.is_none(),
        "pool retiring_epoch must not be set when epoch validation fails",
    );
}

// ----------------------------------------------------------------
// ExtraneousScriptWitness — reference script deduction (Babbage+)
// ----------------------------------------------------------------

/// Helper: build a default (empty) ShelleyWitnessSet.
fn empty_witness_set() -> crate::eras::shelley::ShelleyWitnessSet {
    crate::eras::shelley::ShelleyWitnessSet {
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

#[test]
fn extraneous_script_witness_accepted_when_in_required() {
    // Script is required and provided → OK.
    let ns = crate::eras::allegra::NativeScript::ScriptPubkey([0xAA; 28]);
    let hash = crate::native_script::native_script_hash(&ns);
    let mut ws = empty_witness_set();
    ws.native_scripts.push(ns);
    let mut required = HashSet::new();
    required.insert(hash);
    let result = validate_no_extraneous_script_witnesses_typed(&ws, &required, None);
    assert!(result.is_ok());
}

#[test]
fn extraneous_script_witness_rejected_when_not_required() {
    // Script is provided but NOT required → ExtraneousScriptWitness.
    let ns = crate::eras::allegra::NativeScript::ScriptPubkey([0xBB; 28]);
    let hash = crate::native_script::native_script_hash(&ns);
    let mut ws = empty_witness_set();
    ws.native_scripts.push(ns);
    let required = HashSet::new(); // nothing required
    let result = validate_no_extraneous_script_witnesses_typed(&ws, &required, None);
    assert!(matches!(result, Err(LedgerError::ExtraneousScriptWitness { hash: h }) if h == hash));
}

#[test]
fn extraneous_script_deducted_by_reference_babbage() {
    // Script is required AND provided via reference. The witness copy is
    // extraneous because the reference already satisfies it. Upstream
    // Babbage logic: `neededNonRefs = sNeeded \ sRefs`, then
    // `sReceived ⊆ neededNonRefs` must hold. Because the script is in
    // sRefs, it is removed from needed, making the witness extraneous.
    let ns = crate::eras::allegra::NativeScript::ScriptPubkey([0xCC; 28]);
    let hash = crate::native_script::native_script_hash(&ns);
    let mut ws = empty_witness_set();
    ws.native_scripts.push(ns); // provided in witness set
    let mut required = HashSet::new();
    required.insert(hash); // required by transaction
    let mut refs = HashSet::new();
    refs.insert(hash); // also available via reference input
    // With refs: neededNonRefs = required \ refs = ∅ → witness is extraneous.
    let result = validate_no_extraneous_script_witnesses_typed(&ws, &required, Some(&refs));
    assert!(
        matches!(result, Err(LedgerError::ExtraneousScriptWitness { hash: h }) if h == hash),
        "script satisfied by reference must make the witness extraneous",
    );
}

#[test]
fn extraneous_script_not_deducted_without_reference() {
    // Same scenario but refs = None (pre-Babbage era). The witness is
    // acceptable because reference deduction doesn't apply.
    let ns = crate::eras::allegra::NativeScript::ScriptPubkey([0xCC; 28]);
    let hash = crate::native_script::native_script_hash(&ns);
    let mut ws = empty_witness_set();
    ws.native_scripts.push(ns);
    let mut required = HashSet::new();
    required.insert(hash);
    // Without refs: neededNonRefs = required → witness is accepted.
    let result = validate_no_extraneous_script_witnesses_typed(&ws, &required, None);
    assert!(
        result.is_ok(),
        "without reference deduction, witness covering a required script is fine"
    );
}

#[test]
fn extraneous_script_partial_deduction() {
    // Two scripts required, one is via reference. Providing only the
    // non-referenced one as a witness is OK. Providing both is not.
    let ns_a = crate::eras::allegra::NativeScript::ScriptPubkey([0xDD; 28]);
    let ns_b = crate::eras::allegra::NativeScript::ScriptPubkey([0xEE; 28]);
    let hash_a = crate::native_script::native_script_hash(&ns_a);
    let hash_b = crate::native_script::native_script_hash(&ns_b);
    let mut required = HashSet::new();
    required.insert(hash_a);
    required.insert(hash_b);
    let mut refs = HashSet::new();
    refs.insert(hash_b); // only B is via reference

    // Providing only A → OK
    let mut ws1 = empty_witness_set();
    ws1.native_scripts.push(ns_a.clone());
    assert!(
        validate_no_extraneous_script_witnesses_typed(&ws1, &required, Some(&refs)).is_ok(),
        "only the non-referenced script as witness should be accepted",
    );

    // Providing both A and B → B is extraneous
    let mut ws2 = empty_witness_set();
    ws2.native_scripts.push(ns_a);
    ws2.native_scripts.push(ns_b);
    let result = validate_no_extraneous_script_witnesses_typed(&ws2, &required, Some(&refs));
    assert!(
        matches!(result, Err(LedgerError::ExtraneousScriptWitness { hash: h }) if h == hash_b),
        "script B is available via reference, so providing it as witness is extraneous",
    );
}

// ── MIR certificate validation tests ──────────────────────────────
// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — MIR handling.

fn sample_mir_ctx_alonzo(ir: &InstantaneousRewards) -> MirValidationContext<'_> {
    MirValidationContext {
        current_slot: 100,
        mir_deadline_slot: Some(500),
        alonzo_mir_transfers: true,
        reserves: 10_000_000,
        treasury: 5_000_000,
        instantaneous_rewards: ir,
    }
}

fn sample_shelley_cert_ctx_for_mir() -> CertificateValidationContext {
    CertificateValidationContext {
        key_deposit: 2_000_000,
        pool_deposit: 500_000_000,
        min_pool_cost: 170_000_000,
        e_max: 18,
        current_epoch: EpochNo(100),
        expected_network_id: Some(1),
        drep_deposit: None,
        is_conway: false,
        bootstrap_phase: false,
        post_pv10: false,
    }
}

/// Upstream: `MIRCertificateTooLateinEpochDELEG` — slot >= deadline.
#[test]
fn test_mir_too_late_in_epoch() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = MirValidationContext {
        current_slot: 500,
        mir_deadline_slot: Some(500),
        alonzo_mir_transfers: false,
        reserves: 10_000_000,
        treasury: 5_000_000,
        instantaneous_rewards: &ir,
    };
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);
    let mut map = std::collections::BTreeMap::new();
    map.insert(cred, 1_000i64);
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::StakeCredentials(map),
    )];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        500,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(matches!(
        err,
        Err(LedgerError::MIRCertificateTooLateInEpoch {
            slot: 500,
            deadline: 500
        })
    ));
}

/// Upstream: `MIRNegativesNotCurrentlyAllowed` — pre-Alonzo negatives rejected.
#[test]
fn test_mir_negatives_not_allowed_pre_alonzo() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = MirValidationContext {
        current_slot: 100,
        mir_deadline_slot: Some(500),
        alonzo_mir_transfers: false,
        reserves: 10_000_000,
        treasury: 5_000_000,
        instantaneous_rewards: &ir,
    };
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);
    let mut map = std::collections::BTreeMap::new();
    map.insert(cred, -500i64);
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::StakeCredentials(map),
    )];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(matches!(
        err,
        Err(LedgerError::MIRNegativesNotCurrentlyAllowed)
    ));
}

/// Upstream: `MIRProducesNegativeUpdate` — Alonzo+ combined map negative.
#[test]
fn test_mir_produces_negative_update_alonzo() {
    let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);
    let mut ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    ir.ir_reserves.insert(cred, 500);
    let mir_ctx = sample_mir_ctx_alonzo(&ir);
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let mut map = std::collections::BTreeMap::new();
    map.insert(cred, -600i64); // combined = 500 + (-600) = -100 < 0
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::StakeCredentials(map),
    )];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(matches!(err, Err(LedgerError::MIRProducesNegativeUpdate)));
}

/// Upstream: `InsufficientForInstantaneousRewardsDELEG` — pot insufficient.
#[test]
fn test_mir_insufficient_pot_balance() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = MirValidationContext {
        current_slot: 100,
        mir_deadline_slot: Some(500),
        alonzo_mir_transfers: false,
        reserves: 1_000,
        treasury: 5_000_000,
        instantaneous_rewards: &ir,
    };
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);
    let mut map = std::collections::BTreeMap::new();
    map.insert(cred, 5_000i64); // 5000 > 1000 reserves
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::StakeCredentials(map),
    )];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(matches!(
        err,
        Err(LedgerError::MIRInsufficientPotBalance { .. })
    ));
}

/// Upstream: `MIRTransferNotCurrentlyAllowed` — pre-Alonzo transfer rejected.
#[test]
fn test_mir_transfer_not_allowed_pre_alonzo() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = MirValidationContext {
        current_slot: 100,
        mir_deadline_slot: Some(500),
        alonzo_mir_transfers: false,
        reserves: 10_000_000,
        treasury: 5_000_000,
        instantaneous_rewards: &ir,
    };
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::SendToOppositePot(1_000_000),
    )];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(matches!(
        err,
        Err(LedgerError::MIRTransferNotCurrentlyAllowed)
    ));
}

/// Upstream: `InsufficientForTransferDELEG` — transfer exceeds available.
#[test]
fn test_mir_insufficient_for_transfer() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = MirValidationContext {
        current_slot: 100,
        mir_deadline_slot: Some(500),
        alonzo_mir_transfers: true,
        reserves: 1_000,
        treasury: 5_000_000,
        instantaneous_rewards: &ir,
    };
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::SendToOppositePot(5_000),
    )];
    let err = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(matches!(
        err,
        Err(LedgerError::MIRInsufficientForTransfer { .. })
    ));
}

/// Alonzo+ MIR StakeCredentials with positive deltas that fit should succeed.
#[test]
fn test_mir_alonzo_positive_deltas_accepted() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = sample_mir_ctx_alonzo(&ir);
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let cred = crate::StakeCredential::AddrKeyHash([0xA0; 28]);
    let mut map = std::collections::BTreeMap::new();
    map.insert(cred, 1_000i64);
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Reserves,
        MirTarget::StakeCredentials(map),
    )];
    let result = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(result.is_ok());
}

/// Alonzo+ SendToOppositePot that fits in the pot should succeed.
#[test]
fn test_mir_alonzo_transfer_accepted() {
    let ir = InstantaneousRewards {
        ir_reserves: std::collections::BTreeMap::new(),
        ir_treasury: std::collections::BTreeMap::new(),
        delta_reserves: 0,
        delta_treasury: 0,
    };
    let mir_ctx = sample_mir_ctx_alonzo(&ir);
    let mut pool = PoolState::new();
    let mut sc = StakeCredentials::new();
    let mut cs = CommitteeState::new();
    let mut ds = DrepState::new();
    let mut ra = RewardAccounts::new();
    let mut dp = DepositPot {
        key_deposits: 0,
        pool_deposits: 0,
        drep_deposits: 0,
        proposal_deposits: 0,
    };
    let mut gd = std::collections::BTreeMap::new();
    let mut fgd = std::collections::BTreeMap::new();
    let ctx = sample_shelley_cert_ctx_for_mir();
    let certs = vec![DCert::MoveInstantaneousReward(
        MirPot::Treasury,
        MirTarget::SendToOppositePot(1_000),
    )];
    let result = apply_certificates_and_withdrawals_with_future(
        &mut pool,
        &mut sc,
        &mut cs,
        &mut ds,
        &mut ra,
        &mut dp,
        &mut gd,
        &mut fgd,
        &std::collections::BTreeMap::new(),
        &ctx,
        Some(&certs),
        None,
        100,
        Some(5),
        Some(&mir_ctx),
    );
    assert!(result.is_ok());
}

#[test]
fn test_running_utxo_ref_script_size_pv10_static() {
    // PV <= 10: block-level ref-script size uses static pre-block UTxO.
    // Tx A produces a ref-script output; Tx B references it.
    // At PV <= 10, Tx B does NOT see Tx A's new output, so the
    // block-total only counts Tx A's input ref-script (if any).
    use crate::eras::shelley::ShelleyTxIn;
    use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

    let mut utxo = MultiEraUtxo::new();
    // Existing UTxO entry with a ref-script (from before the block).
    let existing_input = ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    };
    let script_bytes = vec![0x82, 0x01, 0x87]; // 3 bytes
    let existing_out = crate::eras::babbage::BabbageTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: Some(crate::ScriptRef(crate::Script::PlutusV2(
            script_bytes.clone(),
        ))),
    };
    utxo.insert(existing_input.clone(), MultiEraTxOut::Babbage(existing_out));

    // Tx A: spends existing_input (with ref-script → 3 bytes).
    let tx_a_inputs = [existing_input];
    let total_a = utxo.total_ref_scripts_size(&tx_a_inputs, None);
    assert_eq!(total_a, 3);

    // Tx B: ref_inputs = [TxA output at index 0] — this output doesn't
    // exist in the pre-block UTxO yet.
    let tx_b_ref_input = ShelleyTxIn {
        transaction_id: [0xAA; 32],
        index: 0,
    };
    let total_b = utxo.total_ref_scripts_size(&[], Some(&[tx_b_ref_input]));
    // PV <= 10: Tx B can't see Tx A's output → 0 bytes.
    assert_eq!(total_b, 0);

    // Block total at PV <= 10: 3 + 0 = 3.
    assert_eq!(total_a + total_b, 3);
}

#[test]
fn test_running_utxo_ref_script_size_pv11_running() {
    // PV > 10: block-level ref-script size uses running UTxO.
    // After each tx, its outputs are added to a running overlay.
    // This test verifies the running UTxO overlay logic used in
    // apply_conway_block() at PV > 10.
    use crate::eras::shelley::ShelleyTxIn;
    use crate::utxo::{MultiEraTxOut, MultiEraUtxo};

    let mut utxo = MultiEraUtxo::new();
    // Existing UTxO with a ref-script.
    let existing_input = ShelleyTxIn {
        transaction_id: [0x01; 32],
        index: 0,
    };
    let script_bytes = vec![0x82, 0x01, 0x87]; // 3 bytes
    let existing_out = crate::eras::babbage::BabbageTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(2_000_000),
        datum_option: None,
        script_ref: Some(crate::ScriptRef(crate::Script::PlutusV2(
            script_bytes.clone(),
        ))),
    };
    utxo.insert(existing_input.clone(), MultiEraTxOut::Babbage(existing_out));

    // Simulate running UTxO overlay as in apply_conway_block PV > 10 path.
    let mut overlay: std::collections::HashMap<ShelleyTxIn, MultiEraTxOut> =
        std::collections::HashMap::new();

    // --- Tx A: spends existing_input (3 bytes from original UTxO).
    let tx_a_id = [0xAA; 32];
    let tx_a_inputs = [existing_input];
    let mut tx_a_ref_total: usize = 0;
    for input in tx_a_inputs.iter() {
        let txout = overlay.get(input).or_else(|| utxo.get(input));
        if let Some(out) = txout {
            if let Some(sr) = out.script_ref() {
                tx_a_ref_total += sr.0.binary_size();
            }
        }
    }
    assert_eq!(tx_a_ref_total, 3);
    // Tx A produces a new output with a ref-script (5 bytes).
    let new_script = vec![0x82, 0x02, 0x83, 0x00, 0x01]; // 5 bytes
    let tx_a_output = crate::eras::babbage::BabbageTxOut {
        address: vec![0x61; 29],
        amount: Value::Coin(1_000_000),
        datum_option: None,
        script_ref: Some(crate::ScriptRef(crate::Script::PlutusV2(new_script))),
    };
    let tx_a_out_key = ShelleyTxIn {
        transaction_id: tx_a_id,
        index: 0,
    };
    overlay.insert(tx_a_out_key.clone(), MultiEraTxOut::Babbage(tx_a_output));

    // --- Tx B: ref_inputs = [Tx A's output at index 0].
    // With running UTxO overlay, Tx B CAN see Tx A's output.
    let tx_b_ref_inputs = [tx_a_out_key];
    let mut tx_b_ref_total: usize = 0;
    for input in tx_b_ref_inputs.iter() {
        let txout = overlay.get(input).or_else(|| utxo.get(input));
        if let Some(out) = txout {
            if let Some(sr) = out.script_ref() {
                tx_b_ref_total += sr.0.binary_size();
            }
        }
    }
    assert_eq!(tx_b_ref_total, 5);

    // Block total at PV > 10: 3 + 5 = 8.
    assert_eq!(tx_a_ref_total + tx_b_ref_total, 8);
}

// -----------------------------------------------------------------------
// DepositPot — proposal_deposits parity
// Reference: upstream `Obligations` with `oblProposal` from
// `Cardano.Ledger.State.CertState`, and `sumObligation` which
// sums all four fields.
// -----------------------------------------------------------------------

#[test]
fn deposit_pot_total_includes_proposal_deposits() {
    let pot = DepositPot {
        key_deposits: 100,
        pool_deposits: 200,
        drep_deposits: 300,
        proposal_deposits: 400,
    };
    // upstream sumObligation = oblStake + oblPool + oblDRep + oblProposal
    assert_eq!(pot.total(), 1_000);
}

#[test]
fn deposit_pot_add_return_proposal_deposit() {
    let mut pot = DepositPot::default();
    assert_eq!(pot.proposal_deposits, 0);
    pot.add_proposal_deposit(5_000_000);
    assert_eq!(pot.proposal_deposits, 5_000_000);
    pot.add_proposal_deposit(3_000_000);
    assert_eq!(pot.proposal_deposits, 8_000_000);
    pot.return_proposal_deposit(2_000_000);
    assert_eq!(pot.proposal_deposits, 6_000_000);
    // saturating sub — cannot go negative
    pot.return_proposal_deposit(100_000_000);
    assert_eq!(pot.proposal_deposits, 0);
}

#[test]
fn deposit_pot_cbor_round_trip_4_element() {
    let pot = DepositPot {
        key_deposits: 10,
        pool_deposits: 20,
        drep_deposits: 30,
        proposal_deposits: 40,
    };
    let mut enc = Encoder::new();
    pot.encode_cbor(&mut enc);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = DepositPot::decode_cbor(&mut dec).unwrap();
    assert_eq!(pot, decoded);
}

#[test]
fn deposit_pot_cbor_backward_compat_3_element() {
    // Legacy 3-element encoding (before proposal_deposits was added).
    // Should decode with proposal_deposits = 0.
    let mut enc = Encoder::new();
    enc.array(3);
    enc.unsigned(100);
    enc.unsigned(200);
    enc.unsigned(300);
    let bytes = enc.into_bytes();
    let mut dec = Decoder::new(&bytes);
    let decoded = DepositPot::decode_cbor(&mut dec).unwrap();
    assert_eq!(
        decoded,
        DepositPot {
            key_deposits: 100,
            pool_deposits: 200,
            drep_deposits: 300,
            proposal_deposits: 0,
        }
    );
}

/// R264 regression pin: with `byron_shelley_transition = Some((86400, 4))`
/// (preprod), `LedgerState::epoch_first_slot(EpochNo(4))` must return
/// `86400` and `epoch_first_slot(EpochNo(5))` must return `518400`.
///
/// Pre-R264 the three sites in `state.rs` that compute
/// `current_epoch * slots_per_epoch` or `(current_epoch + 1) * slots_per_epoch`
/// silently returned wrong slots for any chain with a Byron prefix
/// (preprod, mainnet) — distorting:
///   - PPUP slot-of-no-return (`validate_ppup_proposal`)
///   - MIR cert deadline (`mir_validation_context`)
///   - blocks_made overlay classification (`should_count_block_producer`)
///
/// Reference: `docs/operational-runs/2026-05-06-round-264-byron-aware-ledger-epoch-first-slot.md`.
#[test]
fn preprod_byron_shelley_aware_epoch_first_slot() {
    use crate::state::EpochNo;

    let mut state = LedgerState::new(Era::Byron);
    state.set_slots_per_epoch(432_000);
    state.set_byron_shelley_transition(Some((86_400, 4))); // preprod

    // Shelley-era epochs use the era-aware boundary.
    assert_eq!(state.epoch_first_slot(EpochNo(4)), 86_400);
    assert_eq!(state.epoch_first_slot(EpochNo(5)), 518_400);
    assert_eq!(state.epoch_first_slot(EpochNo(6)), 950_400);

    // For comparison: fixed-length math from slot 0 would have given
    // 4*432000=1_728_000 for epoch 4 and 5*432000=2_160_000 for
    // epoch 5 — the original R263/R264 bug surface.
    assert_ne!(state.epoch_first_slot(EpochNo(4)), 4 * 432_000);

    // Without byron_shelley_transition (Shelley-only), fixed-length
    // math from slot 0 is the upstream-correct behaviour (preview).
    let mut shelley_only = LedgerState::new(Era::Byron);
    shelley_only.set_slots_per_epoch(86_400);
    shelley_only.set_byron_shelley_transition(None); // preview
    assert_eq!(shelley_only.epoch_first_slot(EpochNo(0)), 0);
    assert_eq!(shelley_only.epoch_first_slot(EpochNo(1)), 86_400);
    assert_eq!(shelley_only.epoch_first_slot(EpochNo(2)), 172_800);
}

/// R264 mainnet pin: same era-aware fix must apply to mainnet's
/// `byron_shelley_transition = Some((4_492_800, 208))`.
#[test]
fn mainnet_byron_shelley_aware_epoch_first_slot() {
    use crate::state::EpochNo;

    let mut state = LedgerState::new(Era::Byron);
    state.set_slots_per_epoch(432_000);
    state.set_byron_shelley_transition(Some((4_492_800, 208))); // mainnet

    assert_eq!(state.epoch_first_slot(EpochNo(208)), 4_492_800);
    assert_eq!(state.epoch_first_slot(EpochNo(209)), 4_924_800);
    assert_eq!(
        state.epoch_first_slot(EpochNo(300)),
        4_492_800 + 92 * 432_000
    );

    // Fixed-length math would have given 208*432000=89_856_000 for
    // mainnet's first Shelley epoch — wildly wrong vs upstream's
    // 4_492_800.
    assert_ne!(state.epoch_first_slot(EpochNo(208)), 208 * 432_000);
}
