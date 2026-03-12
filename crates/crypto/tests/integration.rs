use yggdrasil_crypto::{
    Blake2bHash, CompactKesSignature, CryptoError, KesPeriod, KesSigningKey,
    Signature, SigningKey, SimpleCompactKesSignature, SimpleKesSignature,
    SimpleKesSigningKey, SimpleKesVerificationKey, VerificationKey,
    VrfBatchCompatProof, VrfOutput, VrfProof, VrfSecretKey,
    ed25519_rfc8032_vectors, hash_bytes, simple_kes_two_period_test_vectors,
    vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors,
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
fn vrf_secret_key_equality_is_byte_exact() {
    let left = VrfSecretKey::from_seed([1_u8; 32]);
    let same = VrfSecretKey::from_seed([1_u8; 32]);
    let different = VrfSecretKey::from_seed([2_u8; 32]);

    assert_eq!(left, same);
    assert_ne!(left, different);
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
    assert_eq!(malformed_signing, CryptoError::InvalidKesKeyMaterialLength(31));
    assert_eq!(malformed_verification, CryptoError::InvalidKesKeyMaterialLength(31));
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

        assert_eq!(verification_key.to_bytes().len(), vector.verification_keys.len() * 32);
        assert_eq!(&verification_key.to_bytes()[..32], vector.verification_keys[0].as_slice());
        assert_eq!(&verification_key.to_bytes()[32..64], vector.verification_keys[1].as_slice());

        let signature = signing_key
            .sign(period, &vector.message)
            .expect("SimpleKES fixture period should sign");
        let indexed = signing_key
            .sign_indexed(period, &vector.message)
            .expect("SimpleKES fixture period should sign with indexed encoding");
        let compact_indexed = signing_key
            .sign_indexed_compact(period, &vector.message)
            .expect("SimpleKES fixture period should sign with compact indexed encoding");

        assert_eq!(signature.to_bytes(), vector.signature, "signature mismatch for {}", vector.name);
        assert_eq!(indexed.to_bytes(), vector.indexed_signature, "indexed signature mismatch for {}", vector.name);
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
