//! VRF input construction for Praos and TPraos protocol modes.
//!
//! Mirrors upstream `Ouroboros.Consensus.Protocol.Praos.VRF`
//! (`mkInputVRF`) and `Cardano.Protocol.TPraos.BHeader::mkSeed` (`seedEta`,
//! `seedL`).
//!
//! Three public types and four public fns:
//!
//! - `VrfMode` — TPraos vs Praos protocol mode discriminant.
//! - `VrfUsage` — Leader vs Nonce purpose tag (TPraos only).
//! - `praos_vrf_input` — Babbage/Conway VRF input
//!   (Blake2b-256 over `slot_be8 || nonce`).
//! - `tpraos_vrf_seed` — Shelley/Allegra/Mary/Alonzo VRF seed
//!   (Blake2b-256 over `slot_be8 || nonce` XOR per-purpose tag hash).
//! - `vrf_input` — mode-aware dispatcher.
//!
//! Extracted from `praos.rs` in R273a (Phase γ §R273 first slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** Ouroboros/Consensus/Protocol/Praos/VRF.hs.
//! Filename matches upstream basename (or flattens upstream
//! directory); the module is the canonical 1:1 mirror surface
//! for the Rust port of upstream's `Ouroboros/Consensus/Protocol/Praos/VRF.hs` module.

use yggdrasil_crypto::blake2b::hash_bytes_256;
use yggdrasil_ledger::{Nonce, SlotNo};

/// Distinguishes the two VRF protocol modes used across Cardano eras.
///
/// - **TPraos** (Shelley–Alonzo): uses `mkSeed` with a per-purpose XOR tag
///   and checks the raw 512-bit VRF output against `2^512`.
/// - **Praos** (Babbage/Conway): uses `mkInputVRF` (Blake2b-256 of slot||nonce)
///   and applies range extension (`Blake2b-256("L" || output)`) to check a
///   256-bit value against `2^256`.
///
/// Reference: `Ouroboros.Consensus.Protocol.TPraos` vs
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrfMode {
    /// Shelley through Alonzo: `mkSeed` construction, raw 512-bit leader check.
    TPraos,
    /// Babbage and Conway: `mkInputVRF` construction, range-extended 256-bit
    /// leader check.
    Praos,
}
/// Distinguishes the two VRF proof purposes within a TPraos block header.
///
/// TPraos headers carry two VRF proofs (`nonce_vrf` and `leader_vrf`), each
/// produced over a different seed.  Praos headers carry only one unified VRF
/// proof that serves both purposes.
///
/// Reference: `seedEta` / `seedL` in `Cardano.Protocol.TPraos.BHeader`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VrfUsage {
    /// Leader election proof (TPraos `seedL`, tag = `mkNonceFromNumber 1`).
    Leader,
    /// Nonce contribution proof (TPraos `seedEta`, tag = `mkNonceFromNumber 0`).
    Nonce,
}
/// Builds the raw VRF input bytes from a slot number and an epoch nonce
/// (pre-hash concatenation, no Blake2b-256, no seed tag).
///
/// This is the base concatenation `slot_be8 || nonce_bytes` before any
/// protocol-specific hashing or XOR.  Callers that need upstream-compatible
/// VRF inputs should use [`praos_vrf_input`] or [`tpraos_vrf_seed`] instead.
pub(super) fn raw_vrf_input_bytes(slot: SlotNo, epoch_nonce: Nonce) -> Vec<u8> {
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(&slot.0.to_be_bytes());
    if let Nonce::Hash(h) = epoch_nonce {
        buf.extend_from_slice(&h);
    }
    buf
}

/// Builds the Praos (Babbage/Conway) VRF input: `Blake2b-256(slot_be8 || nonce_bytes)`.
///
/// The result is a 32-byte hash matching upstream `mkInputVRF` from
/// `Ouroboros.Consensus.Protocol.Praos.VRF`, which is used as
/// `getSignableRepresentation` for the single unified VRF proof.
pub fn praos_vrf_input(slot: SlotNo, epoch_nonce: Nonce) -> Vec<u8> {
    hash_bytes_256(&raw_vrf_input_bytes(slot, epoch_nonce))
        .0
        .to_vec()
}

/// Pre-computed seed tag hashes for TPraos VRF input construction.
///
/// Upstream `mkNonceFromNumber n` = `Nonce (Blake2b-256(word64be(n)))`.
///
/// Reference: `mkNonceFromNumber` in `Cardano.Ledger.BaseTypes`.
pub(super) fn tpraos_seed_tag_hash(usage: VrfUsage) -> [u8; 32] {
    let tag = match usage {
        VrfUsage::Nonce => 0u64,  // seedEta
        VrfUsage::Leader => 1u64, // seedL
    };
    hash_bytes_256(&tag.to_be_bytes()).0
}

/// Builds a TPraos (Shelley–Alonzo) VRF seed: `Blake2b-256(slot_be8 || nonce_bytes) XOR tag_hash`.
///
/// `usage` selects the seed tag:
/// - `VrfUsage::Leader` → `seedL` (tag 1): used for the leader VRF proof.
/// - `VrfUsage::Nonce`  → `seedEta` (tag 0): used for the nonce VRF proof.
///
/// The result is a 32-byte value matching upstream `mkSeed` from
/// `Cardano.Protocol.TPraos.BHeader`.
pub fn tpraos_vrf_seed(slot: SlotNo, epoch_nonce: Nonce, usage: VrfUsage) -> Vec<u8> {
    let base_hash = hash_bytes_256(&raw_vrf_input_bytes(slot, epoch_nonce)).0;
    let tag_hash = tpraos_seed_tag_hash(usage);
    // XOR the two 32-byte hashes.
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = base_hash[i] ^ tag_hash[i];
    }
    result.to_vec()
}

/// Builds the VRF input for the given mode and usage.
///
/// - `VrfMode::Praos` ignores `usage` (single unified VRF) and returns
///   `praos_vrf_input()`.
/// - `VrfMode::TPraos` returns `tpraos_vrf_seed()` with the given usage.
pub fn vrf_input(slot: SlotNo, epoch_nonce: Nonce, mode: VrfMode, usage: VrfUsage) -> Vec<u8> {
    match mode {
        VrfMode::Praos => praos_vrf_input(slot, epoch_nonce),
        VrfMode::TPraos => tpraos_vrf_seed(slot, epoch_nonce, usage),
    }
}
