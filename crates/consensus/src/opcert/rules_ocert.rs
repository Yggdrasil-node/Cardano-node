//! OpCert sequence-number monotonic counter tracking.
//!
//! Mirrors upstream `Cardano.Protocol.TPraos.Rules.OCert` (TPraos, Shelley/
//! Allegra/Mary, pre-Babbage) and `Ouroboros.Consensus.Protocol.Praos`
//! (Praos, Babbage+ Vasil HF onward) — the counter validation rule
//! tightened at the Vasil hard fork to enforce both bounds
//! (`stored ≤ new_seq ≤ stored + 1`) instead of just the lower bound.
//!
//! Two public types:
//!
//! - `OcertCounters` — per-pool monotonic-counter map with CBOR sidecar
//!   serialisation.
//! - `OcertCounterRule` — TPraos vs Praos discriminant for protocol-version-
//!   aware counter validation.
//!
//! Extracted from `opcert.rs` in R273c (Phase γ §R273 third slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `Cardano.Protocol.TPraos.Rules.OCert.hs`
//! (upstream's OCERT counter rule). File renamed `counter.rs` →
//! `rules_ocert.rs` in R273-rename to match upstream's `Rules/OCert.hs`
//! path (flattened with directory prefix to disambiguate from
//! sibling `ocert.rs`).

use std::collections::BTreeMap;

use crate::error::ConsensusError;

/// Tracks the highest observed operational-certificate sequence number per
/// pool (keyed by the Blake2b-224 hash of the issuer cold key).
///
/// When a new block arrives the counter is validated using the same rules as
/// the upstream `currentIssueNo` helper. The exact rule depends on the era:
///
/// - **TPraos (Shelley/Allegra/Mary, pre-Babbage)** — `Cardano.Protocol.TPraos.Rules.OCert`
///   only enforces `stored ≤ new_seq` (lower bound). The upper bound is
///   not checked, so a pool may "skip" counter values.
/// - **Praos (Babbage+, Vasil HF onward)** — `Ouroboros.Consensus.Protocol.Praos`
///   tightens the rule to `stored ≤ new_seq ≤ stored + 1` (both bounds), so
///   counters increment in lock-step with KES rotations.
///
/// Pool-initialization rules (when not yet in counter map):
///
/// 1. If the pool is already in the counter map, validate per the era's rule.
/// 2. If the pool is **not** in the counter map but **is** present in the
///    stake distribution, the counter is initialized — the block is the
///    first we have seen from this pool.
/// 3. If the pool is in neither, `NoCounterForKeyHash` is returned.
///
/// Reference:
/// - TPraos: [`Cardano.Protocol.TPraos.Rules.OCert`](https://github.com/IntersectMBO/cardano-ledger/blob/master/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/Rules/OCert.hs)
/// - Praos: [`Ouroboros.Consensus.Protocol.Praos`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs)
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OcertCounters {
    counters: BTreeMap<[u8; 28], u64>,
}

/// Selects which OpCert counter rule to apply.
///
/// The strictness changed at the Vasil hard fork (protocol major version 7).
/// Use [`Self::for_pv_major`] to derive this from a protocol version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OcertCounterRule {
    /// Pre-Babbage: only `stored ≤ new_seq` is enforced.
    TPraos,
    /// Babbage+ (Vasil onward): both bounds enforced — `stored ≤ new_seq ≤ stored + 1`.
    Praos,
}

impl OcertCounterRule {
    /// Picks the correct rule for a protocol-major version.
    ///
    /// Vasil HF activated at PV major 7 on mainnet (epoch 365). PV ≥ 7 ⇒ Praos.
    /// Lower PVs (Shelley/Allegra/Mary/Alonzo) use TPraos.
    pub fn for_pv_major(pv_major: u64) -> Self {
        if pv_major >= 7 {
            Self::Praos
        } else {
            Self::TPraos
        }
    }
}

impl OcertCounters {
    /// Returns a fresh (empty) counter map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates a block's OpCert sequence number under the **Praos**
    /// (Babbage+) rule and updates the map on success.
    ///
    /// Equivalent to calling [`Self::validate_and_update_with_rule`] with
    /// [`OcertCounterRule::Praos`]. Retained for callers that always operate
    /// on Praos-era chains (e.g. preview, post-Vasil mainnet).
    ///
    /// # Errors
    ///
    /// * `OcertCounterTooOld` — `new_seq < stored`.
    /// * `OcertCounterTooFar` — `new_seq > stored + 1`.
    /// * `NoCounterForKeyHash` — pool is in neither the counter map nor the
    ///   stake distribution.
    pub fn validate_and_update(
        &mut self,
        pool_key_hash: [u8; 28],
        new_seq: u64,
        pool_in_dist: bool,
    ) -> Result<(), ConsensusError> {
        self.validate_and_update_with_rule(
            pool_key_hash,
            new_seq,
            pool_in_dist,
            OcertCounterRule::Praos,
        )
    }

