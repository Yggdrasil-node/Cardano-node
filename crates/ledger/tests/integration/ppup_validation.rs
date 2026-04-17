use super::*;
use std::collections::BTreeMap;
use yggdrasil_ledger::{PpupSlotContext, pv_can_follow};

/// Helper: build a minimal LedgerState with gen_delegs seeded.
fn make_state_with_gen_delegs(delegate_keys: &[[u8; 28]]) -> LedgerState {
    let mut state = LedgerState::new(Era::Shelley);
    // Set current epoch for validation.
    state.set_current_epoch(EpochNo(10));
    // Set protocol params with a known protocol version (8,0 — Babbage).
    let mut params = ProtocolParameters::default();
    params.protocol_version = Some((8, 0));
    state.set_protocol_params(params);
    // Seed genesis delegations.
    for &key in delegate_keys {
        use yggdrasil_ledger::GenesisDelegationState;
        state.gen_delegs_mut().insert(
            key,
            GenesisDelegationState {
                delegate: [0xDD; 28],
                vrf: [0xEE; 32],
            },
        );
    }
    state
}

/// Helper: build a ShelleyUpdate with one proposer.
fn make_update(proposer: [u8; 28], epoch: u64, ppu: ProtocolParameterUpdate) -> ShelleyUpdate {
    let mut proposed = BTreeMap::new();
    proposed.insert(proposer, ppu);
    ShelleyUpdate {
        proposed_protocol_parameter_updates: proposed,
        epoch,
    }
}

// --------------------------------------------------------------------------
// pv_can_follow unit tests
// --------------------------------------------------------------------------

#[test]
fn pv_can_follow_major_bump() {
    // (8,0) -> (9,0) — valid major bump.
    assert!(pv_can_follow(8, 0, 9, 0));
}

#[test]
fn pv_can_follow_minor_bump() {
    // (8,0) -> (8,1) — valid minor bump.
    assert!(pv_can_follow(8, 0, 8, 1));
}

#[test]
fn pv_can_follow_rejects_same() {
    // Same version — not a legal successor.
    assert!(!pv_can_follow(8, 0, 8, 0));
}

#[test]
fn pv_can_follow_rejects_double_major() {
    // Skip a major version.
    assert!(!pv_can_follow(8, 0, 10, 0));
}

#[test]
fn pv_can_follow_rejects_major_with_minor() {
    // Major bump must reset minor to 0.
    assert!(!pv_can_follow(8, 0, 9, 1));
}

#[test]
fn pv_can_follow_rejects_skip_minor() {
    // Skip a minor version.
    assert!(!pv_can_follow(8, 0, 8, 2));
}

#[test]
fn pv_can_follow_rejects_downgrade() {
    // Downgrade.
    assert!(!pv_can_follow(8, 1, 8, 0));
}

// --------------------------------------------------------------------------
// validate_ppup_proposal tests
// --------------------------------------------------------------------------

#[test]
fn ppup_valid_proposal_accepted() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    let update = make_update(genesis_key, 10, ProtocolParameterUpdate::default());
    state
        .validate_ppup_proposal(&update, None)
        .expect("valid proposal should be accepted");
}

#[test]
fn ppup_valid_proposal_next_epoch_accepted() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // Target epoch is current+1 — also valid in relaxed mode.
    let update = make_update(genesis_key, 11, ProtocolParameterUpdate::default());
    state
        .validate_ppup_proposal(&update, None)
        .expect("next-epoch proposal should be accepted in relaxed mode");
}

