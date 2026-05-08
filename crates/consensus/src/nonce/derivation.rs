//! VRF-output-to-nonce derivation primitives for Praos and TPraos eras.
//!
//! Mirrors upstream `Cardano.Ledger.BaseTypes::hashVerifiedVRF` (TPraos)
//! and `Ouroboros.Consensus.Protocol.Praos.VRF::vrfNonceValue` (Praos).
//!
//! Three public fns + 1 enum:
//!
//! - `NonceDerivation` — TPraos vs Praos discriminant.
//! - `vrf_output_to_nonce` — TPraos: `Blake2b-256(output)`.
//! - `praos_vrf_output_to_nonce` — Praos: `Blake2b-256("N" || output)`.
//! - `derive_vrf_nonce` — era-aware dispatcher.
//!
//! Extracted from `nonce.rs` in R273b (Phase γ §R273 second slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. The `hashVerifiedVRF` helper that this file
//! wraps lives inside `Cardano.Ledger.BaseTypes` (one of upstream's
//! kitchen-sink modules); there is no `Derivation.hs` upstream. This
//! file is a Yggdrasil-side aggregation that surfaces the upstream
//! helpers under a focused name.

use yggdrasil_crypto::hash_bytes_256;
use yggdrasil_ledger::Nonce;

/// Selects the VRF-output-to-nonce derivation for the current era.
///
/// TPraos (Shelley–Alonzo) and Praos (Babbage/Conway) use different
/// hashing schemes to convert a VRF output into a nonce contribution.
///
/// Reference: `hashVerifiedVRF` (TPraos), `vrfNonceValue` (Praos) in
/// `Ouroboros.Consensus.Protocol.Praos.VRF`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NonceDerivation {
    /// TPraos (Shelley–Alonzo): `Blake2b-256(output)`.
    ///
    /// Reference: `hashVerifiedVRF` in `Cardano.Ledger.BaseTypes`.
    TPraos,
    /// Praos (Babbage/Conway): `Blake2b-256(Blake2b-256("N" || output))`.
    ///
    /// The VRF output is first range-extended via `hashVRF SVRFNonce`
    /// (which prepends `"N"` and hashes), then the resulting 32-byte
    /// hash is hashed again to produce the nonce.
    ///
    /// Reference: `vrfNonceValue` in
    /// `Ouroboros.Consensus.Protocol.Praos.VRF`.
    Praos,
}

/// Converts a VRF output (raw bytes) to a `Nonce` using TPraos derivation.
///
/// This is `Blake2b-256(output)`, matching upstream `hashVerifiedVRF`.
///
/// For Praos-era (Babbage/Conway) blocks, use [`praos_vrf_output_to_nonce`]
/// instead.
///
/// Reference: `hashVerifiedVRF` in `Cardano.Ledger.BaseTypes`.
pub fn vrf_output_to_nonce(output: &[u8]) -> Nonce {
    let hash = hash_bytes_256(output);
    Nonce::Hash(hash.0)
}

/// Converts a VRF output (raw bytes) to a `Nonce` using Praos derivation.
///
/// This is `Blake2b-256(Blake2b-256("N" || output))`, matching upstream
/// `vrfNonceValue` from `Ouroboros.Consensus.Protocol.Praos.VRF`.
///
/// The double hash is intentional: the first hash (`"N" || output`) is
/// the VRF range-extension step, and the second hash converts the
/// crypto-dependent hash into a fixed `Blake2b_256` nonce.
///
/// Reference: `vrfNonceValue`, `hashVRF SVRFNonce` in
/// `Ouroboros.Consensus.Protocol.Praos.VRF`.
pub fn praos_vrf_output_to_nonce(output: &[u8]) -> Nonce {
    // Step 1: hashVRF SVRFNonce = Blake2b-256("N" || output)
    let mut prefixed = Vec::with_capacity(1 + output.len());
    prefixed.push(b'N');
    prefixed.extend_from_slice(output);
    let inner_hash = hash_bytes_256(&prefixed);
    // Step 2: hashWith id (hashToBytes inner_hash) = Blake2b-256(inner_hash_bytes)
    let outer_hash = hash_bytes_256(&inner_hash.0);
    Nonce::Hash(outer_hash.0)
}

/// Derives a nonce from a VRF output using the given era-specific derivation.
pub fn derive_vrf_nonce(output: &[u8], derivation: NonceDerivation) -> Nonce {
    match derivation {
        NonceDerivation::TPraos => vrf_output_to_nonce(output),
        NonceDerivation::Praos => praos_vrf_output_to_nonce(output),
    }
}
