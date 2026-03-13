use yggdrasil_consensus::{
    ActiveSlotCoeff, ChainCandidate, ChainEntry, ChainState, ConsensusError, EpochSize, Header,
    HeaderBody, OpCert, SecurityParam, check_is_leader, check_kes_period, check_leader_value,
    epoch_first_slot, is_new_epoch, kes_period_of_slot, leadership_threshold, select_preferred,
    slot_to_epoch, verify_header, verify_leader_proof, verify_opcert_only, vrf_input,
};
use yggdrasil_crypto::ed25519::SigningKey;
use yggdrasil_crypto::sum_kes::{
    derive_sum_kes_vk, gen_sum_kes_signing_key, sign_sum_kes, update_sum_kes,
};
use yggdrasil_crypto::vrf::{VrfOutput, VrfSecretKey, VRF_SEED_SIZE};
use yggdrasil_ledger::{BlockNo, EpochNo, HeaderHash, Nonce, Point, SlotNo};

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

// ---------------------------------------------------------------------------
// OpCert
// ---------------------------------------------------------------------------

/// Helper: create a valid OpCert signed by the given cold key for the given
/// KES verification key.
fn make_opcert(
    cold_sk: &SigningKey,
    hot_vk: &yggdrasil_crypto::sum_kes::SumKesVerificationKey,
    counter: u64,
    kes_period: u64,
) -> OpCert {
    let mut signable = [0u8; 48];
    signable[..32].copy_from_slice(&hot_vk.to_bytes());
    signable[32..40].copy_from_slice(&counter.to_be_bytes());
    signable[40..48].copy_from_slice(&kes_period.to_be_bytes());

    let sigma = cold_sk.sign(&signable).expect("signing should succeed");

    OpCert {
        hot_vk: *hot_vk,
        cert_counter: counter,
        kes_period,
        sigma,
    }
}

#[test]
fn opcert_verify_valid() {
    let cold_sk = SigningKey::from_bytes([0x01; 32]);
    let cold_vk = cold_sk.verification_key().expect("valid vk");

    let kes_seed = [0xAA; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid kes sk");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid kes vk");

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 0);
    opcert
        .verify(&cold_vk)
        .expect("valid cold-key signature should verify");
}