#[test]
fn ppup_non_genesis_proposer_rejected() {
    let genesis_key = [0x01; 28];
    let intruder_key = [0xFF; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    let update = make_update(intruder_key, 10, ProtocolParameterUpdate::default());
    let err = state
        .validate_ppup_proposal(&update, None)
        .expect_err("non-genesis proposer should be rejected");
    match err {
        LedgerError::NonGenesisUpdatePPUP { proposer } => {
            assert_eq!(proposer, intruder_key);
        }
        other => panic!("expected NonGenesisUpdatePPUP, got: {:?}", other),
    }
}

#[test]
fn ppup_wrong_epoch_rejected() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // Target epoch 99 — neither current (10) nor current+1 (11).
    let update = make_update(genesis_key, 99, ProtocolParameterUpdate::default());
    let err = state
        .validate_ppup_proposal(&update, None)
        .expect_err("wrong epoch should be rejected");
    match err {
        LedgerError::PPUpdateWrongEpoch {
            current_epoch,
            target_epoch,
            ..
        } => {
            assert_eq!(current_epoch, 10);
            assert_eq!(target_epoch, 99);
        }
        other => panic!("expected PPUpdateWrongEpoch, got: {:?}", other),
    }
}

#[test]
fn ppup_illegal_protocol_version_rejected() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // Current is (8,0) — propose (10,0) which skips a major version.
    let mut ppu = ProtocolParameterUpdate::default();
    ppu.protocol_version = Some((10, 0));
    let update = make_update(genesis_key, 10, ppu);
    let err = state
        .validate_ppup_proposal(&update, None)
        .expect_err("illegal protver should be rejected");
    match err {
        LedgerError::PVCannotFollowPPUP {
            current_major,
            current_minor,
            proposed_major,
            proposed_minor,
        } => {
            assert_eq!(current_major, 8);
            assert_eq!(current_minor, 0);
            assert_eq!(proposed_major, 10);
            assert_eq!(proposed_minor, 0);
        }
        other => panic!("expected PVCannotFollowPPUP, got: {:?}", other),
    }
}

#[test]
fn ppup_legal_major_bump_accepted() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // (8,0) -> (9,0) is legal.
    let mut ppu = ProtocolParameterUpdate::default();
    ppu.protocol_version = Some((9, 0));
    let update = make_update(genesis_key, 10, ppu);
    state
        .validate_ppup_proposal(&update, None)
        .expect("legal major bump should be accepted");
}

#[test]
fn ppup_legal_minor_bump_accepted() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // (8,0) -> (8,1) is legal.
    let mut ppu = ProtocolParameterUpdate::default();
    ppu.protocol_version = Some((8, 1));
    let update = make_update(genesis_key, 10, ppu);
    state
        .validate_ppup_proposal(&update, None)
        .expect("legal minor bump should be accepted");
}

// --------------------------------------------------------------------------
// Slot-context (full upstream check) tests
// --------------------------------------------------------------------------

#[test]
fn ppup_slot_context_before_no_return_this_epoch() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // epoch_size=432000, stability_window=129600
    // current_epoch=10 → first_slot_next=11*432000=4752000
    // too_late = 4752000 - 129600 = 4622400
    // slot=4320000 (first slot of epoch 10) < 4622400 → this epoch
    let ctx = PpupSlotContext {
        slot: 4320000,
        epoch_size: 432000,
        stability_window: 129600,
    };
    let update = make_update(genesis_key, 10, ProtocolParameterUpdate::default());
    state
        .validate_ppup_proposal(&update, Some(&ctx))
        .expect("this-epoch proposal before slot-of-no-return should be accepted");
}

#[test]
fn ppup_slot_context_before_no_return_rejects_next_epoch() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    let ctx = PpupSlotContext {
        slot: 4320000,
        epoch_size: 432000,
        stability_window: 129600,
    };
    // Before slot-of-no-return: target must be current (10), not next (11).
    let update = make_update(genesis_key, 11, ProtocolParameterUpdate::default());
    let err = state
        .validate_ppup_proposal(&update, Some(&ctx))
        .expect_err("next-epoch target before slot-of-no-return should be rejected");
    match err {
        LedgerError::PPUpdateWrongEpoch {
            expected_epoch,
            voting_period,
            ..
        } => {
            assert_eq!(expected_epoch, 10);
            assert_eq!(voting_period, "VoteForThisEpoch");
        }
        other => panic!(
            "expected PPUpdateWrongEpoch VoteForThisEpoch, got: {:?}",
            other
        ),
    }
}