    /// Validates a block's OpCert sequence number under the supplied era rule
    /// and updates the map on success.
    ///
    /// Use [`OcertCounterRule::for_pv_major`] to pick the right rule for the
    /// chain's current protocol version.
    ///
    /// # Errors
    ///
    /// * `OcertCounterTooOld` — `new_seq < stored` (always enforced).
    /// * `OcertCounterTooFar` — `new_seq > stored + 1` (Praos only;
    ///   TPraos accepts arbitrary forward jumps).
    /// * `NoCounterForKeyHash` — pool is in neither the counter map nor the
    ///   stake distribution.
    pub fn validate_and_update_with_rule(
        &mut self,
        pool_key_hash: [u8; 28],
        new_seq: u64,
        pool_in_dist: bool,
        rule: OcertCounterRule,
    ) -> Result<(), ConsensusError> {
        if let Some(&stored) = self.counters.get(&pool_key_hash) {
            if new_seq < stored {
                return Err(ConsensusError::OcertCounterTooOld {
                    stored,
                    received: new_seq,
                });
            }
            if matches!(rule, OcertCounterRule::Praos) && new_seq > stored.saturating_add(1) {
                return Err(ConsensusError::OcertCounterTooFar {
                    stored,
                    received: new_seq,
                });
            }
            // Accept — under TPraos any `new_seq ≥ stored` passes; under Praos
            // we already enforced `new_seq ≤ stored + 1` above.
            self.counters.insert(pool_key_hash, new_seq);
            Ok(())
        } else if pool_in_dist {
            // First block seen from a known pool — initialize.
            self.counters.insert(pool_key_hash, new_seq);
            Ok(())
        } else {
            Err(ConsensusError::NoCounterForKeyHash {
                hash: pool_key_hash,
            })
        }
    }

    /// Returns the stored counter for `pool_key_hash`, if tracked.
    pub fn get(&self, pool_key_hash: &[u8; 28]) -> Option<u64> {
        self.counters.get(pool_key_hash).copied()
    }

    /// Returns the number of pools currently tracked.
    pub fn len(&self) -> usize {
        self.counters.len()
    }

    /// Returns `true` when no counters are tracked.
    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }

    /// Returns an iterator over `(pool_key_hash, sequence_number)` pairs in
    /// `BTreeMap` order. Used by the CBOR encoder so encoded bytes are
    /// deterministic regardless of insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8; 28], &u64)> {
        self.counters.iter()
    }

    /// Resets the counter map to empty.
    ///
    /// Required at chain rollback boundaries: the per-pool monotonicity
    /// high-water mark is part of upstream `PraosState.csCounters`,
    /// which is persisted as part of `ChainDepState` and ROLLED BACK
    /// to a snapshot at the rollback restore point. Without resetting,
    /// a fork that happens to include lower-sequence OpCerts from the
    /// same pool (legitimate on the alt chain) is rejected as
    /// `OcertCounterTooOld` even though those certs are valid.
    ///
    /// The "first-seen pool is permissive" rule in
    /// [`Self::validate_and_update`] then naturally re-initialises
    /// each pool's counter from the next block we see, restoring the
    /// monotonicity guard from that point forward.
    ///
    /// Reference: `Cardano.Protocol.TPraos.API` `tickChainDepState`
    /// rollback semantics.
    pub fn clear(&mut self) {
        self.counters.clear();
    }
}

// ---------------------------------------------------------------------------
// OcertCounters — CBOR codec (sidecar persistence)
// ---------------------------------------------------------------------------

/// CBOR encoding of `OcertCounters`: a single CBOR map keyed by the 28-byte
/// pool key hash with `u64` sequence-number values, emitted in canonical
/// `BTreeMap` (lexicographic) key order.
///
/// This sidecar is persisted alongside the ledger checkpoint so a
/// restarted node retains its per-pool monotonicity guarantees rather than
/// resetting the high-water mark to zero — closing the upstream-parity
/// gap with `PraosState.csCounters`, which is part of the persistent
/// `ChainDepState` in `Ouroboros.Consensus.Protocol.Praos`.
impl yggdrasil_ledger::cbor::CborEncode for OcertCounters {
    fn encode_cbor(&self, enc: &mut yggdrasil_ledger::cbor::Encoder) {
        enc.map(self.counters.len() as u64);
        for (pool_key_hash, &seq) in &self.counters {
            enc.bytes(pool_key_hash);
            enc.unsigned(seq);
        }
    }
}

impl yggdrasil_ledger::cbor::CborDecode for OcertCounters {
    fn decode_cbor(
        dec: &mut yggdrasil_ledger::cbor::Decoder<'_>,
    ) -> Result<Self, yggdrasil_ledger::LedgerError> {
        let entries = dec.map()?;
        let mut counters = BTreeMap::new();
        for _ in 0..entries {
            let key_bytes = dec.bytes()?;
            if key_bytes.len() != 28 {
                return Err(yggdrasil_ledger::LedgerError::CborInvalidLength {
                    expected: 28,
                    actual: key_bytes.len(),
                });
            }
            let mut pool_key_hash = [0u8; 28];
            pool_key_hash.copy_from_slice(key_bytes);
            let seq = dec.unsigned()?;
            counters.insert(pool_key_hash, seq);
        }
        Ok(Self { counters })
    }
}