#[test]
fn opcert_verify_wrong_cold_key_fails() {
    let cold_sk = SigningKey::from_bytes([0x01; 32]);
    let wrong_vk = SigningKey::from_bytes([0x02; 32])
        .verification_key()
        .expect("valid vk");

    let kes_seed = [0xBB; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");

    let opcert = make_opcert(&cold_sk, &kes_vk, 1, 100);
    assert_eq!(
        opcert.verify(&wrong_vk),
        Err(ConsensusError::InvalidOpCertSignature),
        "wrong cold key should reject"
    );
}

#[test]
fn opcert_verify_tampered_counter_fails() {
    let cold_sk = SigningKey::from_bytes([0x03; 32]);
    let cold_vk = cold_sk.verification_key().expect("valid");

    let kes_seed = [0xCC; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");

    let mut opcert = make_opcert(&cold_sk, &kes_vk, 5, 0);
    // Tamper with the counter after signing.
    opcert.cert_counter = 6;
    assert_eq!(
        opcert.verify(&cold_vk),
        Err(ConsensusError::InvalidOpCertSignature),
        "tampered counter should invalidate signature"
    );
}

// ---------------------------------------------------------------------------
// KES period checks
// ---------------------------------------------------------------------------

#[test]
fn kes_period_of_slot_basic() {
    assert_eq!(kes_period_of_slot(0, 100).expect("valid"), 0);
    assert_eq!(kes_period_of_slot(99, 100).expect("valid"), 0);
    assert_eq!(kes_period_of_slot(100, 100).expect("valid"), 1);
    assert_eq!(kes_period_of_slot(250, 100).expect("valid"), 2);
}

#[test]
fn kes_period_of_slot_zero_divisor() {
    assert_eq!(
        kes_period_of_slot(42, 0),
        Err(ConsensusError::InvalidSlotsPerKesPeriod)
    );
}

#[test]
fn check_kes_period_valid_window() {
    let kes_seed = [0xDD; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let cold_sk = SigningKey::from_bytes([0x04; 32]);

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 10);
    // max_kes_evolutions = 62 for mainnet depth-6 SumKES (2^6 - 2 = 62... no, 2^6 = 64 periods)
    // KES periods: valid from 10 to 10+64 = 74 (exclusive)
    check_kes_period(&opcert, 10, 64).expect("start of window should be valid");
    check_kes_period(&opcert, 73, 64).expect("last valid period should be valid");
}

#[test]
fn check_kes_period_too_early() {
    let kes_seed = [0xEE; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let cold_sk = SigningKey::from_bytes([0x05; 32]);

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 10);
    assert_eq!(
        check_kes_period(&opcert, 9, 64),
        Err(ConsensusError::KesPeriodTooEarly {
            current: 9,
            cert_start: 10,
        })
    );
}

#[test]
fn check_kes_period_expired() {
    let kes_seed = [0xFF; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let cold_sk = SigningKey::from_bytes([0x06; 32]);

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 10);
    // Window is [10, 74).  Period 74 is past the window.
    assert_eq!(
        check_kes_period(&opcert, 74, 64),
        Err(ConsensusError::KesPeriodExpired {
            current: 74,
            cert_end: 74,
        })
    );
}

// ---------------------------------------------------------------------------
// Full header verification
// ---------------------------------------------------------------------------

/// Helper: build a valid Header (with full OpCert + KES signature chain).
fn make_signed_header(
    cold_sk: &SigningKey,
    kes_seed: &[u8; 32],
    kes_depth: u32,
    slot: u64,
    slots_per_kes_period: u64,
    cert_kes_period: u64,
) -> Header {
    let cold_vk = cold_sk.verification_key().expect("valid cold vk");
    let kes_sk = gen_sum_kes_signing_key(kes_seed, kes_depth).expect("valid kes sk");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid kes vk");
    let vrf_sk = VrfSecretKey::from_seed([0x77; 32]);

    let opcert = make_opcert(cold_sk, &kes_vk, 0, cert_kes_period);

    let body = HeaderBody {
        block_no: BlockNo(1),
        slot_no: SlotNo(slot),
        prev_hash: Some(HeaderHash([0xAB; 32])),
        issuer_vk: cold_vk,
        vrf_vk: vrf_sk.verification_key(),
        body_size: 1024,
        body_hash: [0xCD; 32],
        opcert,
        protocol_version: (10, 0),
    };

    let signable = body.to_signable_bytes();
    let current_kes_period = slot / slots_per_kes_period;
    let kes_offset = (current_kes_period - cert_kes_period) as u32;

    // Evolve the KES key to the target period.
    let mut evolved_sk = kes_sk;
    for p in 0..kes_offset {
        evolved_sk = update_sum_kes(&evolved_sk, p)
            .expect("kes update should succeed")
            .expect("kes key should not be exhausted yet");
    }

    let kes_sig = sign_sum_kes(&evolved_sk, kes_offset, &signable).expect("signing should succeed");

    Header {
        body,
        kes_signature: kes_sig,
    }
}

#[test]
fn verify_header_valid_depth0() {
    // depth-0 SumKES: 1 period, slot falls in KES period 0, cert starts at 0.
    let cold_sk = SigningKey::from_bytes([0x10; 32]);
    let header = make_signed_header(&cold_sk, &[0x20; 32], 0, 50, 100, 0);

    verify_header(&header, 100, 1).expect("valid header should verify");
}

#[test]
fn verify_header_valid_depth2() {
    // depth-2 SumKES: 4 periods. Slot 250 / 100 = KES period 2, cert starts at 0.
    // KES offset = 2 - 0 = 2.
    let cold_sk = SigningKey::from_bytes([0x30; 32]);
    let header = make_signed_header(&cold_sk, &[0x40; 32], 2, 250, 100, 0);

    verify_header(&header, 100, 4).expect("valid depth-2 header should verify");
}

#[test]
fn verify_header_valid_depth6_mainnet_scale() {
    // depth-6 SumKES: 64 periods.  Slot 259200, slots_per_kes = 129600.
    // KES period = 259200 / 129600 = 2. OpCert starts at period 0. KES offset = 2.
    let cold_sk = SigningKey::from_bytes([0x50; 32]);
    let header = make_signed_header(&cold_sk, &[0x60; 32], 6, 259_200, 129_600, 0);

    verify_header(&header, 129_600, 64).expect("valid depth-6 mainnet-scale header should verify");
}

#[test]
fn verify_header_rejects_wrong_cold_key() {
    let cold_sk = SigningKey::from_bytes([0x70; 32]);
    let mut header = make_signed_header(&cold_sk, &[0x80; 32], 0, 50, 100, 0);

    // Swap the issuer_vk with a different cold key.
    let wrong_vk = SigningKey::from_bytes([0x71; 32])
        .verification_key()
        .expect("valid");
    header.body.issuer_vk = wrong_vk;

    assert_eq!(
        verify_header(&header, 100, 1),
        Err(ConsensusError::InvalidOpCertSignature),
    );
}

#[test]
fn verify_header_rejects_expired_kes() {
    // depth-0: only 1 period.  Slot 150 / 100 = KES period 1, cert starts at 0.
    // Window [0, 1), period 1 is past the window.
    let cold_sk = SigningKey::from_bytes([0x90; 32]);

    // Build the header manually at period 0 (valid) but set slot so period = 1.
    let kes_seed = [0xA0; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let cold_vk = cold_sk.verification_key().expect("valid");
    let vrf_sk = VrfSecretKey::from_seed([0xB0; 32]);

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 0);

    let body = HeaderBody {
        block_no: BlockNo(1),
        slot_no: SlotNo(150), // KES period 1 with slots_per_kes = 100
        prev_hash: None,
        issuer_vk: cold_vk,
        vrf_vk: vrf_sk.verification_key(),
        body_size: 0,
        body_hash: [0; 32],
        opcert,
        protocol_version: (10, 0),
    };

    // Sign at period 0 (the only valid one for depth-0).
    let signable = body.to_signable_bytes();
    let kes_sig = sign_sum_kes(&kes_sk, 0, &signable).expect("valid");

    let header = Header {
        body,
        kes_signature: kes_sig,
    };

    assert_eq!(
        verify_header(&header, 100, 1),
        Err(ConsensusError::KesPeriodExpired {
            current: 1,
            cert_end: 1,
        }),
    );
}

#[test]
fn verify_header_rejects_tampered_body() {
    let cold_sk = SigningKey::from_bytes([0xC0; 32]);
    let mut header = make_signed_header(&cold_sk, &[0xD0; 32], 1, 50, 100, 0);

    // Tamper with the body after signing.
    header.body.body_size = 9999;

    assert_eq!(
        verify_header(&header, 100, 2),
        Err(ConsensusError::InvalidKesSignature),
    );
}

#[test]
fn verify_opcert_only_valid() {
    let cold_sk = SigningKey::from_bytes([0xE0; 32]);
    let cold_vk = cold_sk.verification_key().expect("valid");

    let kes_seed = [0xF0; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 0);

    verify_opcert_only(&opcert, &cold_vk, SlotNo(50), 100, 1)
        .expect("valid opcert should pass pre-validation");
}

#[test]
fn header_body_signable_bytes_deterministic() {
    let cold_sk = SigningKey::from_bytes([0x11; 32]);
    let cold_vk = cold_sk.verification_key().expect("valid");
    let kes_seed = [0x22; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let vrf_sk = VrfSecretKey::from_seed([0x33; 32]);

    let opcert = make_opcert(&cold_sk, &kes_vk, 42, 7);

    let body = HeaderBody {
        block_no: BlockNo(100),
        slot_no: SlotNo(5000),
        prev_hash: Some(HeaderHash([0xEE; 32])),
        issuer_vk: cold_vk,
        vrf_vk: vrf_sk.verification_key(),
        body_size: 2048,
        body_hash: [0xDD; 32],
        opcert,
        protocol_version: (10, 1),
    };

    let bytes1 = body.to_signable_bytes();
    let bytes2 = body.to_signable_bytes();
    assert_eq!(bytes1, bytes2, "signable bytes should be deterministic");
    // Check expected length:
    // block_no(8) + slot_no(8) + prev_hash(1+32) + issuer_vk(32) + vrf_vk(32) +
    // body_size(4) + body_hash(32) + opcert_signable(48) + opcert_sigma(64) +
    // protocol_version(16) = 277
    assert_eq!(bytes1.len(), 277);
}

#[test]
fn header_body_signable_bytes_genesis_prev() {
    let cold_sk = SigningKey::from_bytes([0x44; 32]);
    let cold_vk = cold_sk.verification_key().expect("valid");
    let kes_seed = [0x55; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let vrf_sk = VrfSecretKey::from_seed([0x66; 32]);

    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 0);

    let body = HeaderBody {
        block_no: BlockNo(0),
        slot_no: SlotNo(0),
        prev_hash: None, // genesis
        issuer_vk: cold_vk,
        vrf_vk: vrf_sk.verification_key(),
        body_size: 0,
        body_hash: [0; 32],
        opcert,
        protocol_version: (1, 0),
    };

    let bytes = body.to_signable_bytes();
    // Same as above but prev_hash is None: tag byte only (1 byte instead of 33).
    // 8 + 8 + 1 + 32 + 32 + 4 + 32 + 48 + 64 + 16 = 245
    assert_eq!(bytes.len(), 245);
}

// ---------------------------------------------------------------------------
// Chain state tracking
// ---------------------------------------------------------------------------

fn entry(fill: u8, slot: u64, block_no: u64) -> ChainEntry {
    ChainEntry {
        hash: HeaderHash([fill; 32]),
        slot: SlotNo(slot),
        block_no: BlockNo(block_no),
    }
}

#[test]
fn chain_state_starts_at_origin() {
    let cs = ChainState::new(SecurityParam(3));
    assert_eq!(cs.tip(), Point::Origin);
    assert!(cs.is_empty());
    assert_eq!(cs.volatile_len(), 0);
    assert_eq!(cs.stable_count(), 0);
}

#[test]
fn chain_state_roll_forward_and_tip() {
    let mut cs = ChainState::new(SecurityParam(3));
    cs.roll_forward(entry(0x01, 10, 0)).expect("append 0");
    cs.roll_forward(entry(0x02, 20, 1)).expect("append 1");

    assert_eq!(
        cs.tip(),
        Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32]))
    );
    assert_eq!(cs.tip_block_no(), Some(BlockNo(1)));
    assert_eq!(cs.volatile_len(), 2);
}

