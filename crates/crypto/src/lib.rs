//! Cryptographic primitives and compatibility fixtures used across the workspace.

/// Blake2b hashing helpers.
pub mod blake2b;
/// Ed25519 signing and verification types.
pub mod ed25519;
mod error;
/// Key-evolving signature helpers and shared types.
pub mod kes;
/// Published compatibility vectors used by crypto tests.
pub mod test_vectors;
/// Verifiable random function placeholders and shared types.
pub mod vrf;

/// Blake2b hash output and hashing entry point.
pub use blake2b::{Blake2bHash, hash_bytes};
/// Ed25519 byte-backed key and signature types.
pub use ed25519::{Signature, SigningKey, VerificationKey};
/// Errors surfaced by the crypto crate.
pub use error::CryptoError;
/// Key-evolving signature period, key, and signature wrappers.
pub use kes::{
	CompactKesSignature, KesPeriod, KesSignature, KesSigningKey, KesVerificationKey,
};
/// RFC-backed Ed25519 test vector structures and fixtures.
pub use test_vectors::{
	Ed25519TestVector, VrfPraosBatchCompatTestVector, VrfPraosTestVector,
	ed25519_rfc8032_vectors, vrf_praos_batchcompat_test_vectors, vrf_praos_test_vectors,
};
/// VRF byte-backed key, proof, and output types.
pub use vrf::{
	VrfBatchCompatProof, VrfOutput, VrfProof, VrfSecretKey, VrfVerificationKey,
};
