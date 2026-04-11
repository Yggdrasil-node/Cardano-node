#![allow(clippy::unwrap_used)]
use yggdrasil_consensus::{
    ActiveSlotCoeff, ChainCandidate, ChainEntry, ChainState, ConsensusError, EpochSize, Header,
    HeaderBody, NonceDerivation, NonceEvolutionConfig, NonceEvolutionState, OpCert, SecurityParam,
    VrfMode, VrfTiebreakerFlavor, VrfUsage, check_is_leader, check_kes_period,
    check_leader_value, epoch_first_slot, is_new_epoch, kes_period_of_slot,
    leadership_threshold, select_preferred, slot_to_epoch, verify_header, verify_leader_proof,
    verify_opcert_only, vrf_input, vrf_output_to_nonce,
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

const UNRESTRICTED: VrfTiebreakerFlavor = VrfTiebreakerFlavor::UnrestrictedVrfTiebreaker;

fn mk_candidate(block_no: u64, slot: u64, vrf: Option<[u8; 32]>) -> ChainCandidate {
    ChainCandidate {
        block_no: BlockNo(block_no),
        slot_no: SlotNo(slot),
        issuer_vkey_hash: None,
        ocert_issue_no: None,
        vrf_tiebreaker: vrf,
    }
}

#[test]
fn prefers_longer_chain_candidate() {
    let left = mk_candidate(4, 10, None);
    let right = mk_candidate(5, 9, None);
    assert_eq!(select_preferred(left, right, UNRESTRICTED), right);
}

#[test]
fn equal_height_vrf_tiebreaker() {
    // With unrestricted VRF flavor, lower VRF wins regardless of slot.
    let left = mk_candidate(5, 10, Some([0xFF; 32]));
    let right = mk_candidate(5, 12, Some([0x00; 32]));
    assert_eq!(select_preferred(left, right, UNRESTRICTED), right);
}

#[test]
fn equal_height_equal_slot_vrf_tiebreaker() {
    let left = mk_candidate(5, 10, Some([0xFF; 32]));
    let right = mk_candidate(5, 10, Some([0x00; 32]));
    // Lower VRF tiebreaker wins.
    assert_eq!(select_preferred(left, right, UNRESTRICTED), right);
}

#[test]
fn no_vrf_tiebreaker_defaults_to_left() {
    let left = mk_candidate(5, 10, None);
    let right = mk_candidate(5, 10, None);
    assert_eq!(select_preferred(left, right, UNRESTRICTED), left);
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
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let threshold = leadership_threshold(&asc, 0.7);
    assert!(threshold > 0.0);
    assert!(threshold < 1.0);
}

#[test]
fn full_stake_equals_active_slot_coeff() {
    // With sigma = 1.0 the threshold should be exactly f.
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let threshold = leadership_threshold(&asc, 1.0);
    assert!((threshold - 0.05).abs() < 1e-10);
}

#[test]
fn zero_stake_yields_zero_threshold() {
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let threshold = leadership_threshold(&asc, 0.0);
    assert!(threshold.abs() < 1e-15);
}

#[test]
fn rejects_invalid_active_slot_coeff() {
    assert!(ActiveSlotCoeff::new(-0.1).is_err());
    assert!(ActiveSlotCoeff::new(1.5).is_err());
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
fn vrf_input_praos_is_32_byte_hash() {
    let slot = SlotNo(42);
    let nonce = Nonce::Hash([0xBB; 32]);
    // Praos mkInputVRF: Blake2b-256(slot_be8 || nonce_bytes) → 32 bytes.
    let input = vrf_input(slot, nonce, VrfMode::Praos, VrfUsage::Leader);
    assert_eq!(input.len(), 32);
}

#[test]
fn vrf_input_tpraos_is_32_byte_xored_hash() {
    let slot = SlotNo(42);
    let nonce = Nonce::Hash([0xBB; 32]);
    // TPraos mkSeed: Blake2b-256(slot_be8 || nonce_bytes) XOR tag_hash → 32 bytes.
    let input = vrf_input(slot, nonce, VrfMode::TPraos, VrfUsage::Leader);
    assert_eq!(input.len(), 32);
}

#[test]
fn vrf_input_tpraos_neutral_nonce_still_32_bytes() {
    let input = vrf_input(SlotNo(1), Nonce::Neutral, VrfMode::TPraos, VrfUsage::Leader);
    assert_eq!(input.len(), 32);
}

// ---------------------------------------------------------------------------
// VRF leader check (unit)
// ---------------------------------------------------------------------------

#[test]
fn check_leader_value_all_zeros_output_is_leader() {
    // An all-zeros VRF output maps to p = 0 which is below any positive
    // threshold, so it should always be a leader.
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let output = VrfOutput::from_bytes([0u8; 64]);
    let result = check_leader_value(&output, 1, 1, &asc, VrfMode::TPraos)
        .expect("valid");
    assert!(result, "all-zeros output should be below any positive threshold");
}

#[test]
fn check_leader_value_all_ones_output_is_not_leader() {
    // An all-FF VRF output maps to p ≈ max which exceeds the threshold for
    // any stake fraction less than 1.
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let output = VrfOutput::from_bytes([0xFF; 64]);
    let result = check_leader_value(&output, 1, 100, &asc, VrfMode::TPraos)
        .expect("valid");
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
    let asc = ActiveSlotCoeff::from_rational(1, 1).expect("valid");

    let result = check_is_leader(&sk, slot, epoch_nonce, 1, 1, &asc, VrfMode::TPraos)
        .expect("should not error with valid parameters");
    let (_output, proof_bytes) = result.expect("with sigma=1 and f=1, should always be leader");

    // Verify the proof.
    let verified = verify_leader_proof(&vk, slot, epoch_nonce, &proof_bytes, 1, 1, &asc, VrfMode::TPraos)
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
    let asc = ActiveSlotCoeff::from_rational(1, 1).expect("valid");

    let (_, proof_bytes) = check_is_leader(&sk, slot, epoch_nonce, 1, 1, &asc, VrfMode::TPraos)
        .expect("valid")
        .expect("leader with f=1,σ=1");

    // Wrong slot should fail VRF verification.
    let result = verify_leader_proof(&vk, SlotNo(51), epoch_nonce, &proof_bytes, 1, 1, &asc, VrfMode::TPraos);
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
    let asc = ActiveSlotCoeff::from_rational(1, 1).expect("valid");

    let (_, proof_bytes) = check_is_leader(&sk_a, slot, epoch_nonce, 1, 1, &asc, VrfMode::TPraos)
        .expect("valid")
        .expect("leader");

    let result = verify_leader_proof(&vk_b, slot, epoch_nonce, &proof_bytes, 1, 1, &asc, VrfMode::TPraos);
    assert!(result.is_err(), "proof from key A should not verify with key B");
}

#[test]
fn verify_leader_proof_rejects_truncated_proof() {
    let seed = [0x33u8; VRF_SEED_SIZE];
    let vk = VrfSecretKey::from_seed(seed).verification_key();
    let slot = SlotNo(1);
    let epoch_nonce = Nonce::Neutral;
    let asc = ActiveSlotCoeff::from_rational(1, 1).expect("valid");

    let result = verify_leader_proof(&vk, slot, epoch_nonce, &[0u8; 10], 1, 1, &asc, VrfMode::TPraos);
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
        hot_vkey: *hot_vk,
        sequence_number: counter,
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
    opcert.sequence_number = 6;
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
        block_number: BlockNo(1),
        slot: SlotNo(slot),
        prev_hash: Some(HeaderHash([0xAB; 32])),
        issuer_vkey: cold_vk,
        vrf_vkey: vrf_sk.verification_key(),
        leader_vrf_output: vec![0u8; 32],
        leader_vrf_proof: [0u8; 80],
        nonce_vrf_output: None,
        nonce_vrf_proof: None,
        block_body_size: 1024,
        block_body_hash: [0xCD; 32],
        operational_cert: opcert,
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
    header.body.issuer_vkey = wrong_vk;

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
        block_number: BlockNo(1),
        slot: SlotNo(150), // KES period 1 with slots_per_kes = 100
        prev_hash: None,
        issuer_vkey: cold_vk,
        vrf_vkey: vrf_sk.verification_key(),
        leader_vrf_output: vec![0u8; 32],
        leader_vrf_proof: [0u8; 80],
        nonce_vrf_output: None,
        nonce_vrf_proof: None,
        block_body_size: 0,
        block_body_hash: [0; 32],
        operational_cert: opcert,
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
    header.body.block_body_size = 9999;

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
        block_number: BlockNo(100),
        slot: SlotNo(5000),
        prev_hash: Some(HeaderHash([0xEE; 32])),
        issuer_vkey: cold_vk,
        vrf_vkey: vrf_sk.verification_key(),
        leader_vrf_output: vec![0u8; 32],
        leader_vrf_proof: [0u8; 80],
        nonce_vrf_output: None,
        nonce_vrf_proof: None,
        block_body_size: 2048,
        block_body_hash: [0xDD; 32],
        operational_cert: opcert,
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
        block_number: BlockNo(0),
        slot: SlotNo(0),
        prev_hash: None, // genesis
        issuer_vkey: cold_vk,
        vrf_vkey: vrf_sk.verification_key(),
        leader_vrf_output: vec![0u8; 32],
        leader_vrf_proof: [0u8; 80],
        nonce_vrf_output: None,
        nonce_vrf_proof: None,
        block_body_size: 0,
        block_body_hash: [0; 32],
        operational_cert: opcert,
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
        prev_hash: None,
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

// ===========================================================================
// Mainnet protocol parameter parity
// ===========================================================================
//
// These tests verify our consensus primitives against known Cardano mainnet
// protocol constants. Values are sourced from the Shelley genesis file and
// the upstream ouroboros-consensus configuration.
//
// Reference:
//   https://github.com/IntersectMBO/cardano-node/blob/master/configuration/cardano/shelley-genesis.json

/// Mainnet Shelley epoch size is 432,000 slots.
#[test]
fn mainnet_epoch_size() {
    let epoch_size = EpochSize(432_000);
    // First slot of epoch 1.
    assert_eq!(epoch_first_slot(EpochNo(1), epoch_size), SlotNo(432_000));
    // Last slot of epoch 0.
    assert_eq!(slot_to_epoch(SlotNo(431_999), epoch_size), EpochNo(0));
    // First slot of epoch 0.
    assert_eq!(slot_to_epoch(SlotNo(0), epoch_size), EpochNo(0));
    // Epoch transition boundary.
    assert!(is_new_epoch(Some(SlotNo(431_999)), SlotNo(432_000), epoch_size));
    assert!(!is_new_epoch(Some(SlotNo(431_998)), SlotNo(431_999), epoch_size));
}

/// Mainnet security parameter k = 2160.
#[test]
fn mainnet_security_param() {
    let k = SecurityParam(2160);
    let mut cs = ChainState::new(k);
    assert_eq!(cs.security_param(), k);

    // Add 2161 blocks — exactly 1 should be stable.
    for i in 0u64..2161 {
        cs.roll_forward(ChainEntry {
            hash: HeaderHash([0x00; 32]),
            slot: SlotNo(i),
            block_no: BlockNo(i),
            prev_hash: None,
        })
        .expect("forward");
    }
    assert_eq!(cs.stable_count(), 1);
    assert_eq!(cs.volatile_len(), 2161);

    let drained = cs.drain_stable();
    assert_eq!(drained.len(), 1);
    assert_eq!(cs.volatile_len(), 2160);
}

/// Mainnet active slot coefficient is 1/20 = 0.05.
#[test]
fn mainnet_active_slot_coeff() {
    let f = ActiveSlotCoeff::from_rational(1, 20).expect("valid");

    // With full stake, threshold = f.
    let t_full = leadership_threshold(&f, 1.0);
    assert!((t_full - 0.05).abs() < 1e-10);

    // With half stake, threshold = 1 - (1 - 0.05)^0.5 ≈ 0.02532..
    let t_half = leadership_threshold(&f, 0.5);
    let expected = 1.0 - (0.95_f64).powf(0.5);
    assert!((t_half - expected).abs() < 1e-10);
}

/// Mainnet slots per KES period is 129,600 (= 36 hours at 1s slots).
#[test]
fn mainnet_kes_period() {
    let slots_per_kes = 129_600u64;

    // Slot 0 → KES period 0.
    assert_eq!(kes_period_of_slot(0, slots_per_kes).expect("valid"), 0);
    // Last slot in period 0.
    assert_eq!(
        kes_period_of_slot(129_599, slots_per_kes).expect("valid"),
        0
    );
    // First slot of period 1.
    assert_eq!(
        kes_period_of_slot(129_600, slots_per_kes).expect("valid"),
        1
    );
}

/// Mainnet max KES evolutions is 62 (depth-6 SumKES: 2^6 - 2 = 62).
#[test]
fn mainnet_max_kes_evolutions() {
    let max_kes = 62u64;
    let slots_per_kes = 129_600u64;

    // A cert issued at period 0 is valid for periods [0, 62).
    let kes_seed = [0xAA; 32];
    let kes_sk = gen_sum_kes_signing_key(&kes_seed, 0).expect("valid");
    let kes_vk = derive_sum_kes_vk(&kes_sk).expect("valid");
    let cold_sk = SigningKey::from_bytes([0xBB; 32]);
    let opcert = make_opcert(&cold_sk, &kes_vk, 0, 0);

    check_kes_period(&opcert, 0, max_kes).expect("period 0 valid");
    check_kes_period(&opcert, 61, max_kes).expect("period 61 valid");
    assert_eq!(
        check_kes_period(&opcert, 62, max_kes),
        Err(ConsensusError::KesPeriodExpired {
            current: 62,
            cert_end: 62,
        })
    );

    // Integration: slot in the middle of the first epoch.
    let current_period = kes_period_of_slot(500_000, slots_per_kes).expect("valid");
    assert_eq!(current_period, 3); // 500000 / 129600 = 3
}

// ---------------------------------------------------------------------------
// Nonce evolution
// ---------------------------------------------------------------------------

/// Helper: make a deterministic VRF-like output (64 bytes) from a seed byte.
fn make_vrf_output(seed: u8) -> Vec<u8> {
    let mut out = vec![0u8; 64];
    out[0] = seed;
    out
}

/// Helper: make a deterministic header hash from a seed byte.
fn make_header_hash(seed: u8) -> HeaderHash {
    let mut h = [0u8; 32];
    h[0] = seed;
    HeaderHash(h)
}

/// Helper: standard test config — 10-slot epochs, stability window of 3 slots.
fn test_nonce_config() -> NonceEvolutionConfig {
    NonceEvolutionConfig {
        epoch_size: EpochSize(10),
        stability_window: 3,
        extra_entropy: Nonce::Neutral,
    }
}

#[test]
fn vrf_output_to_nonce_hashes_bytes() {
    let output = make_vrf_output(42);
    let nonce = vrf_output_to_nonce(&output);
    // Must produce a Hash variant (not Neutral).
    assert!(matches!(nonce, Nonce::Hash(_)));
    // Two different VRF outputs must yield different nonces.
    let nonce2 = vrf_output_to_nonce(&make_vrf_output(99));
    assert_ne!(nonce, nonce2);
}

#[test]
fn vrf_output_to_nonce_is_deterministic() {
    let output = make_vrf_output(7);
    let n1 = vrf_output_to_nonce(&output);
    let n2 = vrf_output_to_nonce(&output);
    assert_eq!(n1, n2);
}

#[test]
fn initial_state_has_expected_fields() {
    let init = Nonce::Hash([1u8; 32]);
    let state = NonceEvolutionState::new(init);
    assert_eq!(state.evolving_nonce, init);
    assert_eq!(state.candidate_nonce, init);
    assert_eq!(state.epoch_nonce, init);
    assert_eq!(state.prev_hash_nonce, Nonce::Neutral);
    assert_eq!(state.lab_nonce, Nonce::Neutral);
    assert_eq!(state.current_epoch, EpochNo(0));
}

#[test]
fn from_epoch_sets_epoch_and_nonce() {
    let n = Nonce::Hash([0xAB; 32]);
    let state = NonceEvolutionState::from_epoch(EpochNo(5), n);
    assert_eq!(state.current_epoch, EpochNo(5));
    assert_eq!(state.epoch_nonce, n);
    assert_eq!(state.evolving_nonce, n);
    assert_eq!(state.candidate_nonce, n);
}

#[test]
fn single_block_updates_evolving_nonce() {
    let config = test_nonce_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    let vrf_out = make_vrf_output(1);
    let eta = vrf_output_to_nonce(&vrf_out);

    // Slot 0 is in epoch 0, NOT in stability window (0 + 3 < 10).
    state.apply_block(SlotNo(0), &vrf_out, Some(make_header_hash(0xAA)), &config, NonceDerivation::TPraos);

    // evolving_nonce = init ⭒ eta
    assert_eq!(state.evolving_nonce, init.combine(eta));
    // Not in stability window → candidate = evolving.
    assert_eq!(state.candidate_nonce, state.evolving_nonce);
    // lab_nonce = from_header_hash(prev_hash)
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(make_header_hash(0xAA)));
}

#[test]
fn multiple_blocks_accumulate_evolving_nonce() {
    let config = test_nonce_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    let vrf1 = make_vrf_output(1);
    let vrf2 = make_vrf_output(2);
    let eta1 = vrf_output_to_nonce(&vrf1);
    let eta2 = vrf_output_to_nonce(&vrf2);

    state.apply_block(SlotNo(0), &vrf1, Some(make_header_hash(10)), &config, NonceDerivation::TPraos);
    state.apply_block(SlotNo(1), &vrf2, Some(make_header_hash(11)), &config, NonceDerivation::TPraos);

    // evolving = init ⭒ eta1 ⭒ eta2
    assert_eq!(state.evolving_nonce, init.combine(eta1).combine(eta2));
}

#[test]
fn candidate_freezes_in_stability_window() {
    // epoch_size=10, stability_window=3.
    // Stability window starts when slot + 3 >= 10, i.e. slot >= 7.
    let config = test_nonce_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Apply blocks at slots 0..=6 (NOT in stability window).
    for i in 0u8..=6u8 {
        state.apply_block(SlotNo(i as u64), &make_vrf_output(i + 1), Some(make_header_hash(i)), &config, NonceDerivation::TPraos);
    }
    let candidate_before_freeze = state.candidate_nonce;
    let evolving_before_freeze = state.evolving_nonce;
    assert_eq!(candidate_before_freeze, evolving_before_freeze);

    // Slot 7: in stability window (7 + 3 >= 10). Candidate should freeze.
    state.apply_block(SlotNo(7), &make_vrf_output(100), Some(make_header_hash(7)), &config, NonceDerivation::TPraos);
    assert_ne!(state.evolving_nonce, evolving_before_freeze); // evolving still moves
    assert_eq!(state.candidate_nonce, candidate_before_freeze); // candidate frozen

    // Slot 8: still frozen.
    state.apply_block(SlotNo(8), &make_vrf_output(101), Some(make_header_hash(8)), &config, NonceDerivation::TPraos);
    assert_eq!(state.candidate_nonce, candidate_before_freeze);

    // Slot 9: still in stability window (9 + 3 >= 10). Still frozen.
    state.apply_block(SlotNo(9), &make_vrf_output(102), Some(make_header_hash(9)), &config, NonceDerivation::TPraos);
    assert_eq!(state.candidate_nonce, candidate_before_freeze);
}

#[test]
fn epoch_transition_computes_new_epoch_nonce() {
    // epoch_size=10, stability_window=3.
    let config = test_nonce_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Fill epoch 0 with blocks at slots 0..=9.
    for i in 0u8..=9u8 {
        state.apply_block(SlotNo(i as u64), &make_vrf_output(i + 1), Some(make_header_hash(i)), &config, NonceDerivation::TPraos);
    }

    // Record state at end of epoch 0.
    let candidate_at_epoch_end = state.candidate_nonce;
    let prev_hash_at_epoch_end = state.prev_hash_nonce;
    let lab_at_epoch_end = state.lab_nonce;
    assert_eq!(state.current_epoch, EpochNo(0));

    // First block of epoch 1 at slot 10 — triggers TICKN.
    state.apply_block(SlotNo(10), &make_vrf_output(200), Some(make_header_hash(10)), &config, NonceDerivation::TPraos);
    assert_eq!(state.current_epoch, EpochNo(1));

    // epoch_nonce' = candidate ⭒ prev_hash_nonce ⭒ extra_entropy (Neutral)
    let expected_epoch_nonce = candidate_at_epoch_end.combine(prev_hash_at_epoch_end);
    assert_eq!(state.epoch_nonce, expected_epoch_nonce);

    // prev_hash_nonce' = lab_nonce (from end of epoch 0)
    assert_eq!(state.prev_hash_nonce, lab_at_epoch_end);
}

#[test]
fn epoch_transition_with_extra_entropy() {
    let extra = Nonce::Hash([0xFF; 32]);
    let config = NonceEvolutionConfig {
        epoch_size: EpochSize(10),
        stability_window: 3,
        extra_entropy: extra,
    };
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Fill epoch 0.
    for i in 0u8..=9u8 {
        state.apply_block(SlotNo(i as u64), &make_vrf_output(i + 1), Some(make_header_hash(i)), &config, NonceDerivation::TPraos);
    }
    let candidate = state.candidate_nonce;
    let prev_hash = state.prev_hash_nonce;

    // Trigger epoch 1.
    state.apply_block(SlotNo(10), &make_vrf_output(200), Some(make_header_hash(10)), &config, NonceDerivation::TPraos);

    // epoch_nonce' = candidate ⭒ prev_hash ⭒ extra_entropy
    let expected = candidate.combine(prev_hash).combine(extra);
    assert_eq!(state.epoch_nonce, expected);
}

#[test]
fn candidate_unfreezes_in_new_epoch() {
    let config = test_nonce_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Fill epoch 0 with blocks in stability window to freeze candidate.
    for i in 0u8..=9u8 {
        state.apply_block(SlotNo(i as u64), &make_vrf_output(i + 1), Some(make_header_hash(i)), &config, NonceDerivation::TPraos);
    }

    // Epoch 1, slot 10: triggers transition. Slot 10 + 3 < 20 → NOT in
    // stability window → candidate should track evolving again.
    state.apply_block(SlotNo(10), &make_vrf_output(200), Some(make_header_hash(10)), &config, NonceDerivation::TPraos);
    assert_eq!(state.candidate_nonce, state.evolving_nonce);
}

#[test]
fn genesis_prev_hash_sets_lab_nonce_neutral() {
    let config = test_nonce_config();
    let mut state = NonceEvolutionState::new(Nonce::Neutral);

    // None prev_hash → lab_nonce should be Neutral.
    state.apply_block(SlotNo(0), &make_vrf_output(1), None, &config, NonceDerivation::TPraos);
    assert_eq!(state.lab_nonce, Nonce::Neutral);
}

#[test]
fn multi_epoch_nonce_chain() {
    // Verify nonce evolution across 3 epochs.
    let config = test_nonce_config();
    let init = Nonce::Hash([0x42; 32]);
    let mut state = NonceEvolutionState::new(init);

    for epoch in 0u64..3 {
        let base_slot = epoch * 10;
        for offset in 0u64..10 {
            let slot = base_slot + offset;
            state.apply_block(
                SlotNo(slot),
                &make_vrf_output((slot & 0xFF) as u8),
                Some(make_header_hash((slot & 0xFF) as u8)),
                &config,
                NonceDerivation::TPraos,
            );
        }
    }

    assert_eq!(state.current_epoch, EpochNo(2));
    // Each epoch transition should have updated epoch_nonce.
    // Just verify it's not the initial nonce (it evolved).
    assert_ne!(state.epoch_nonce, init);
}

#[test]
fn epoch_nonce_differs_between_epochs() {
    let config = test_nonce_config();
    let init = Nonce::Hash([0u8; 32]);

    // Run two independent states with different block contents to
    // produce different epoch nonces.
    let mut state_a = NonceEvolutionState::new(init);
    let mut state_b = NonceEvolutionState::new(init);

    for i in 0u8..=10u8 {
        state_a.apply_block(SlotNo(i as u64), &make_vrf_output(i), Some(make_header_hash(i)), &config, NonceDerivation::TPraos);
        state_b.apply_block(SlotNo(i as u64), &make_vrf_output(i + 50), Some(make_header_hash(i)), &config, NonceDerivation::TPraos);
    }

    // Different VRF outputs → different epoch nonces.
    assert_ne!(state_a.epoch_nonce, state_b.epoch_nonce);
}

#[test]
fn stability_window_boundary_slot() {
    // Exact boundary: slot + stability_window == first_slot_next_epoch.
    // In the upstream check: `slot + sw < first_slot_next` → at equality
    // the condition is false → we ARE in stability window → candidate frozen.
    let config = test_nonce_config(); // epoch=10, sw=3
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Slot 6: 6 + 3 = 9 < 10 → NOT in stability window.
    state.apply_block(SlotNo(6), &make_vrf_output(1), Some(make_header_hash(1)), &config, NonceDerivation::TPraos);
    assert_eq!(state.candidate_nonce, state.evolving_nonce);

    let candidate_at_6 = state.candidate_nonce;

    // Slot 7: 7 + 3 = 10 >= 10 → IN stability window → candidate frozen.
    state.apply_block(SlotNo(7), &make_vrf_output(2), Some(make_header_hash(2)), &config, NonceDerivation::TPraos);
    assert_ne!(state.evolving_nonce, candidate_at_6); // evolving changed
    assert_eq!(state.candidate_nonce, candidate_at_6); // candidate unchanged
}

#[test]
fn skip_epoch_detects_transition() {
    // If blocks jump from epoch 0 to epoch 2 (skipping epoch 1),
    // a single transition should fire.
    let config = test_nonce_config(); // epoch=10
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    state.apply_block(SlotNo(0), &make_vrf_output(1), Some(make_header_hash(1)), &config, NonceDerivation::TPraos);
    assert_eq!(state.current_epoch, EpochNo(0));

    // Jump to slot 20 (epoch 2).
    state.apply_block(SlotNo(20), &make_vrf_output(2), Some(make_header_hash(2)), &config, NonceDerivation::TPraos);
    assert_eq!(state.current_epoch, EpochNo(2));
    // epoch_nonce should have been updated by the transition.
    assert_ne!(state.epoch_nonce, init);
}

// ---------------------------------------------------------------------------
// Chain selection edge cases
// ---------------------------------------------------------------------------

#[test]
fn select_preferred_reflexivity() {
    // When both candidates are identical, `select_preferred` returns left.
    let c = mk_candidate(42, 100, Some([0xCC; 32]));
    let result = select_preferred(c, c, UNRESTRICTED);
    assert_eq!(result, c);
}

#[test]
fn select_preferred_max_block_no() {
    // Candidates at u64::MAX block height — verify no overflow or panic.
    let left = mk_candidate(u64::MAX, 10, None);
    let right = mk_candidate(u64::MAX, 20, None);
    // Equal block_no, no VRF → incumbent (left) wins.
    assert_eq!(select_preferred(left, right, UNRESTRICTED), left);
}

#[test]
fn select_preferred_max_slot_no() {
    // Equal block_no, different slots, VRF decides, not slot order.
    let left = mk_candidate(5, u64::MAX, Some([0x01; 32]));
    let right = mk_candidate(5, 0, Some([0x00; 32]));
    // Lower VRF wins (right), not lower slot.
    assert_eq!(select_preferred(left, right, UNRESTRICTED), right);
}

// ---------------------------------------------------------------------------
// ActiveSlotCoeff edge cases
// ---------------------------------------------------------------------------

#[test]
fn active_slot_coeff_near_one_rational() {
    // f = 999/1000 — very close to 1 but not equal.
    let asc = ActiveSlotCoeff::from_rational(999, 1000).expect("999/1000 is valid");
    assert!((asc.to_f64() - 0.999).abs() < 1e-9);
}

#[test]
fn active_slot_coeff_one_over_max() {
    // f = 1/u64::MAX — smallest possible positive rational.
    let asc = ActiveSlotCoeff::from_rational(1, u64::MAX).expect("1/u64::MAX is valid");
    assert!(asc.to_f64() > 0.0);
    assert!(asc.to_f64() < 1e-10);
}

// ---------------------------------------------------------------------------
// check_leader_value edge cases
// ---------------------------------------------------------------------------

#[test]
fn leader_check_fractional_stake_with_zeros_output() {
    // sigma = 1/100, all-zeros VRF output → p = 0 which is below any
    // positive threshold, so this should always be a leader.
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let output = VrfOutput::from_bytes([0u8; 64]);
    let result = check_leader_value(&output, 1, 100, &asc, VrfMode::TPraos).expect("valid");
    assert!(result, "all-zeros output with fractional stake should be leader");
}

#[test]
fn leader_check_full_stake_all_ones_output() {
    // sigma = sigma_den (full stake), all-ones VRF output.
    // target = certNatMax - certNat ≈ 0, and the comparison is
    // target > certNatMax × (1-f)^σ. With f=0.05 and full stake,
    // (1-f)^1 = 0.95, so target ≈ 0 < certNatMax × 0.95 → NOT leader.
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let output = VrfOutput::from_bytes([0xFF; 64]);
    let result = check_leader_value(&output, 100, 100, &asc, VrfMode::TPraos).expect("valid");
    assert!(!result, "all-ones output should not be leader even with full stake");
}

#[test]
fn leader_check_sigma_exceeds_denominator() {
    // sigma_num > sigma_den is not a valid stake fraction, but the function
    // should not panic — it just performs the arithmetic.
    let asc = ActiveSlotCoeff::from_rational(1, 20).expect("valid");
    let output = VrfOutput::from_bytes([0x80; 64]);
    let result = check_leader_value(&output, 200, 100, &asc, VrfMode::TPraos);
    // We only care that it doesn't panic; the result can be Ok(true) or Ok(false).
    assert!(result.is_ok(), "sigma_num > sigma_den should not panic");
}

// ---------------------------------------------------------------------------
// ChainState edge cases
// ---------------------------------------------------------------------------

#[test]
fn chain_state_roll_backward_then_forward() {
    // Roll backward to Origin, then build a completely new chain.
    let mut cs = ChainState::new(SecurityParam(5));
    cs.roll_forward(entry(0x01, 10, 0)).expect("forward 0");
    cs.roll_forward(entry(0x02, 20, 1)).expect("forward 1");

    cs.roll_backward(&Point::Origin).expect("rollback to origin");
    assert!(cs.is_empty());

    // Build a new chain from scratch.
    cs.roll_forward(entry(0xA0, 5, 0)).expect("new forward 0");
    cs.roll_forward(entry(0xA1, 15, 1)).expect("new forward 1");
    cs.roll_forward(entry(0xA2, 25, 2)).expect("new forward 2");

    assert_eq!(cs.volatile_len(), 3);
    assert_eq!(
        cs.tip(),
        Point::BlockPoint(SlotNo(25), HeaderHash([0xA2; 32]))
    );
}

#[test]
fn chain_state_drain_stable_after_rollback() {
    // Fill past k, rollback to within k, verify stable_count drops to 0.
    let k = 3u64;
    let mut cs = ChainState::new(SecurityParam(k));

    // Insert 6 blocks: stable_count should be 3 (6 - 3 = 3).
    for i in 0u64..6 {
        cs.roll_forward(entry(i as u8, (i + 1) * 10, i))
            .expect("forward");
    }
    assert_eq!(cs.stable_count(), 3);

    // Roll back to block 4 (remove block 5) — now 5 entries, stable_count = 2.
    cs.roll_backward(&Point::BlockPoint(SlotNo(50), HeaderHash([0x04; 32])))
        .expect("rollback");
    assert_eq!(cs.volatile_len(), 5);

    // Roll back further to block 2 — now 3 entries, stable_count = 0.
    cs.roll_backward(&Point::BlockPoint(SlotNo(30), HeaderHash([0x02; 32])))
        .expect("rollback");
    assert_eq!(cs.volatile_len(), 3);
    assert_eq!(cs.stable_count(), 0);
}

#[test]
fn chain_state_volatile_entries_preserves_order() {
    // Insert 5 entries and verify volatile_entries returns them oldest first.
    let mut cs = ChainState::new(SecurityParam(10));
    for i in 0u64..5 {
        cs.roll_forward(entry(i as u8, (i + 1) * 10, i))
            .expect("forward");
    }

    let entries = cs.volatile_entries();
    assert_eq!(entries.len(), 5);
    for (idx, e) in entries.iter().enumerate() {
        assert_eq!(e.block_no, BlockNo(idx as u64));
        assert_eq!(e.hash, HeaderHash([idx as u8; 32]));
    }
}

#[test]
fn chain_state_k_zero_all_entries_stable() {
    // With SecurityParam(0), every entry is immediately stable.
    let mut cs = ChainState::new(SecurityParam(0));
    cs.roll_forward(entry(0x01, 10, 0)).expect("forward 0");
    assert_eq!(cs.stable_count(), 1);

    cs.roll_forward(entry(0x02, 20, 1)).expect("forward 1");
    assert_eq!(cs.stable_count(), 2);

    let drained = cs.drain_stable();
    assert_eq!(drained.len(), 2);
    assert_eq!(cs.volatile_len(), 0);
}

// ---------------------------------------------------------------------------
// Epoch math edge cases
// ---------------------------------------------------------------------------

#[test]
fn slot_to_epoch_large_slot() {
    // Slot near u64::MAX with mainnet epoch size — should not overflow.
    let epoch_size = EpochSize(432_000);
    let large_slot = SlotNo(u64::MAX - 1);
    let epoch = slot_to_epoch(large_slot, epoch_size);
    // u64::MAX - 1 = 18446744073709551614, / 432000 = 42699869152105
    assert_eq!(epoch, EpochNo((u64::MAX - 1) / 432_000));
}

#[test]
fn is_new_epoch_first_slot_of_next() {
    // The exact boundary between epoch 0 and 1: slot 431999 → epoch 0,
    // slot 432000 → epoch 1.
    let epoch_size = EpochSize(432_000);
    assert!(
        !is_new_epoch(Some(SlotNo(431_998)), SlotNo(431_999), epoch_size),
        "both in epoch 0"
    );
    assert!(
        is_new_epoch(Some(SlotNo(431_999)), SlotNo(432_000), epoch_size),
        "crossing from epoch 0 to 1"
    );
}

#[test]
fn epoch_first_slot_epoch_zero() {
    // Epoch 0 starts at slot 0 for any epoch size.
    assert_eq!(epoch_first_slot(EpochNo(0), EpochSize(432_000)), SlotNo(0));
    assert_eq!(epoch_first_slot(EpochNo(0), EpochSize(10)), SlotNo(0));
    assert_eq!(epoch_first_slot(EpochNo(0), EpochSize(1)), SlotNo(0));
}

// ---------------------------------------------------------------------------
// Nonce evolution edge cases
// ---------------------------------------------------------------------------

#[test]
fn nonce_evolution_consecutive_epochs_without_blocks() {
    // Apply blocks that skip multiple epochs: epoch 0 → epoch 3.
    // Each call to apply_block should trigger one epoch transition.
    let config = test_nonce_config(); // epoch=10, sw=3
    let init = Nonce::Hash([0x11; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Block in epoch 0.
    state.apply_block(SlotNo(0), &make_vrf_output(1), Some(make_header_hash(1)), &config, NonceDerivation::TPraos);
    assert_eq!(state.current_epoch, EpochNo(0));
    let epoch_nonce_0 = state.epoch_nonce;

    // Jump to epoch 3 (slot 30) — triggers a transition.
    state.apply_block(SlotNo(30), &make_vrf_output(2), Some(make_header_hash(2)), &config, NonceDerivation::TPraos);
    assert_eq!(state.current_epoch, EpochNo(3));
    assert_ne!(state.epoch_nonce, epoch_nonce_0, "epoch nonce should change after transition");

    let epoch_nonce_3 = state.epoch_nonce;

    // Jump to epoch 7 (slot 70) — another transition.
    state.apply_block(SlotNo(70), &make_vrf_output(3), Some(make_header_hash(3)), &config, NonceDerivation::TPraos);
    assert_eq!(state.current_epoch, EpochNo(7));
    assert_ne!(state.epoch_nonce, epoch_nonce_3, "epoch nonce should change again");
}

#[test]
fn nonce_evolution_stability_window_exact_boundary() {
    // With epoch_size=10 and stability_window=3:
    // Slot 6: 6 + 3 = 9 < 10 → NOT in stability window (candidate updated).
    // Slot 7: 7 + 3 = 10 >= 10 → IN stability window (candidate frozen).
    // This test verifies the exact boundary at slot 7.
    let config = test_nonce_config(); // epoch=10, sw=3
    let init = Nonce::Hash([0x22; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Slot 6: outside stability window — candidate should be updated.
    state.apply_block(SlotNo(6), &make_vrf_output(0xAA), Some(make_header_hash(0xAA)), &config, NonceDerivation::TPraos);
    let candidate_after_6 = state.candidate_nonce;
    assert_eq!(state.candidate_nonce, state.evolving_nonce, "candidate tracks evolving outside window");

    // Slot 7: exactly at stability window boundary — candidate should freeze.
    state.apply_block(SlotNo(7), &make_vrf_output(0xBB), Some(make_header_hash(0xBB)), &config, NonceDerivation::TPraos);
    assert_eq!(state.candidate_nonce, candidate_after_6, "candidate frozen at boundary");
    assert_ne!(state.evolving_nonce, candidate_after_6, "evolving still evolves");
}

#[test]
fn nonce_evolution_neutral_extra_entropy() {
    // Verify that Nonce::Neutral extra entropy produces the same epoch nonce
    // as the default configuration. Neutral is the identity for combine(),
    // so candidate ⭒ prev_hash ⭒ Neutral == candidate ⭒ prev_hash.
    let config_neutral = NonceEvolutionConfig {
        epoch_size: EpochSize(10),
        stability_window: 3,
        extra_entropy: Nonce::Neutral,
    };
    let config_zero = NonceEvolutionConfig {
        epoch_size: EpochSize(10),
        stability_window: 3,
        extra_entropy: Nonce::Hash([0u8; 32]),
    };
    let init = Nonce::Hash([0x33; 32]);

    let mut state_neutral = NonceEvolutionState::new(init);
    let mut state_zero = NonceEvolutionState::new(init);

    // Apply identical blocks in epoch 0.
    state_neutral.apply_block(SlotNo(0), &make_vrf_output(1), Some(make_header_hash(1)), &config_neutral, NonceDerivation::TPraos);
    state_zero.apply_block(SlotNo(0), &make_vrf_output(1), Some(make_header_hash(1)), &config_zero, NonceDerivation::TPraos);

    // Trigger epoch transition by jumping to epoch 1.
    state_neutral.apply_block(SlotNo(10), &make_vrf_output(2), Some(make_header_hash(2)), &config_neutral, NonceDerivation::TPraos);
    state_zero.apply_block(SlotNo(10), &make_vrf_output(2), Some(make_header_hash(2)), &config_zero, NonceDerivation::TPraos);

    // Both Nonce::Neutral and Nonce::Hash([0;32]) act as identity elements
    // under combine (Neutral by definition, Hash([0;32]) because XOR with
    // zeros is a no-op), so the resulting epoch nonces must be equal.
    assert_eq!(
        state_neutral.epoch_nonce, state_zero.epoch_nonce,
        "Neutral and Hash([0;32]) are both identity for XOR combine"
    );
}

// ---------------------------------------------------------------------------
// Cross-epoch TICKN nonce verification (deterministic values)
// ---------------------------------------------------------------------------

/// Deterministic end-to-end test for nonce evolution across an epoch boundary.
///
/// Applies 4 blocks: two in epoch 0 (before and inside the stability window),
/// then the first block of epoch 1 (triggering TICKN).  Verifies every
/// intermediate nonce field against hand-computed expected values so that any
/// regression in UPDN or TICKN logic is caught immediately.
///
/// Reference: `Cardano.Protocol.TPraos.Rules.Updn` (UPDN),
///            `Cardano.Protocol.TPraos.Rules.Tickn` (TICKN).
#[test]
fn cross_epoch_nonce_deterministic_tickn() {
    let cfg = NonceEvolutionConfig {
        epoch_size: EpochSize(100),
        stability_window: 30,
        extra_entropy: Nonce::Neutral,
    };
    let initial = Nonce::Hash([0xAA; 32]);
    let mut state = NonceEvolutionState::new(initial);

    // ── Block 1: slot 10, epoch 0, outside stability window ─────────
    let vrf1 = [0x42u8; 64];
    let prev1 = HeaderHash([0xDD; 32]);
    state.apply_block(SlotNo(10), &vrf1, Some(prev1), &cfg, NonceDerivation::TPraos);

    let eta1 = vrf_output_to_nonce(&vrf1);
    let expected_evolving_1 = initial.combine(eta1);
    assert_eq!(state.evolving_nonce, expected_evolving_1, "evolving after block 1");
    // Not in stability window ⇒ candidate tracks evolving.
    assert_eq!(state.candidate_nonce, expected_evolving_1, "candidate after block 1");
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(prev1), "lab after block 1");
    assert_eq!(state.epoch_nonce, initial, "epoch_nonce unchanged in same epoch");

    // ── Block 2: slot 50, epoch 0, outside stability window ─────────
    let vrf2 = [0x43u8; 64];
    let prev2 = HeaderHash([0xEE; 32]);
    state.apply_block(SlotNo(50), &vrf2, Some(prev2), &cfg, NonceDerivation::TPraos);

    let eta2 = vrf_output_to_nonce(&vrf2);
    let expected_evolving_2 = expected_evolving_1.combine(eta2);
    assert_eq!(state.evolving_nonce, expected_evolving_2, "evolving after block 2");
    assert_eq!(state.candidate_nonce, expected_evolving_2, "candidate after block 2");
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(prev2), "lab after block 2");

    // ── Block 3: slot 80, epoch 0, INSIDE stability window ──────────
    // slot 80 + stability_window 30 = 110 ≥ 100 (next epoch first slot)
    let vrf3 = [0x44u8; 64];
    let prev3 = HeaderHash([0xFF; 32]);
    state.apply_block(SlotNo(80), &vrf3, Some(prev3), &cfg, NonceDerivation::TPraos);

    let eta3 = vrf_output_to_nonce(&vrf3);
    let expected_evolving_3 = expected_evolving_2.combine(eta3);
    assert_eq!(state.evolving_nonce, expected_evolving_3, "evolving after block 3");
    // Candidate should be FROZEN at its pre-stability-window value.
    assert_eq!(state.candidate_nonce, expected_evolving_2, "candidate frozen in stability window");
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(prev3), "lab after block 3");
    assert_eq!(state.epoch_nonce, initial, "epoch_nonce still unchanged");
    assert_eq!(state.current_epoch, EpochNo(0));

    // ── Block 4: slot 110, epoch 1 — triggers TICKN transition ──────
    let vrf4 = [0x45u8; 64];
    let prev4 = HeaderHash([0x11; 32]);

    // Save pre-transition state for TICKN computation.
    let pre_candidate = state.candidate_nonce;
    let pre_prev_hash_nonce = state.prev_hash_nonce;
    let pre_lab_nonce = state.lab_nonce;

    state.apply_block(SlotNo(110), &vrf4, Some(prev4), &cfg, NonceDerivation::TPraos);

    // TICKN rule:
    //   epoch_nonce' = candidate_nonce ⭒ prev_hash_nonce ⭒ extra_entropy
    //   prev_hash_nonce' = lab_nonce  (from the last block before transition)
    let expected_epoch_nonce = pre_candidate
        .combine(pre_prev_hash_nonce)
        .combine(cfg.extra_entropy);
    assert_eq!(state.epoch_nonce, expected_epoch_nonce, "TICKN epoch_nonce after transition");
    assert_eq!(state.prev_hash_nonce, pre_lab_nonce, "prev_hash_nonce = old lab_nonce");
    assert_eq!(state.current_epoch, EpochNo(1));

    // After transition, UPDN continues for the first block of the new epoch.
    let eta4 = vrf_output_to_nonce(&vrf4);
    let expected_evolving_4 = expected_evolving_3.combine(eta4);
    assert_eq!(state.evolving_nonce, expected_evolving_4, "evolving continues in epoch 1");
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(prev4), "lab updated for block 4");

    // ── Verify the epoch nonce is a concrete Hash value ─────────────
    // Since initial was Hash, candidate was Hash, and prev_hash_nonce was
    // Neutral at transition time, the epoch_nonce = candidate ⭒ Neutral = candidate.
    assert_eq!(
        state.epoch_nonce, pre_candidate,
        "with neutral prev_hash_nonce, epoch_nonce equals frozen candidate"
    );
}

/// Two-transition deterministic test: verifies that prev_hash_nonce from the
/// first epoch carries into the second epoch's TICKN computation.
#[test]
fn two_epoch_transitions_nonce_chain() {
    let cfg = NonceEvolutionConfig {
        epoch_size: EpochSize(100),
        stability_window: 30,
        extra_entropy: Nonce::Neutral,
    };
    let initial = Nonce::Hash([0x01; 32]);
    let mut state = NonceEvolutionState::new(initial);

    // EPOCH 0: one block outside stability window.
    let prev0 = HeaderHash([0xAA; 32]);
    state.apply_block(SlotNo(10), &[0x10; 64], Some(prev0), &cfg, NonceDerivation::TPraos);
    let candidate_epoch0 = state.candidate_nonce;
    let lab_epoch0 = state.lab_nonce;

    // EPOCH 1: triggers first transition.
    let prev1 = HeaderHash([0xBB; 32]);
    state.apply_block(SlotNo(110), &[0x20; 64], Some(prev1), &cfg, NonceDerivation::TPraos);

    // After first transition:
    //   epoch_nonce = candidate_epoch0 ⭒ Neutral ⭒ Neutral = candidate_epoch0
    //   prev_hash_nonce = lab_epoch0
    assert_eq!(state.epoch_nonce, candidate_epoch0, "first transition: epoch = candidate_0");
    assert_eq!(state.prev_hash_nonce, lab_epoch0, "first transition: prev_hash = lab_0");

    // Capture state for second transition.
    let candidate_epoch1 = state.candidate_nonce;
    let lab_epoch1 = state.lab_nonce;

    // EPOCH 2: triggers second transition.
    state.apply_block(SlotNo(210), &[0x30; 64], None, &cfg, NonceDerivation::TPraos);

    // After second transition:
    //   epoch_nonce = candidate_epoch1 ⭒ lab_epoch0 ⭒ Neutral
    //   prev_hash_nonce = lab_epoch1
    let expected = candidate_epoch1.combine(lab_epoch0);
    assert_eq!(state.epoch_nonce, expected, "second transition: epoch = candidate_1 ⭒ lab_0");
    assert_eq!(state.prev_hash_nonce, lab_epoch1, "second transition: prev_hash = lab_1");
    assert_eq!(state.current_epoch, EpochNo(2));
}

/// Verifies that non-neutral extra_entropy is correctly XOR'd into the
/// epoch nonce during TICKN.
#[test]
fn tickn_with_non_neutral_extra_entropy() {
    let extra = Nonce::Hash([0xFF; 32]);
    let cfg = NonceEvolutionConfig {
        epoch_size: EpochSize(100),
        stability_window: 30,
        extra_entropy: extra,
    };
    let initial = Nonce::Hash([0x01; 32]);
    let mut state = NonceEvolutionState::new(initial);

    // One block in epoch 0.
    state.apply_block(SlotNo(10), &[0x10; 64], Some(HeaderHash([0xAA; 32])), &cfg, NonceDerivation::TPraos);
    let candidate = state.candidate_nonce;

    // Transition to epoch 1.
    state.apply_block(SlotNo(100), &[0x20; 64], None, &cfg, NonceDerivation::TPraos);

    // epoch_nonce = candidate ⭒ Neutral (prev_hash_nonce was neutral) ⭒ extra
    let expected = candidate.combine(Nonce::Neutral).combine(extra);
    assert_eq!(state.epoch_nonce, expected, "extra_entropy XOR'd into epoch nonce");
    // Verify it's different from what we'd get with neutral extra_entropy.
    assert_ne!(state.epoch_nonce, candidate, "non-neutral extra changes the result");
}