#[test]
fn chain_state_rejects_non_contiguous_block() {
    let mut cs = ChainState::new(SecurityParam(3));
    cs.roll_forward(entry(0x01, 10, 0)).expect("first");
    let err = cs.roll_forward(entry(0x02, 20, 5)).expect_err("gap");
    assert_eq!(
        err,
        ConsensusError::NonContiguousBlock {
            expected: 1,
            got: 5,
        }
    );
}

#[test]
fn chain_state_rollback_to_point() {
    let mut cs = ChainState::new(SecurityParam(5));
    for i in 0u64..4 {
        cs.roll_forward(entry(i as u8, (i + 1) * 10, i))
            .expect("forward");
    }

    // Roll back to entry 1 (block 1, slot 20).
    cs.roll_backward(&Point::BlockPoint(SlotNo(20), HeaderHash([0x01; 32])))
        .expect("rollback");

    assert_eq!(
        cs.tip(),
        Point::BlockPoint(SlotNo(20), HeaderHash([0x01; 32]))
    );
    assert_eq!(cs.volatile_len(), 2);
}

#[test]
fn chain_state_rollback_to_origin() {
    let mut cs = ChainState::new(SecurityParam(5));
    cs.roll_forward(entry(0x01, 10, 0)).expect("forward");
    cs.roll_forward(entry(0x02, 20, 1)).expect("forward");

    cs.roll_backward(&Point::Origin).expect("rollback to origin");
    assert_eq!(cs.tip(), Point::Origin);
    assert!(cs.is_empty());
}

