use yggdrasil_consensus::{
    ActiveSlotCoeff, ChainCandidate, ConsensusError, EpochSize, check_is_leader,
    check_leader_value, epoch_first_slot, is_new_epoch, leadership_threshold, select_preferred,
    slot_to_epoch, verify_leader_proof, vrf_input,
};
use yggdrasil_crypto::vrf::{VrfOutput, VrfSecretKey, VRF_SEED_SIZE};
use yggdrasil_ledger::{BlockNo, EpochNo, Nonce, SlotNo};

// ---------------------------------------------------------------------------
// Chain selection
// ---------------------------------------------------------------------------

#[test]
fn prefers_longer_chain_candidate() {
    let left = ChainCandidate {
        block_no: BlockNo(4),
        slot_no: SlotNo(10),
        vrf_tiebreaker: None,
    };
    let right = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(9),
        vrf_tiebreaker: None,
    };
    assert_eq!(select_preferred(left, right), right);
}

#[test]
fn equal_height_prefers_earlier_slot() {
    let left = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(10),
        vrf_tiebreaker: None,
    };
    let right = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(12),
        vrf_tiebreaker: None,
    };
    assert_eq!(select_preferred(left, right), left);
}

#[test]
fn equal_height_equal_slot_vrf_tiebreaker() {
    let left = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(10),
        vrf_tiebreaker: Some([0xFF; 32]),
    };
    let right = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(10),
        vrf_tiebreaker: Some([0x00; 32]),
    };
    // Lower VRF tiebreaker wins.
    assert_eq!(select_preferred(left, right), right);
}

#[test]
fn no_vrf_tiebreaker_defaults_to_left() {
    let left = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(10),
        vrf_tiebreaker: None,
    };
    let right = ChainCandidate {
        block_no: BlockNo(5),
        slot_no: SlotNo(10),
        vrf_tiebreaker: None,
    };
    assert_eq!(select_preferred(left, right), left);
}

// ---------------------------------------------------------------------------
// Epoch math
// ---------------------------------------------------------------------------

#[test]
fn slot_to_epoch_basic() {
    let epoch_size = EpochSize(432_000); // mainnet Shelley epoch length
    assert_eq!(slot_to_epoch(SlotNo(0), epoch_size), EpochNo(0));
    assert_eq!(slot_to_epoch(SlotNo(431_999), epoch_size), EpochNo(0));
    assert_eq!(slot_to_epoch(SlotNo(432_000), epoch_size), EpochNo(1));
}

#[test]
fn epoch_first_slot_round_trip() {
    let epoch_size = EpochSize(100);
    let epoch = EpochNo(3);
    assert_eq!(epoch_first_slot(epoch, epoch_size), SlotNo(300));
    assert_eq!(slot_to_epoch(SlotNo(300), epoch_size), epoch);
}

#[test]
fn is_new_epoch_transitions() {
    let sz = EpochSize(100);
    assert!(is_new_epoch(None, SlotNo(0), sz), "first slot is always a new epoch");
    assert!(!is_new_epoch(Some(SlotNo(0)), SlotNo(99), sz));
    assert!(is_new_epoch(Some(SlotNo(99)), SlotNo(100), sz));
}

// ---------------------------------------------------------------------------
// Leadership threshold
// ---------------------------------------------------------------------------

#[test]
fn computes_nonzero_threshold() {
    let threshold = leadership_threshold(ActiveSlotCoeff(0.05), 0.7)
        .expect("active slot coefficient within bounds should compute a threshold");
    assert!(threshold > 0.0);
    assert!(threshold < 1.0);
}

#[test]
fn full_stake_equals_active_slot_coeff() {
    // With sigma = 1.0 the threshold should be exactly f.
    let f = 0.05;
    let threshold = leadership_threshold(ActiveSlotCoeff(f), 1.0)
        .expect("valid coefficient");
    assert!((threshold - f).abs() < 1e-10);
}

#[test]
fn zero_stake_yields_zero_threshold() {
    let threshold = leadership_threshold(ActiveSlotCoeff(0.05), 0.0)
        .expect("valid coefficient");
    assert!(threshold.abs() < 1e-15);
}

#[test]
fn rejects_invalid_active_slot_coeff() {
    assert_eq!(
        leadership_threshold(ActiveSlotCoeff(-0.1), 0.5),
        Err(ConsensusError::InvalidActiveSlotCoeff)
    );
    assert_eq!(
        leadership_threshold(ActiveSlotCoeff(1.5), 0.5),
        Err(ConsensusError::InvalidActiveSlotCoeff)
    );
}

// ---------------------------------------------------------------------------
// Nonce
// ---------------------------------------------------------------------------

#[test]
fn nonce_neutral_is_identity() {
    let h = Nonce::Hash([0xAB; 32]);
    assert_eq!(h.combine(Nonce::Neutral), h);
    assert_eq!(Nonce::Neutral.combine(h), h);
    assert_eq!(Nonce::Neutral.combine(Nonce::Neutral), Nonce::Neutral);
}

#[test]
fn nonce_combine_is_xor() {
    let a = Nonce::Hash([0xFF; 32]);
    let b = Nonce::Hash([0x0F; 32]);
    let combined = a.combine(b);
    assert_eq!(combined, Nonce::Hash([0xF0; 32]));
}

