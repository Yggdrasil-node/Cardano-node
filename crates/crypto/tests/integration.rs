use yggdrasil_crypto::{
    Blake2bHash, CompactKesSignature, CryptoError, KesPeriod, KesSigningKey,
    Signature, SigningKey, VerificationKey, VrfBatchCompatProof, VrfOutput,
    VrfProof, VrfSecretKey,
    ed25519_rfc8032_vectors, hash_bytes, vrf_praos_batchcompat_test_vectors,
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

        assert_eq!(signature, expected_signature, "signature mismatch for {}", vector.name);

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

        assert_eq!(signing_key.to_bytes(), vector.secret_key, "signing key mismatch for {}", vector.name);
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

        assert_eq!(signing_key.to_bytes(), vector.secret_key, "signing key mismatch for {}", vector.name);
        assert_eq!(
            verification_key.to_bytes(),
            vector.public_key,
            "verification key mismatch for {}",
            vector.name
        );
        assert_eq!(
            proof.output().expect("published batch-compatible Praos proof should decode"),
            expected_output,
            "output mismatch for {}",
            vector.name
        );
    }
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
    assert_eq!(decoded.signature().to_bytes().as_slice(), &compact_bytes[..64]);
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