#[test]
fn chain_state_rollback_too_deep() {
    let mut cs = ChainState::new(SecurityParam(2));
    for i in 0u64..5 {
        cs.roll_forward(entry(i as u8, (i + 1) * 10, i))
            .expect("forward");
    }

    // Rollback to origin requires removing 5 blocks, but k=2.
    let err = cs
        .roll_backward(&Point::Origin)
        .expect_err("too deep");
    assert_eq!(
        err,
        ConsensusError::RollbackTooDeep {
            requested: 5,
            max: 2,
        }
    );
}

#[test]
fn chain_state_rollback_point_not_found() {
    let mut cs = ChainState::new(SecurityParam(5));
    cs.roll_forward(entry(0x01, 10, 0)).expect("forward");

    let err = cs
        .roll_backward(&Point::BlockPoint(SlotNo(99), HeaderHash([0xFF; 32])))
        .expect_err("not found");
    assert_eq!(
        err,
        ConsensusError::RollbackPointNotFound {
            slot: 99,
            hash: HeaderHash([0xFF; 32]),
        }
    );
}

#[test]
fn chain_state_stable_entries_drain() {
    let k = 3;
    let mut cs = ChainState::new(SecurityParam(k));

    // Add 5 blocks — with k=3, the first 2 should be stable.
    for i in 0u64..5 {
        cs.roll_forward(entry(i as u8, (i + 1) * 10, i))
            .expect("forward");
    }

    assert_eq!(cs.stable_count(), 2);
    assert_eq!(cs.volatile_len(), 5);

    let drained = cs.drain_stable();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].hash, HeaderHash([0x00; 32]));
    assert_eq!(drained[1].hash, HeaderHash([0x01; 32]));

    // After draining, volatile should have exactly k entries.
    assert_eq!(cs.volatile_len(), 3);
    assert_eq!(cs.stable_count(), 0);
}