#[test]
fn nonce_self_combine_yields_zero() {
    let n = Nonce::Hash([0x42; 32]);
    assert_eq!(n.combine(n), Nonce::Hash([0x00; 32]));
}

// ---------------------------------------------------------------------------
// VRF input construction
// ---------------------------------------------------------------------------

#[test]
fn vrf_input_contains_slot_and_nonce() {
    let slot = SlotNo(42);
    let nonce = Nonce::Hash([0xBB; 32]);
    let input = vrf_input(slot, nonce);
    assert_eq!(input.len(), 8 + 32);
    assert_eq!(&input[..8], &42u64.to_be_bytes());
    assert_eq!(&input[8..], &[0xBB; 32]);
}

#[test]
fn vrf_input_neutral_nonce_has_no_nonce_bytes() {
    let input = vrf_input(SlotNo(1), Nonce::Neutral);
    assert_eq!(input.len(), 8);
}

// ---------------------------------------------------------------------------
// VRF leader check (unit)
// ---------------------------------------------------------------------------

#[test]
fn check_leader_value_all_zeros_output_is_leader() {
    // An all-zeros VRF output maps to p ≈ 0 which is below any positive
    // threshold, so it should always be a leader.
    let output = VrfOutput::from_bytes([0u8; 64]);
    let result = check_leader_value(&output, 1.0, ActiveSlotCoeff(0.05))
        .expect("valid coefficient");
    assert!(result, "all-zeros output should be below any positive threshold");
}

#[test]
fn check_leader_value_all_ones_output_is_not_leader() {
    // An all-FF VRF output maps to p ≈ 1 which exceeds the threshold for
    // any stake fraction less than 1.
    let output = VrfOutput::from_bytes([0xFF; 64]);
    let result = check_leader_value(&output, 0.01, ActiveSlotCoeff(0.05))
        .expect("valid coefficient");
    assert!(!result, "all-ones output should exceed threshold for small stake");
}

// ---------------------------------------------------------------------------
// Full leader election round-trip
// ---------------------------------------------------------------------------

#[test]
fn check_is_leader_round_trip() {
    let seed = [0x42u8; VRF_SEED_SIZE];
    let sk = VrfSecretKey::from_seed(seed);
    let vk = sk.verification_key();
    let slot = SlotNo(100);
    let epoch_nonce = Nonce::Hash([0xAA; 32]);
    // Use full stake and high active slot coefficient to guarantee leadership.
    let asc = ActiveSlotCoeff(1.0);
    let sigma = 1.0;

    let result = check_is_leader(&sk, slot, epoch_nonce, sigma, asc)
        .expect("should not error with valid parameters");
    let (_output, proof_bytes) = result.expect("with sigma=1 and f=1, should always be leader");

    // Verify the proof.
    let verified = verify_leader_proof(&vk, slot, epoch_nonce, &proof_bytes, sigma, asc)
        .expect("verification should succeed");
    assert!(verified, "valid proof should pass leader threshold");
}

#[test]
fn verify_leader_proof_rejects_wrong_slot() {
    let seed = [0x77u8; VRF_SEED_SIZE];
    let sk = VrfSecretKey::from_seed(seed);
    let vk = sk.verification_key();
    let slot = SlotNo(50);
    let epoch_nonce = Nonce::Hash([0xCC; 32]);
    let asc = ActiveSlotCoeff(1.0);
    let sigma = 1.0;

    let (_, proof_bytes) = check_is_leader(&sk, slot, epoch_nonce, sigma, asc)
        .expect("valid")
        .expect("leader with f=1,σ=1");

    // Wrong slot should fail VRF verification.
    let result = verify_leader_proof(&vk, SlotNo(51), epoch_nonce, &proof_bytes, sigma, asc);
    assert!(result.is_err(), "proof computed for slot 50 should not verify at slot 51");
}

#[test]
fn verify_leader_proof_rejects_wrong_key() {
    let seed_a = [0x11u8; VRF_SEED_SIZE];
    let seed_b = [0x22u8; VRF_SEED_SIZE];
    let sk_a = VrfSecretKey::from_seed(seed_a);
    let vk_b = VrfSecretKey::from_seed(seed_b).verification_key();
    let slot = SlotNo(10);
    let epoch_nonce = Nonce::Neutral;
    let asc = ActiveSlotCoeff(1.0);
    let sigma = 1.0;

    let (_, proof_bytes) = check_is_leader(&sk_a, slot, epoch_nonce, sigma, asc)
        .expect("valid")
        .expect("leader");

    let result = verify_leader_proof(&vk_b, slot, epoch_nonce, &proof_bytes, sigma, asc);
    assert!(result.is_err(), "proof from key A should not verify with key B");
}

#[test]
fn verify_leader_proof_rejects_truncated_proof() {
    let seed = [0x33u8; VRF_SEED_SIZE];
    let vk = VrfSecretKey::from_seed(seed).verification_key();
    let slot = SlotNo(1);
    let epoch_nonce = Nonce::Neutral;
    let asc = ActiveSlotCoeff(1.0);
    let sigma = 1.0;

    let result = verify_leader_proof(&vk, slot, epoch_nonce, &[0u8; 10], sigma, asc);
    assert_eq!(result, Err(ConsensusError::InvalidVrfProof));
}
