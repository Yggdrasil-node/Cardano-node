use yggdrasil_crypto::{Blake2bHash, CryptoError, SigningKey, hash_bytes};

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