#[test]
fn chain_state_drain_stable_empty_when_below_k() {
    let mut cs = ChainState::new(SecurityParam(10));
    cs.roll_forward(entry(0x01, 10, 0)).expect("forward");
    cs.roll_forward(entry(0x02, 20, 1)).expect("forward");

    assert_eq!(cs.stable_count(), 0);
    let drained = cs.drain_stable();
    assert!(drained.is_empty());
}

#[test]
fn chain_state_roll_forward_after_rollback() {
    let mut cs = ChainState::new(SecurityParam(5));
    cs.roll_forward(entry(0x01, 10, 0)).expect("forward 0");
    cs.roll_forward(entry(0x02, 20, 1)).expect("forward 1");
    cs.roll_forward(entry(0x03, 30, 2)).expect("forward 2");

    // Roll back to block 1.
    cs.roll_backward(&Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32])))
        .expect("rollback");

    // Fork from block 1 with a different block 2.
    cs.roll_forward(entry(0xAA, 25, 2)).expect("fork forward");
    assert_eq!(
        cs.tip(),
        Point::BlockPoint(SlotNo(25), HeaderHash([0xAA; 32]))
    );
    assert_eq!(cs.volatile_len(), 3);
}

#[test]
fn chain_state_security_param_accessor() {
    let cs = ChainState::new(SecurityParam(42));
    assert_eq!(cs.security_param(), SecurityParam(42));
}
