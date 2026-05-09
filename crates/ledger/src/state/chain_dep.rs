//! `ChainDepStateContext` — sidecar nonce + OCert counter mirror attached to
//! [`super::LedgerStateSnapshot`] so LSQ dispatchers can serve live nonces and
//! OCert counters in `query protocol-state`.
//!
//! Mirrors upstream
//! [`Ouroboros.Consensus.Protocol.Praos.PraosState`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/Ouroboros/Consensus/Protocol/Praos/PraosState.hs).
//!
//! `crates/consensus` owns the canonical `NonceEvolutionState` /
//! `OcertCounters` types but cannot be imported here without inverting the
//! dependency direction. The runtime translates from those types into this
//! snapshot-side mirror at snapshot capture time.
//!
//! Extracted from `state.rs` in R269 twelfth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269l-state-treasury-chaindep-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side sidecar that mirrors the
//! `crates/consensus`-owned `NonceEvolutionState` + `OcertCounters` so
//! LSQ dispatchers can answer `query protocol-state` without inverting
//! the `crates → consensus` dependency direction. Upstream's analogous
//! data lives in `Ouroboros.Consensus.Protocol.Praos.PraosState` (in
//! the praos protocol module, not a separate file in 11.0.1).

use crate::types::Nonce;
use std::collections::BTreeMap;

/// Round 192 — Companion `ChainDepState` snapshot data attached to
/// [`super::LedgerStateSnapshot`] so LSQ dispatchers can serve live nonces
/// and OCert counters in `query protocol-state`.
///
/// `crates/consensus` owns the canonical
/// `NonceEvolutionState`/`OcertCounters` types but cannot be imported
/// here without inverting the dependency direction. The runtime
/// translates from those types into this snapshot-side mirror at
/// snapshot capture time.
///
/// Reference: `Ouroboros.Consensus.Protocol.Praos.PraosState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainDepStateContext {
    /// `praosStateEvolvingNonce` (η_v) — combines every block's VRF
    /// nonce contribution within an epoch.
    pub evolving_nonce: Nonce,
    /// `praosStateCandidateNonce` (η_c) — frozen at the stability
    /// window inside an epoch.
    pub candidate_nonce: Nonce,
    /// `praosStateEpochNonce` — the active epoch nonce used for VRF
    /// verification.
    pub epoch_nonce: Nonce,
    /// `praosStatePreviousEpochNonce` — previous epoch's nonce.
    /// Yggdrasil does not yet track this distinctly from the epoch
    /// nonce; emits Neutral until plumbed.
    pub previous_epoch_nonce: Nonce,
    /// `praosStateLabNonce` — the "last applied block" nonce derived
    /// from the most recent block's prev-hash.
    pub lab_nonce: Nonce,
    /// `praosStateLastEpochBlockNonce` — the nonce derived from the
    /// last block of the previous epoch (yggdrasil's
    /// `NonceEvolutionState::prev_hash_nonce`).
    pub last_epoch_block_nonce: Nonce,
    /// `praosStateOCertCounters` — per-pool monotonic OpCert
    /// sequence-number tracker keyed by 28-byte cold-key hash.
    pub opcert_counters: BTreeMap<[u8; 28], u64>,
}

impl Default for ChainDepStateContext {
    fn default() -> Self {
        Self {
            evolving_nonce: Nonce::Neutral,
            candidate_nonce: Nonce::Neutral,
            epoch_nonce: Nonce::Neutral,
            previous_epoch_nonce: Nonce::Neutral,
            lab_nonce: Nonce::Neutral,
            last_epoch_block_nonce: Nonce::Neutral,
            opcert_counters: BTreeMap::new(),
        }
    }
}