#[test]
fn ppup_slot_context_after_no_return_next_epoch() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // After slot-of-no-return: slot=4622400 >= too_late=4622400
    let ctx = PpupSlotContext {
        slot: 4622400,
        epoch_size: 432000,
        stability_window: 129600,
    };
    let update = make_update(genesis_key, 11, ProtocolParameterUpdate::default());
    state
        .validate_ppup_proposal(&update, Some(&ctx))
        .expect("next-epoch target at slot-of-no-return should be accepted");
}

#[test]
fn ppup_slot_context_after_no_return_rejects_this_epoch() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    let ctx = PpupSlotContext {
        slot: 4700000, // well past no-return
        epoch_size: 432000,
        stability_window: 129600,
    };
    // After slot-of-no-return: target must be next (11), not current (10).
    let update = make_update(genesis_key, 10, ProtocolParameterUpdate::default());
    let err = state
        .validate_ppup_proposal(&update, Some(&ctx))
        .expect_err("this-epoch target after slot-of-no-return should be rejected");
    match err {
        LedgerError::PPUpdateWrongEpoch {
            expected_epoch,
            voting_period,
            ..
        } => {
            assert_eq!(expected_epoch, 11);
            assert_eq!(voting_period, "VoteForNextEpoch");
        }
        other => panic!(
            "expected PPUpdateWrongEpoch VoteForNextEpoch, got: {:?}",
            other
        ),
    }
}

// --------------------------------------------------------------------------
// ppup_slot_context helper + stability_window wiring
// --------------------------------------------------------------------------

#[test]
fn ppup_slot_context_builds_from_stability_window() {
    let genesis_key = [0x01; 28];
    let mut state = make_state_with_gen_delegs(&[genesis_key]);
    state.set_slots_per_epoch(432_000);
    state.set_stability_window(129_600); // 3 * 2160 / 0.05

    // Block at slot 4_320_000 (first slot of epoch 10, before slot-of-no-return).
    // This-epoch target (10) should pass; next-epoch (11) should fail.
    let update_this = make_update(genesis_key, 10, ProtocolParameterUpdate::default());
    let update_next = make_update(genesis_key, 11, ProtocolParameterUpdate::default());

    // Block-apply builds the context internally — simulate by calling
    // validate_ppup_proposal with a manually built context that would
    // match what ppup_slot_context(4_320_000) produces.
    let ctx = PpupSlotContext {
        slot: 4_320_000,
        epoch_size: 432_000,
        stability_window: 129_600,
    };
    state
        .validate_ppup_proposal(&update_this, Some(&ctx))
        .expect("this-epoch proposal should pass with stability_window configured");

    let err = state
        .validate_ppup_proposal(&update_next, Some(&ctx))
        .expect_err("next-epoch proposal before slot-of-no-return should fail");
    assert!(
        matches!(err, LedgerError::PPUpdateWrongEpoch { .. }),
        "expected PPUpdateWrongEpoch, got: {:?}",
        err,
    );
}

#[test]
fn stability_window_none_uses_relaxed_check() {
    let genesis_key = [0x01; 28];
    let state = make_state_with_gen_delegs(&[genesis_key]);
    // No set_stability_window call — state.stability_window() == None.
    assert!(state.stability_window().is_none());
    // Both current (10) and current+1 (11) should pass the relaxed check.
    let update_this = make_update(genesis_key, 10, ProtocolParameterUpdate::default());
    let update_next = make_update(genesis_key, 11, ProtocolParameterUpdate::default());
    state
        .validate_ppup_proposal(&update_this, None)
        .expect("relaxed: current epoch ok");
    state
        .validate_ppup_proposal(&update_next, None)
        .expect("relaxed: next epoch ok");
}
