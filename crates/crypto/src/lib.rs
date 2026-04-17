#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Cryptographic primitives and compatibility fixtures used across the workspace.

/// Blake2b hashing helpers.
pub mod blake2b;
/// BLS12-381 elliptic curve operations for PlutusV3 builtins.
pub mod bls12_381;
/// Ed25519 signing and verification types.
pub mod ed25519;
mod error;
/// Key-evolving signature helpers and shared types.
pub mod kes;
/// secp256k1 ECDSA and Schnorr (BIP-340) signature verification.
pub mod secp256k1;
/// SHA3-256 hashing for Byron address root reconstruction.
pub mod sha3_hash;
/// Sum-composition Key-Evolving Signatures (SumKES).
pub mod sum_kes;
/// Published compatibility vectors used by crypto tests.
pub mod test_vectors;
/// Verifiable random function key, proof, and output helpers.
pub mod vrf;

/// Blake2b hash output and hashing entry point.
pub use blake2b::{
    Blake2b224Hash, Blake2b256Hash, Blake2bHash, hash_bytes, hash_bytes_224, hash_bytes_256,
};
/// BLS12-381 opaque element types.
pub use bls12_381::{G1Element, G2Element, MlResult};
/// Ed25519 byte-backed key and signature types.
pub use ed25519::{Signature, SigningKey, VerificationKey};
/// Errors surfaced by the crypto crate.
pub use error::CryptoError;
/// Key-evolving signature period, key, and signature wrappers.
pub use kes::{
    CompactKesSignature, KesPeriod, KesSignature, KesSigningKey, KesVerificationKey,
    SimpleCompactKesSignature, SimpleKesSignature, SimpleKesSigningKey, SimpleKesVerificationKey,
};
/// secp256k1 ECDSA and Schnorr verification entry points.
pub use secp256k1::{verify_ecdsa, verify_schnorr};
/// SHA3-256 hashing for Byron address root reconstruction.
pub use sha3_hash::{Sha3_256Hash, sha3_256};
/// SumKES key-evolving signature types and operations.
pub use sum_kes::{
    SumKesSignature, SumKesSigningKey, SumKesVerificationKey, derive_sum_kes_vk,
    gen_sum_kes_signing_key, sign_sum_kes, update_sum_kes, verify_sum_kes,
};
/// RFC-backed Ed25519 test vector structures and fixtures.
pub use test_vectors::{
    Ed25519TestVector, SimpleKesTwoPeriodTestVector, VrfPraosBatchCompatTestVector,
    VrfPraosTestVector, ed25519_rfc8032_vectors, simple_kes_two_period_test_vectors,
    vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors,
};
/// VRF byte-backed key, proof, and output types.
pub use vrf::{VrfBatchCompatProof, VrfOutput, VrfProof, VrfSecretKey, VrfVerificationKey};
