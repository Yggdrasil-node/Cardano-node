#![allow(clippy::unwrap_used)]
use yggdrasil_crypto::vrf::VRF_SEED_SIZE;
use yggdrasil_crypto::{
    Blake2bHash,
    CompactKesSignature,
    CryptoError,
    KesPeriod,
    KesSigningKey,
    Signature,
    SigningKey,
    SimpleCompactKesSignature,
    SimpleKesSignature,
    SimpleKesSigningKey,
    SimpleKesVerificationKey,
    // SumKES
    SumKesSignature,
    SumKesVerificationKey,
    VerificationKey,
    VrfBatchCompatProof,
    VrfOutput,
    VrfProof,
    VrfSecretKey,
    VrfVerificationKey,
    derive_sum_kes_vk,
    ed25519_rfc8032_vectors,
    gen_sum_kes_signing_key,
    hash_bytes,
    sign_sum_kes,
    simple_kes_two_period_test_vectors,
    update_sum_kes,
    verify_sum_kes,
    vrf_praos_batchcompat_test_vectors,
    vrf_praos_test_vectors,
};

#[test]
fn blake2b_hash_is_deterministic() {
    let first = hash_bytes(b"yggdrasil");
    let second = hash_bytes(b"yggdrasil");

    assert_eq!(first, second);
    assert_ne!(first, Blake2bHash([0_u8; 64]));
}

