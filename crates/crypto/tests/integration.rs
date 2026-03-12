use yggdrasil_crypto::{
    Blake2bHash, CryptoError, Signature, SigningKey, VerificationKey,
    ed25519_rfc8032_vectors, hash_bytes,
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