#[test]
fn ed25519_round_trip_sign_and_verify() {
    let signing_key = SigningKey::from_bytes([7_u8; 32]);
    let message = b"yggdrasil-ed25519";
    let verification_key = signing_key
        .verification_key()
        .expect("verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign(message)
        .expect("signing should succeed for a 32-byte seed");

    verification_key
        .verify(message, &signature)
        .expect("freshly produced signature should verify");
}

#[test]
fn ed25519_signing_key_equality_is_byte_exact() {
    let left = SigningKey::from_bytes([7_u8; 32]);
    let same = SigningKey::from_bytes([7_u8; 32]);
    let different = SigningKey::from_bytes([8_u8; 32]);

    assert_eq!(left, same);
    assert_ne!(left, different);
}

#[test]
fn ed25519_rejects_modified_message() {
    let signing_key = SigningKey::from_bytes([9_u8; 32]);
    let verification_key = signing_key
        .verification_key()
        .expect("verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign(b"original")
        .expect("signing should succeed for a 32-byte seed");

    let error = verification_key
        .verify(b"modified", &signature)
        .expect_err("signature verification should fail for a modified message");

    assert_eq!(error, CryptoError::SignatureVerificationFailed);
}

#[test]
fn ed25519_matches_rfc8032_test_vectors() {
    for vector in ed25519_rfc8032_vectors() {
        let signing_key = SigningKey::from_bytes(vector.secret_key);
        let derived_verification_key = signing_key
            .verification_key()
            .expect("verification key derivation should succeed for vector seed");
        let expected_verification_key = VerificationKey::from_bytes(vector.public_key);
        let expected_signature = Signature::from_bytes(vector.signature);

        assert_eq!(
            derived_verification_key, expected_verification_key,
            "public key mismatch for {}",
            vector.name
        );

        let signature = signing_key
            .sign(&vector.message)
            .expect("signing should succeed for vector seed");

        assert_eq!(
            signature, expected_signature,
            "signature mismatch for {}",
            vector.name
        );

        derived_verification_key
            .verify(&vector.message, &expected_signature)
            .expect("RFC 8032 signature should verify");
    }
}

#[test]
fn praos_vrf_vectors_match_embedded_key_layout_and_output_hash() {
    for vector in vrf_praos_test_vectors() {
        let seed: [u8; 32] = vector.secret_key[..32]
            .try_into()
            .expect("Praos vector seeds should be 32 bytes");
        let signing_key = VrfSecretKey::from_seed(seed);
        let verification_key = signing_key.verification_key();
        let proof = VrfProof::from_bytes(vector.proof);
        let expected_output = VrfOutput::from_bytes(vector.output);

        assert_eq!(
            signing_key.to_bytes(),
            vector.secret_key,
            "signing key mismatch for {}",
            vector.name
        );
        assert_eq!(
            signing_key.seed_bytes(),
            vector.secret_key[..32],
            "seed prefix mismatch for {}",
            vector.name
        );
        assert_eq!(
            verification_key.to_bytes(),
            vector.public_key,
            "verification key mismatch for {}",
            vector.name
        );
        assert_eq!(
            proof.output().expect("published Praos proof should decode"),
            expected_output,
            "output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn batchcompat_vrf_vectors_match_embedded_key_layout_and_output_hash() {
    for vector in vrf_praos_batchcompat_test_vectors() {
        let seed: [u8; 32] = vector.secret_key[..32]
            .try_into()
            .expect("Batch-compatible Praos vector seeds should be 32 bytes");
        let signing_key = VrfSecretKey::from_seed(seed);
        let verification_key = signing_key.verification_key();
        let proof = VrfBatchCompatProof::from_bytes(vector.proof);
        let expected_output = VrfOutput::from_bytes(vector.output);

        assert_eq!(
            signing_key.to_bytes(),
            vector.secret_key,
            "signing key mismatch for {}",
            vector.name
        );
        assert_eq!(
            verification_key.to_bytes(),
            vector.public_key,
            "verification key mismatch for {}",
            vector.name
        );
        assert_eq!(
            proof
                .output()
                .expect("published batch-compatible Praos proof should decode"),
            expected_output,
            "output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn standard_vrf_verify_accepts_published_vectors() {
    for vector in vrf_praos_test_vectors() {
        let verification_key = VrfVerificationKey::from_bytes(vector.public_key);
        let proof = VrfProof::from_bytes(vector.proof);
        let expected_output = VrfOutput::from_bytes(vector.output);

        let verified_output = verification_key
            .verify(&vector.message, &proof)
            .expect("published standard Praos proof should verify");

        assert_eq!(
            verified_output, expected_output,
            "verified output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn batchcompat_vrf_verify_accepts_published_vectors() {
    for vector in vrf_praos_batchcompat_test_vectors() {
        let verification_key = VrfVerificationKey::from_bytes(vector.public_key);
        let proof = VrfBatchCompatProof::from_bytes(vector.proof);
        let expected_output = VrfOutput::from_bytes(vector.output);

        let verified_output = verification_key
            .verify_batchcompat(&vector.message, &proof)
            .expect("published batch-compatible Praos proof should verify");

        assert_eq!(
            verified_output, expected_output,
            "verified output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn batchcompat_vrf_verify_rejects_modified_message() {
    let vector = vrf_praos_batchcompat_test_vectors()
        .into_iter()
        .next()
        .expect("at least one batch-compatible Praos VRF vector should be available");
    let verification_key = VrfVerificationKey::from_bytes(vector.public_key);
    let proof = VrfBatchCompatProof::from_bytes(vector.proof);
    let error = verification_key
        .verify_batchcompat(b"modified", &proof)
        .expect_err("batch-compatible VRF verification should fail for a modified message");

    assert_eq!(error, CryptoError::InvalidVrfProof);
}

#[test]
fn standard_vrf_prove_produces_byte_exact_proofs() {
    for vector in vrf_praos_test_vectors() {
        let signing_key = VrfSecretKey::from_bytes(vector.secret_key);
        let (output, proof) = signing_key
            .prove(&vector.message)
            .expect("proof generation should succeed for published vector");

        assert_eq!(
            proof.to_bytes(),
            vector.proof,
            "proof bytes mismatch for {}",
            vector.name
        );
        assert_eq!(
            output.to_bytes(),
            vector.output,
            "output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn standard_vrf_prove_then_verify_round_trips() {
    for vector in vrf_praos_test_vectors() {
        let signing_key = VrfSecretKey::from_bytes(vector.secret_key);
        let verification_key = signing_key.verification_key();
        let (output, proof) = signing_key
            .prove(&vector.message)
            .expect("proof generation should succeed");

        let verified_output = verification_key
            .verify(&vector.message, &proof)
            .expect("verify should accept a freshly generated proof");

        assert_eq!(
            verified_output, output,
            "round-trip output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn batchcompat_vrf_prove_produces_byte_exact_proofs() {
    for vector in vrf_praos_batchcompat_test_vectors() {
        let signing_key = VrfSecretKey::from_bytes(vector.secret_key);
        let (output, proof) = signing_key
            .prove_batchcompat(&vector.message)
            .expect("batchcompat proof generation should succeed for published vector");

        assert_eq!(
            proof.to_bytes(),
            vector.proof,
            "batchcompat proof bytes mismatch for {}",
            vector.name
        );
        assert_eq!(
            output.to_bytes(),
            vector.output,
            "batchcompat output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn batchcompat_vrf_prove_then_verify_round_trips() {
    for vector in vrf_praos_batchcompat_test_vectors() {
        let signing_key = VrfSecretKey::from_bytes(vector.secret_key);
        let verification_key = signing_key.verification_key();
        let (output, proof) = signing_key
            .prove_batchcompat(&vector.message)
            .expect("batchcompat proof generation should succeed");

        let verified_output = verification_key
            .verify_batchcompat(&vector.message, &proof)
            .expect("verify_batchcompat should accept a freshly generated proof");

        assert_eq!(
            verified_output, output,
            "batchcompat round-trip output mismatch for {}",
            vector.name
        );
    }
}

#[test]
fn standard_vrf_verify_rejects_modified_message() {
    let vector = vrf_praos_test_vectors()
        .into_iter()
        .next()
        .expect("at least one standard Praos VRF vector should be available");
    let verification_key = VrfVerificationKey::from_bytes(vector.public_key);
    let proof = VrfProof::from_bytes(vector.proof);
    let error = verification_key
        .verify(b"modified", &proof)
        .expect_err("standard VRF verification should fail for a modified message");

    assert_eq!(error, CryptoError::InvalidVrfProof);
}

#[test]
fn standard_vrf_verify_rejects_tampered_proof_components() {
    let vector = vrf_praos_test_vectors()
        .into_iter()
        .next()
        .expect("at least one standard Praos VRF vector should be available");
    let vk = VrfVerificationKey::from_bytes(vector.public_key);

    // Flip a bit in each proof region: gamma (byte 0), challenge (byte 32), response (byte 48).
    for flip_offset in [0_usize, 32, 48] {
        let mut tampered = vector.proof;
        tampered[flip_offset] ^= 0x01;
        let result = vk.verify(&vector.message, &VrfProof::from_bytes(tampered));
        assert!(
            result.is_err(),
            "standard proof tampered at byte {flip_offset} should be rejected"
        );
    }
}

#[test]
fn batchcompat_vrf_verify_rejects_tampered_proof_components() {
    let vector = vrf_praos_batchcompat_test_vectors()
        .into_iter()
        .next()
        .expect("at least one batchcompat VRF vector should be available");
    let vk = VrfVerificationKey::from_bytes(vector.public_key);

    // Flip a bit in each proof region: gamma (byte 0), ann1 (byte 32), ann2 (byte 64), response (byte 96).
    for flip_offset in [0_usize, 32, 64, 96] {
        let mut tampered = vector.proof;
        tampered[flip_offset] ^= 0x01;
        let result =
            vk.verify_batchcompat(&vector.message, &VrfBatchCompatProof::from_bytes(tampered));
        assert!(
            result.is_err(),
            "batchcompat proof tampered at byte {flip_offset} should be rejected"
        );
    }
}

#[test]
fn vrf_validate_accepts_published_proofs() {
    for vector in vrf_praos_test_vectors() {
        let proof = VrfProof::from_bytes(vector.proof);
        proof
            .validate()
            .expect("published Praos proof should pass structural validation");
    }

    for vector in vrf_praos_batchcompat_test_vectors() {
        let proof = VrfBatchCompatProof::from_bytes(vector.proof);
        proof
            .validate()
            .expect("published batch-compatible Praos proof should pass structural validation");
    }
}

#[test]
fn vrf_validate_rejects_malformed_proofs() {
    let praos_error = VrfProof::from_bytes([0xff; 80])
        .validate()
        .expect_err("invalid Praos proof bytes should fail structural validation");
    let batch_error = VrfBatchCompatProof::from_bytes([0xff; 128])
        .validate()
        .expect_err("invalid batch-compatible proof bytes should fail structural validation");

    assert_eq!(praos_error, CryptoError::InvalidVrfProof);
    assert_eq!(batch_error, CryptoError::InvalidVrfProof);
}

#[test]
fn vrf_verification_key_validate_accepts_published_keys() {
    for vector in vrf_praos_test_vectors() {
        let verification_key = VrfVerificationKey::from_bytes(vector.public_key);
        verification_key
            .validate()
            .expect("published Praos VRF verification keys should validate");
    }

    for vector in vrf_praos_batchcompat_test_vectors() {
        let verification_key = VrfVerificationKey::from_bytes(vector.public_key);
        verification_key
            .validate()
            .expect("published batch-compatible VRF verification keys should validate");
    }
}

#[test]
fn vrf_verification_key_validate_rejects_invalid_bytes() {
    let error = VrfVerificationKey::from_bytes([0_u8; 32])
        .validate()
        .expect_err("identity-style VRF verification key bytes should be rejected");

    assert_eq!(error, CryptoError::InvalidVrfVerificationKey);
}

#[test]
fn vrf_verify_rejects_invalid_key_bytes() {
    let proof = VrfProof::from_bytes([0_u8; 80]);
    let error = VrfVerificationKey::from_bytes([0_u8; 32])
        .verify(b"", &proof)
        .expect_err("invalid VRF key bytes should fail before full verification path");

    assert_eq!(error, CryptoError::InvalidVrfVerificationKey);
}

#[test]
fn vrf_verify_rejects_invalid_proof_bytes() {
    let vector = vrf_praos_test_vectors()
        .into_iter()
        .next()
        .expect("at least one Praos VRF vector should be available");
    let error = VrfVerificationKey::from_bytes(vector.public_key)
        .verify(&vector.message, &VrfProof::from_bytes([0xff; 80]))
        .expect_err("invalid VRF proof bytes should fail before full verification path");

    assert_eq!(error, CryptoError::InvalidVrfProof);
}

#[test]
fn vrf_output_rejects_invalid_proof_bytes() {
    let error = VrfProof::from_bytes([0xff; 80])
        .output()
        .expect_err("nonsensical proof bytes should be rejected");

    assert_eq!(error, CryptoError::InvalidVrfProof);
}

#[test]
fn vrf_from_bytes_normalizes_to_seed_derived_layout() {
    let vector = vrf_praos_test_vectors()
        .into_iter()
        .next()
        .expect("at least one Praos VRF vector should be available");
    let signing_key = VrfSecretKey::from_bytes(vector.secret_key);

    assert_eq!(signing_key.normalized().to_bytes(), vector.secret_key);
}

#[test]
fn vrf_secret_key_equality_is_byte_exact() {
    let left = VrfSecretKey::from_seed([1_u8; 32]);
    let same = VrfSecretKey::from_seed([1_u8; 32]);
    let different = VrfSecretKey::from_seed([2_u8; 32]);

    assert_eq!(left, same);
    assert_ne!(left, different);
}

#[test]
fn vrf_signing_key_validate_accepts_seed_derived_layout() {
    let vector = vrf_praos_test_vectors()
        .into_iter()
        .next()
        .expect("at least one Praos VRF vector should be available");

    VrfSecretKey::from_bytes(vector.secret_key)
        .validate()
        .expect("seed-derived Praos signing key layout should validate");
}

#[test]
fn vrf_signing_key_validate_rejects_mismatched_embedded_key() {
    let mut malformed = VrfSecretKey::from_seed([3_u8; 32]).to_bytes();
    malformed[VRF_SEED_SIZE] ^= 0x01;

    let error = VrfSecretKey::from_bytes(malformed)
        .validate()
        .expect_err("malformed signing key layout should be rejected");

    assert_eq!(error, CryptoError::InvalidVrfSigningKey);
}

#[test]
fn single_kes_round_trip_sign_and_verify() {
    let signing_key = KesSigningKey::from_bytes([11_u8; 32]);
    let verification_key = signing_key
        .verification_key()
        .expect("KES verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign(KesPeriod(0), b"yggdrasil-kes")
        .expect("single-period KES signing should succeed at period zero");

    verification_key
        .verify(KesPeriod(0), b"yggdrasil-kes", &signature)
        .expect("single-period KES signatures should verify at period zero");
}

#[test]
fn kes_signing_key_equality_is_byte_exact() {
    let left = KesSigningKey::from_bytes([11_u8; 32]);
    let same = KesSigningKey::from_bytes([11_u8; 32]);
    let different = KesSigningKey::from_bytes([12_u8; 32]);

    assert!(left == same);
    assert!(left != different);
}

#[test]
fn single_kes_rejects_invalid_periods() {
    let signing_key = KesSigningKey::from_bytes([13_u8; 32]);
    let verification_key = signing_key
        .verification_key()
        .expect("KES verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign(KesPeriod(0), b"period-zero")
        .expect("period zero signing should succeed");

    let sign_error = signing_key
        .sign(KesPeriod(1), b"period-one")
        .expect_err("single-period KES should reject signing beyond period zero");
    let verify_error = verification_key
        .verify(KesPeriod(1), b"period-zero", &signature)
        .expect_err("single-period KES should reject verification beyond period zero");

    assert_eq!(sign_error, CryptoError::InvalidKesPeriod(1));
    assert_eq!(verify_error, CryptoError::InvalidKesPeriod(1));
}

#[test]
fn single_kes_rejects_modified_message() {
    let signing_key = KesSigningKey::from_bytes([17_u8; 32]);
    let verification_key = signing_key
        .verification_key()
        .expect("KES verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign(KesPeriod(0), b"original")
        .expect("period zero signing should succeed");

    let error = verification_key
        .verify(KesPeriod(0), b"modified", &signature)
        .expect_err("single-period KES should reject modified messages");

    assert_eq!(error, CryptoError::SignatureVerificationFailed);
}

#[test]
fn compact_single_kes_round_trip_sign_and_verify() {
    let signing_key = KesSigningKey::from_bytes([19_u8; 32]);
    let verification_key = signing_key
        .verification_key()
        .expect("compact KES verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign_compact(KesPeriod(0), b"yggdrasil-compact-kes")
        .expect("compact single-period KES signing should succeed at period zero");

    assert_eq!(signature.verification_key(), verification_key);

    verification_key
        .verify_compact(KesPeriod(0), b"yggdrasil-compact-kes", &signature)
        .expect("compact single-period KES signatures should verify at period zero");
}

#[test]
fn compact_single_kes_rejects_mismatched_key() {
    let signing_key = KesSigningKey::from_bytes([21_u8; 32]);
    let other_key = KesSigningKey::from_bytes([22_u8; 32])
        .verification_key()
        .expect("KES verification key derivation should succeed for a 32-byte seed");
    let signature = signing_key
        .sign_compact(KesPeriod(0), b"compact")
        .expect("compact period zero signing should succeed");

    let error = other_key
        .verify_compact(KesPeriod(0), b"compact", &signature)
        .expect_err("compact KES verification should reject mismatched external keys");

    assert_eq!(error, CryptoError::KesVerificationKeyMismatch);
}

#[test]
fn compact_single_kes_from_bytes_round_trips() {
    let signing_key = KesSigningKey::from_bytes([23_u8; 32]);
    let signature = signing_key
        .sign_compact(KesPeriod(0), b"bytes")
        .expect("compact period zero signing should succeed");
    let decoded = CompactKesSignature::from_bytes(signature.to_bytes());
    let compact_bytes = signature.to_bytes();

    assert_eq!(decoded, signature);
    assert_eq!(
        decoded.signature().to_bytes().as_slice(),
        &compact_bytes[..64]
    );
}

#[test]
fn simple_kes_round_trip_sign_and_verify_across_periods() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[31_u8; 32], [32_u8; 32], [33_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");

    for period in [KesPeriod(0), KesPeriod(1), KesPeriod(2)] {
        let signature = signing_key
            .sign(period, b"simple-kes")
            .expect("SimpleKES should sign within range");
        verification_key
            .verify(period, b"simple-kes", &signature)
            .expect("SimpleKES should verify within range");
    }
}

#[test]
fn simple_kes_rejects_out_of_range_periods() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[41_u8; 32], [42_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");
    let signature = signing_key
        .sign(KesPeriod(1), b"in-range")
        .expect("period one should be in-range for depth two");

    let sign_error = signing_key
        .sign(KesPeriod(2), b"out-of-range")
        .expect_err("SimpleKES should reject out-of-range signing periods");
    let verify_error = verification_key
        .verify(KesPeriod(2), b"in-range", &signature)
        .expect_err("SimpleKES should reject out-of-range verification periods");

    assert_eq!(sign_error, CryptoError::InvalidKesPeriod(2));
    assert_eq!(verify_error, CryptoError::InvalidKesPeriod(2));
}

#[test]
fn simple_kes_signing_and_verification_keys_round_trip_bytes() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[51_u8; 32], [52_u8; 32], [53_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let signing_bytes = signing_key.to_bytes();
    let signing_decoded = SimpleKesSigningKey::from_bytes(&signing_bytes)
        .expect("SimpleKES signing key bytes should round-trip");

    assert_eq!(signing_decoded, signing_key);

    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");
    let verification_bytes = verification_key.to_bytes();
    let verification_decoded = SimpleKesVerificationKey::from_bytes(&verification_bytes)
        .expect("SimpleKES verification key bytes should round-trip");

    assert_eq!(verification_decoded, verification_key);
    assert_eq!(verification_decoded.total_periods(), 3);
}

#[test]
fn simple_kes_rejects_invalid_depth_or_length() {
    let empty = SimpleKesSigningKey::from_seeds(vec![])
        .expect_err("SimpleKES should reject zero-depth key material");
    let malformed_signing = SimpleKesSigningKey::from_bytes(&[0_u8; 31])
        .expect_err("SimpleKES should reject malformed signing key byte lengths");
    let malformed_verification = SimpleKesVerificationKey::from_bytes(&[0_u8; 31])
        .expect_err("SimpleKES should reject malformed verification key byte lengths");

    assert_eq!(empty, CryptoError::InvalidKesDepth(0));
    assert_eq!(
        malformed_signing,
        CryptoError::InvalidKesKeyMaterialLength(31)
    );
    assert_eq!(
        malformed_verification,
        CryptoError::InvalidKesKeyMaterialLength(31)
    );
}

#[test]
fn simple_kes_indexed_signature_round_trips_and_verifies() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[61_u8; 32], [62_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");
    let signature = signing_key
        .sign_indexed(KesPeriod(1), b"indexed")
        .expect("SimpleKES should produce indexed signatures in-range");
    let decoded = SimpleKesSignature::from_bytes(signature.to_bytes());

    assert_eq!(decoded, signature);
    assert_eq!(decoded.period(), KesPeriod(1));
    verification_key
        .verify_indexed(b"indexed", &decoded)
        .expect("SimpleKES indexed signatures should verify");
}

#[test]
fn simple_kes_indexed_signature_rejects_tampered_period() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[71_u8; 32], [72_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");
    let signature = signing_key
        .sign_indexed(KesPeriod(1), b"indexed-tamper")
        .expect("SimpleKES should produce indexed signatures in-range");

    let mut tampered_bytes = signature.to_bytes();
    tampered_bytes[..4].copy_from_slice(&5_u32.to_be_bytes());
    let tampered = SimpleKesSignature::from_bytes(tampered_bytes);
    let error = verification_key
        .verify_indexed(b"indexed-tamper", &tampered)
        .expect_err("SimpleKES indexed verification should reject out-of-range embedded periods");

    assert_eq!(error, CryptoError::InvalidKesPeriod(5));
}

#[test]
fn simple_kes_compact_indexed_signature_round_trips_and_verifies() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[81_u8; 32], [82_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");
    let signature = signing_key
        .sign_indexed_compact(KesPeriod(1), b"indexed-compact")
        .expect("SimpleKES should produce compact indexed signatures in-range");
    let decoded = SimpleCompactKesSignature::from_bytes(signature.to_bytes());

    assert_eq!(decoded, signature);
    assert_eq!(decoded.period(), KesPeriod(1));
    verification_key
        .verify_indexed_compact(b"indexed-compact", &decoded)
        .expect("SimpleKES compact indexed signatures should verify");
}

#[test]
fn simple_kes_compact_indexed_signature_rejects_mismatched_key() {
    let signing_key = SimpleKesSigningKey::from_seeds(vec![[91_u8; 32], [92_u8; 32]])
        .expect("SimpleKES should accept non-empty seed sets");
    let verification_key = signing_key
        .verification_key()
        .expect("SimpleKES verification key derivation should succeed");
    let signature = signing_key
        .sign_indexed_compact(KesPeriod(1), b"indexed-compact-mismatch")
        .expect("SimpleKES should produce compact indexed signatures in-range");

    let mut tampered_bytes = signature.to_bytes();
    tampered_bytes[(4 + 64)..].fill(0);
    let tampered = SimpleCompactKesSignature::from_bytes(tampered_bytes);
    let error = verification_key
        .verify_indexed_compact(b"indexed-compact-mismatch", &tampered)
        .expect_err("SimpleKES compact indexed verification should reject embedded key mismatch");

    assert_eq!(error, CryptoError::KesVerificationKeyMismatch);
}

#[test]
fn simple_kes_fixture_vectors_match_exact_signature_bytes() {
    for vector in simple_kes_two_period_test_vectors() {
        let signing_key = SimpleKesSigningKey::from_seeds(vector.seeds.to_vec())
            .expect("SimpleKES fixture seeds should build a signing key");
        let verification_key = signing_key
            .verification_key()
            .expect("SimpleKES fixture should derive verification keys");
        let period = KesPeriod(vector.period);

        assert_eq!(
            verification_key.to_bytes().len(),
            vector.verification_keys.len() * 32
        );
        assert_eq!(
            &verification_key.to_bytes()[..32],
            vector.verification_keys[0].as_slice()
        );
        assert_eq!(
            &verification_key.to_bytes()[32..64],
            vector.verification_keys[1].as_slice()
        );

        let signature = signing_key
            .sign(period, &vector.message)
            .expect("SimpleKES fixture period should sign");
        let indexed = signing_key
            .sign_indexed(period, &vector.message)
            .expect("SimpleKES fixture period should sign with indexed encoding");
        let compact_indexed = signing_key
            .sign_indexed_compact(period, &vector.message)
            .expect("SimpleKES fixture period should sign with compact indexed encoding");

        assert_eq!(
            signature.to_bytes(),
            vector.signature,
            "signature mismatch for {}",
            vector.name
        );
        assert_eq!(
            indexed.to_bytes(),
            vector.indexed_signature,
            "indexed signature mismatch for {}",
            vector.name
        );
        assert_eq!(
            compact_indexed.to_bytes(),
            vector.compact_indexed_signature,
            "compact indexed signature mismatch for {}",
            vector.name
        );

        verification_key
            .verify_indexed(&vector.message, &indexed)
            .expect("indexed fixture signature should verify");
        verification_key
            .verify_indexed_compact(&vector.message, &compact_indexed)
            .expect("compact indexed fixture signature should verify");
    }
}

#[test]
fn kes_period_evolution_advances_or_overflows() {
    let next = yggdrasil_crypto::kes::evolve_period(KesPeriod(7))
        .expect("small KES periods should advance");
    let overflow = yggdrasil_crypto::kes::evolve_period(KesPeriod(u32::MAX))
        .expect_err("maximum KES period should overflow");

    assert_eq!(next, KesPeriod(8));
    assert_eq!(overflow, CryptoError::KesPeriodOverflow);
}

// ═══════════════════════════════════════════════════════════════════════════
// SumKES tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sum_kes_depth0_sign_verify() {
    let seed = [0x42u8; 32];
    let sk = gen_sum_kes_signing_key(&seed, 0).expect("depth-0 key gen");
    let vk = derive_sum_kes_vk(&sk).expect("depth-0 VK derivation");

    assert_eq!(sk.total_periods(), 1);

    let sig = sign_sum_kes(&sk, 0, b"hello depth-0").expect("depth-0 sign");
    verify_sum_kes(&vk, 0, b"hello depth-0", &sig).expect("depth-0 verify");
}

#[test]
fn sum_kes_depth0_wrong_message_fails() {
    let seed = [0x42u8; 32];
    let sk = gen_sum_kes_signing_key(&seed, 0).expect("depth-0 key gen");
    let vk = derive_sum_kes_vk(&sk).expect("depth-0 VK derivation");

    let sig = sign_sum_kes(&sk, 0, b"good message").expect("depth-0 sign");
    verify_sum_kes(&vk, 0, b"bad message", &sig)
        .expect_err("wrong message should fail verification");
}

#[test]
fn sum_kes_depth1_both_periods() {
    let seed = [0xAA; 32];
    let sk = gen_sum_kes_signing_key(&seed, 1).expect("depth-1 key gen");
    let vk = derive_sum_kes_vk(&sk).expect("depth-1 VK derivation");

    assert_eq!(sk.total_periods(), 2);

    // Period 0: sign with the initial key
    let sig0 = sign_sum_kes(&sk, 0, b"period zero").expect("depth-1 sign period 0");
    verify_sum_kes(&vk, 0, b"period zero", &sig0).expect("depth-1 verify period 0");

    // Evolve to period 1
    let sk1 = update_sum_kes(&sk, 0)
        .expect("depth-1 update")
        .expect("depth-1 should evolve to period 1");

    let sig1 = sign_sum_kes(&sk1, 1, b"period one").expect("depth-1 sign period 1");
    verify_sum_kes(&vk, 1, b"period one", &sig1).expect("depth-1 verify period 1");

    // Cannot evolve past last period
    let expired = update_sum_kes(&sk1, 1).expect("depth-1 update at last period");
    assert!(expired.is_none(), "should be None at final period");
}

#[test]
fn sum_kes_depth2_four_periods() {
    let seed = [0xBB; 32];
    let sk = gen_sum_kes_signing_key(&seed, 2).expect("depth-2 key gen");
    let vk = derive_sum_kes_vk(&sk).expect("depth-2 VK derivation");

    assert_eq!(sk.total_periods(), 4);

    let mut current_sk = sk;
    for period in 0u32..4 {
        let msg = format!("message for period {period}");
        let sig =
            sign_sum_kes(&current_sk, period, msg.as_bytes()).expect("depth-2 sign should succeed");
        verify_sum_kes(&vk, period, msg.as_bytes(), &sig).expect("depth-2 verify should succeed");

        if period < 3 {
            current_sk = update_sum_kes(&current_sk, period)
                .expect("depth-2 update should succeed")
                .expect("depth-2 should produce evolved key");
        }
    }
}

#[test]
fn sum_kes_depth3_full_lifecycle() {
    let seed = [0xCC; 32];
    let sk = gen_sum_kes_signing_key(&seed, 3).expect("depth-3 key gen");
    let vk = derive_sum_kes_vk(&sk).expect("depth-3 VK derivation");

    assert_eq!(sk.total_periods(), 8);

    let mut current_sk = sk;
    for period in 0u32..8 {
        let msg = format!("depth3-period-{period}");
        let sig = sign_sum_kes(&current_sk, period, msg.as_bytes())
            .expect("depth-3 sign should succeed for all 8 periods");
        verify_sum_kes(&vk, period, msg.as_bytes(), &sig)
            .expect("depth-3 verify should succeed for all 8 periods");

        if period < 7 {
            current_sk = update_sum_kes(&current_sk, period)
                .expect("depth-3 update should succeed")
                .expect("depth-3 should produce evolved key");
        }
    }

    let expired = update_sum_kes(&current_sk, 7).expect("depth-3 final update");
    assert!(
        expired.is_none(),
        "depth-3 key should be exhausted at period 7"
    );
}

#[test]
fn sum_kes_vk_is_deterministic() {
    let seed = [0xDD; 32];
    let sk1 = gen_sum_kes_signing_key(&seed, 2).expect("key gen 1");
    let sk2 = gen_sum_kes_signing_key(&seed, 2).expect("key gen 2");

    let vk1 = derive_sum_kes_vk(&sk1).expect("VK 1");
    let vk2 = derive_sum_kes_vk(&sk2).expect("VK 2");

    assert_eq!(vk1, vk2, "same seed should produce same VK");
}

#[test]
fn sum_kes_different_seeds_produce_different_vks() {
    let sk1 = gen_sum_kes_signing_key(&[0x01; 32], 2).expect("key gen 1");
    let sk2 = gen_sum_kes_signing_key(&[0x02; 32], 2).expect("key gen 2");

    let vk1 = derive_sum_kes_vk(&sk1).expect("VK 1");
    let vk2 = derive_sum_kes_vk(&sk2).expect("VK 2");

    assert_ne!(vk1, vk2, "different seeds should produce different VKs");
}

#[test]
fn sum_kes_cross_period_forgery_fails() {
    let seed = [0xEE; 32];
    let sk = gen_sum_kes_signing_key(&seed, 2).expect("key gen");
    let vk = derive_sum_kes_vk(&sk).expect("VK");

    // Sign at period 0
    let sig0 = sign_sum_kes(&sk, 0, b"cross-period test").expect("sign period 0");

    // Verify at period 1 should fail
    verify_sum_kes(&vk, 1, b"cross-period test", &sig0)
        .expect_err("signature from period 0 should not verify at period 1");
}

#[test]
fn sum_kes_invalid_period_rejected() {
    let seed = [0xFF; 32];
    let sk = gen_sum_kes_signing_key(&seed, 1).expect("key gen");

    // Only 2 periods (0, 1) are valid for depth 1
    let err = sign_sum_kes(&sk, 2, b"oob period")
        .expect_err("period 2 should be rejected for depth-1 key");
    assert_eq!(err, CryptoError::InvalidKesPeriod(2));
}

#[test]
fn sum_kes_signature_size() {
    assert_eq!(SumKesSignature::expected_size(0), 64);
    assert_eq!(SumKesSignature::expected_size(1), 128);
    assert_eq!(SumKesSignature::expected_size(2), 192);
    assert_eq!(SumKesSignature::expected_size(6), 448);
}

#[test]
fn sum_kes_depth6_key_gen_and_sign() {
    // Mainnet uses Sum6KES — verify it works with 64 periods.
    let seed = [0x99; 32];
    let sk = gen_sum_kes_signing_key(&seed, 6).expect("depth-6 key gen");
    let vk = derive_sum_kes_vk(&sk).expect("depth-6 VK derivation");

    assert_eq!(sk.total_periods(), 64);

    // Sign at period 0 and verify.
    let sig = sign_sum_kes(&sk, 0, b"mainnet depth-6").expect("depth-6 sign");
    assert_eq!(sig.to_bytes().len(), 448);
    verify_sum_kes(&vk, 0, b"mainnet depth-6", &sig).expect("depth-6 verify");

    // Evolve to period 1 and verify.
    let sk1 = update_sum_kes(&sk, 0)
        .expect("depth-6 update")
        .expect("depth-6 should evolve to period 1");
    let sig1 = sign_sum_kes(&sk1, 1, b"mainnet depth-6 p1").expect("depth-6 sign p1");
    verify_sum_kes(&vk, 1, b"mainnet depth-6 p1", &sig1).expect("depth-6 verify p1");
}

#[test]
fn sum_kes_signature_round_trip() {
    let seed = [0x77; 32];
    let sk = gen_sum_kes_signing_key(&seed, 2).expect("key gen");
    let sig = sign_sum_kes(&sk, 0, b"round-trip test").expect("sign");

    let bytes = sig.to_bytes();
    let restored = SumKesSignature::from_bytes(2, bytes).expect("from_bytes");
    assert_eq!(sig, restored);
}

#[test]
fn sum_kes_vk_round_trip() {
    let seed = [0x88; 32];
    let sk = gen_sum_kes_signing_key(&seed, 3).expect("key gen");
    let vk = derive_sum_kes_vk(&sk).expect("VK");

    let restored = SumKesVerificationKey::from_bytes(vk.to_bytes());
    assert_eq!(vk, restored);
}
