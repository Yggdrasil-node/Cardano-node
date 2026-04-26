//! Operational certificate (OpCert) types and verification.
//!
//! An operational certificate binds a cold (offline) Ed25519 key to a hot
//! KES verification key for a window of KES periods.  The cold key signs
//! `hot_vkey || sequence_number || kes_period` to produce the certificate
//! signature.
//!
//! Reference: `Cardano.Protocol.TPraos.OCert` in `cardano-ledger`.

use yggdrasil_crypto::ed25519::{Signature, VerificationKey};
use yggdrasil_crypto::sum_kes::SumKesVerificationKey;

use std::collections::BTreeMap;

use crate::error::ConsensusError;

/// Operational certificate: proves that the cold key authorized a hot KES
/// key for block signing starting at a given KES period.
///
/// The certificate is verified by checking the cold-key signature over the
/// canonical signable representation (hot_vkey ‖ sequence_number ‖ kes_period).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpCert {
    /// The hot KES verification key certified by this certificate.
    pub hot_vkey: SumKesVerificationKey,
    /// Monotonic sequence number — must be ≥ the previous certificate's
    /// sequence number for the same pool.
    pub sequence_number: u64,
    /// The KES period at which this certificate becomes valid.
    pub kes_period: u64,
    /// Ed25519 signature by the cold key over the signable representation.
    pub sigma: Signature,
}

impl OpCert {
    /// Compute the serialized signable bytes: `hot_vkey || sequence_number_BE || kes_period_BE`.
    ///
    /// This matches the upstream `OCertSignable` representation:
    /// ```text
    /// rawSerialiseVerKeyKES vk <> word64BE sequence_number <> word64BE (fromIntegral kesPeriod)
    /// ```
    pub fn signable_bytes(&self) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[..32].copy_from_slice(&self.hot_vkey.to_bytes());
        buf[32..40].copy_from_slice(&self.sequence_number.to_be_bytes());
        buf[40..48].copy_from_slice(&self.kes_period.to_be_bytes());
        buf
    }

    /// Verify the cold-key signature on this certificate.
    pub fn verify(&self, cold_vk: &VerificationKey) -> Result<(), ConsensusError> {
        let signable = self.signable_bytes();
        cold_vk
            .verify(&signable, &self.sigma)
            .map_err(|_| ConsensusError::InvalidOpCertSignature)
    }
}

/// Compute the KES period for a given slot number.
///
/// ```text
/// kes_period = slot_no / slots_per_kes_period
/// ```
///
/// # Errors
///
/// Returns `InvalidSlotsPerKesPeriod` if `slots_per_kes_period` is zero.
pub fn kes_period_of_slot(slot: u64, slots_per_kes_period: u64) -> Result<u64, ConsensusError> {
    if slots_per_kes_period == 0 {
        return Err(ConsensusError::InvalidSlotsPerKesPeriod);
    }
    Ok(slot / slots_per_kes_period)
}

/// Check that the current KES period falls within the validity window of an
/// operational certificate.
///
/// The certificate is valid when:
/// ```text
/// opcert.kes_period <= current_kes_period < opcert.kes_period + max_kes_evolutions
/// ```
pub fn check_kes_period(
    opcert: &OpCert,
    current_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<(), ConsensusError> {
    if current_kes_period < opcert.kes_period {
        return Err(ConsensusError::KesPeriodTooEarly {
            current: current_kes_period,
            cert_start: opcert.kes_period,
        });
    }
    let end = opcert
        .kes_period
        .checked_add(max_kes_evolutions)
        .ok_or(ConsensusError::KesPeriodOverflow)?;
    if current_kes_period >= end {
        return Err(ConsensusError::KesPeriodExpired {
            current: current_kes_period,
            cert_end: end,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// OcertCounters — monotonic OpCert sequence-number tracking
// ---------------------------------------------------------------------------

/// Tracks the highest observed operational-certificate sequence number per
/// pool (keyed by the Blake2b-224 hash of the issuer cold key).
///
/// When a new block arrives the counter is validated using the same rules as
/// the upstream `currentIssueNo` helper in
/// `Ouroboros.Consensus.Protocol.Praos`:
///
/// 1. If the pool is already in the counter map, the new sequence number
///    `n` must satisfy  `stored ≤ n ≤ stored + 1`.
/// 2. If the pool is **not** in the counter map but **is** present in the
///    stake distribution, the counter is initialized — the block is the
///    first we have seen from this pool.
/// 3. If the pool is in neither, `NoCounterForKeyHash` is returned.
///
/// Reference: `PraosState.csCounters` and `currentIssueNo` in
/// `Ouroboros.Consensus.Protocol.Praos`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OcertCounters {
    counters: BTreeMap<[u8; 28], u64>,
}

impl OcertCounters {
    /// Returns a fresh (empty) counter map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates a block's OpCert sequence number and updates the map on
    /// success.
    ///
    /// # Arguments
    ///
    /// * `pool_key_hash` — Blake2b-224 of the block issuer's cold
    ///   verification key.
    /// * `new_seq` — The `sequence_number` from the block's `OpCert`.
    /// * `pool_in_dist` — Whether the pool appears in the current stake
    ///   distribution (used only when the pool is not yet tracked).
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
        if let Some(&stored) = self.counters.get(&pool_key_hash) {
            if new_seq < stored {
                return Err(ConsensusError::OcertCounterTooOld {
                    stored,
                    received: new_seq,
                });
            }
            if new_seq > stored.saturating_add(1) {
                return Err(ConsensusError::OcertCounterTooFar {
                    stored,
                    received: new_seq,
                });
            }
            // stored ≤ new_seq ≤ stored + 1 — accept.
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

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_crypto::ed25519::SigningKey;

    fn mk_valid_opcert(kes_period: u64, seq: u64) -> (OpCert, VerificationKey) {
        let cold_sk = SigningKey::from_bytes([0x42; 32]);
        let cold_vk = cold_sk.verification_key().unwrap();
        let hot_vkey = SumKesVerificationKey::from_bytes([0xBB; 32]);
        let mut oc = OpCert {
            hot_vkey,
            sequence_number: seq,
            kes_period,
            sigma: Signature::from_bytes([0; 64]),
        };
        let signable = oc.signable_bytes();
        oc.sigma = cold_sk.sign(&signable).unwrap();
        (oc, cold_vk)
    }

    // ── existing tests ───────────────────────────────────────────────

    #[test]
    fn kes_period_of_slot_mainnet() {
        assert_eq!(kes_period_of_slot(0, 129_600).expect("valid"), 0);
        assert_eq!(kes_period_of_slot(129_599, 129_600).expect("valid"), 0);
        assert_eq!(kes_period_of_slot(129_600, 129_600).expect("valid"), 1);
        assert_eq!(kes_period_of_slot(259_200, 129_600).expect("valid"), 2);
    }

    #[test]
    fn kes_period_of_slot_rejects_zero() {
        assert_eq!(
            kes_period_of_slot(100, 0),
            Err(ConsensusError::InvalidSlotsPerKesPeriod)
        );
    }

    // ── signable_bytes ───────────────────────────────────────────────

    #[test]
    fn signable_bytes_length_is_48() {
        let (oc, _) = mk_valid_opcert(0, 0);
        assert_eq!(oc.signable_bytes().len(), 48);
    }

    #[test]
    fn signable_bytes_deterministic() {
        let (oc, _) = mk_valid_opcert(5, 10);
        assert_eq!(oc.signable_bytes(), oc.signable_bytes());
    }

    #[test]
    fn signable_bytes_differ_for_different_kes_period() {
        let (oc1, _) = mk_valid_opcert(0, 0);
        let oc2_hot = SumKesVerificationKey::from_bytes([0xBB; 32]);
        let oc2 = OpCert {
            hot_vkey: oc2_hot,
            sequence_number: 0,
            kes_period: 5,
            sigma: Signature::from_bytes([0; 64]),
        };
        assert_ne!(oc1.signable_bytes(), oc2.signable_bytes());
    }

    #[test]
    fn signable_bytes_differ_for_different_sequence() {
        let oc1 = OpCert {
            hot_vkey: SumKesVerificationKey::from_bytes([0xBB; 32]),
            sequence_number: 0,
            kes_period: 0,
            sigma: Signature::from_bytes([0; 64]),
        };
        let oc2 = OpCert {
            hot_vkey: SumKesVerificationKey::from_bytes([0xBB; 32]),
            sequence_number: 1,
            kes_period: 0,
            sigma: Signature::from_bytes([0; 64]),
        };
        assert_ne!(oc1.signable_bytes(), oc2.signable_bytes());
    }

    #[test]
    fn signable_bytes_layout() {
        let hot = [0xAA; 32];
        let oc = OpCert {
            hot_vkey: SumKesVerificationKey::from_bytes(hot),
            sequence_number: 1,
            kes_period: 2,
            sigma: Signature::from_bytes([0; 64]),
        };
        let buf = oc.signable_bytes();
        // First 32 bytes: hot_vkey
        assert_eq!(&buf[..32], &hot);
        // Next 8 bytes: sequence_number BE
        assert_eq!(&buf[32..40], &1u64.to_be_bytes());
        // Last 8 bytes: kes_period BE
        assert_eq!(&buf[40..48], &2u64.to_be_bytes());
    }

    // ── verify ───────────────────────────────────────────────────────

    #[test]
    fn verify_valid_opcert() {
        let (oc, cold_vk) = mk_valid_opcert(0, 0);
        assert!(oc.verify(&cold_vk).is_ok());
    }

    #[test]
    fn verify_wrong_cold_key_fails() {
        let (oc, _) = mk_valid_opcert(0, 0);
        let wrong_vk = VerificationKey::from_bytes([0xFF; 32]);
        assert_eq!(
            oc.verify(&wrong_vk),
            Err(ConsensusError::InvalidOpCertSignature)
        );
    }

    #[test]
    fn verify_tampered_hot_vkey_fails() {
        let (mut oc, cold_vk) = mk_valid_opcert(0, 0);
        oc.hot_vkey = SumKesVerificationKey::from_bytes([0xFF; 32]);
        assert_eq!(
            oc.verify(&cold_vk),
            Err(ConsensusError::InvalidOpCertSignature)
        );
    }

    #[test]
    fn verify_tampered_sequence_fails() {
        let (mut oc, cold_vk) = mk_valid_opcert(0, 0);
        oc.sequence_number = 999;
        assert_eq!(
            oc.verify(&cold_vk),
            Err(ConsensusError::InvalidOpCertSignature)
        );
    }

    #[test]
    fn verify_tampered_kes_period_fails() {
        let (mut oc, cold_vk) = mk_valid_opcert(0, 0);
        oc.kes_period = 999;
        assert_eq!(
            oc.verify(&cold_vk),
            Err(ConsensusError::InvalidOpCertSignature)
        );
    }

    // ── check_kes_period ─────────────────────────────────────────────

    #[test]
    fn check_kes_period_valid() {
        let (oc, _) = mk_valid_opcert(5, 0);
        // Current period 5 is at the start of the window
        assert!(check_kes_period(&oc, 5, 62).is_ok());
        // Current period 10 is within the window
        assert!(check_kes_period(&oc, 10, 62).is_ok());
        // Current period 66 is at the end (5 + 62 - 1)
        assert!(check_kes_period(&oc, 66, 62).is_ok());
    }

    #[test]
    fn check_kes_period_too_early() {
        let (oc, _) = mk_valid_opcert(10, 0);
        let err = check_kes_period(&oc, 5, 62).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::KesPeriodTooEarly {
                current: 5,
                cert_start: 10,
            }
        );
    }

    #[test]
    fn check_kes_period_expired() {
        let (oc, _) = mk_valid_opcert(0, 0);
        // max_evolutions=10, so valid range is [0, 10)
        let err = check_kes_period(&oc, 10, 10).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::KesPeriodExpired {
                current: 10,
                cert_end: 10,
            }
        );
    }

    #[test]
    fn check_kes_period_boundary_just_before_expiry() {
        let (oc, _) = mk_valid_opcert(0, 0);
        // max_evolutions=10, period 9 is the last valid period
        assert!(check_kes_period(&oc, 9, 10).is_ok());
    }

    #[test]
    fn check_kes_period_rejects_overflow() {
        // When `opcert.kes_period + max_kes_evolutions` would overflow u64,
        // the checked_add-based computation returns `None` and yields
        // `KesPeriodOverflow` instead of silently wrapping to a smaller
        // period that might accept or reject the wrong way.
        //
        // Use opcert.kes_period = u64::MAX - 5 and max_kes_evolutions = 10
        // so the sum overflows into the "wrapped" range [0, 4].
        let (mut oc, _) = mk_valid_opcert(u64::MAX - 5, 0);
        // Even current_kes_period = u64::MAX - 5 (equal to cert_start)
        // must not slip through via wraparound — the overflow guard fires
        // before the <= comparison.
        oc.kes_period = u64::MAX - 5;
        assert_eq!(
            check_kes_period(&oc, u64::MAX - 5, 10),
            Err(ConsensusError::KesPeriodOverflow)
        );
        // At the exact non-overflow boundary `max_kes_evolutions = 5`,
        // `u64::MAX - 5 + 5 == u64::MAX` is representable, so the function
        // must return `Ok` for a current period strictly below u64::MAX.
        assert!(check_kes_period(&oc, u64::MAX - 5, 5).is_ok());
    }

    #[test]
    fn kes_period_of_slot_large_values() {
        // Verify it works without overflow for large slot numbers
        let period = kes_period_of_slot(u64::MAX / 2, 1).unwrap();
        assert_eq!(period, u64::MAX / 2);
    }

    // ── OcertCounters ────────────────────────────────────────────────

    #[test]
    fn ocert_counters_new_is_empty() {
        let c = OcertCounters::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn ocert_counters_first_seen_pool_in_dist_accepted() {
        let mut c = OcertCounters::new();
        let pool = [0x01; 28];
        assert!(c.validate_and_update(pool, 5, true).is_ok());
        assert_eq!(c.get(&pool), Some(5));
    }

    #[test]
    fn ocert_counters_first_seen_pool_not_in_dist_rejected() {
        let mut c = OcertCounters::new();
        let pool = [0x02; 28];
        let err = c.validate_and_update(pool, 5, false).unwrap_err();
        assert_eq!(err, ConsensusError::NoCounterForKeyHash { hash: pool });
        assert!(c.is_empty());
    }

    #[test]
    fn ocert_counters_same_seq_accepted() {
        let mut c = OcertCounters::new();
        let pool = [0x03; 28];
        c.validate_and_update(pool, 10, true).unwrap();
        // Re-submitting the same sequence number is valid.
        assert!(c.validate_and_update(pool, 10, true).is_ok());
        assert_eq!(c.get(&pool), Some(10));
    }

    #[test]
    fn ocert_counters_increment_by_one_accepted() {
        let mut c = OcertCounters::new();
        let pool = [0x04; 28];
        c.validate_and_update(pool, 10, true).unwrap();
        assert!(c.validate_and_update(pool, 11, true).is_ok());
        assert_eq!(c.get(&pool), Some(11));
    }

    #[test]
    fn ocert_counters_too_old_rejected() {
        let mut c = OcertCounters::new();
        let pool = [0x05; 28];
        c.validate_and_update(pool, 10, true).unwrap();
        let err = c.validate_and_update(pool, 9, true).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::OcertCounterTooOld {
                stored: 10,
                received: 9,
            }
        );
        // Counter should NOT have changed.
        assert_eq!(c.get(&pool), Some(10));
    }

    #[test]
    fn ocert_counters_too_far_rejected() {
        let mut c = OcertCounters::new();
        let pool = [0x06; 28];
        c.validate_and_update(pool, 10, true).unwrap();
        let err = c.validate_and_update(pool, 12, true).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::OcertCounterTooFar {
                stored: 10,
                received: 12,
            }
        );
        // Counter should NOT have changed.
        assert_eq!(c.get(&pool), Some(10));
    }

    #[test]
    fn ocert_counters_multiple_pools_independent() {
        let mut c = OcertCounters::new();
        let pool_a = [0x0A; 28];
        let pool_b = [0x0B; 28];
        c.validate_and_update(pool_a, 1, true).unwrap();
        c.validate_and_update(pool_b, 100, true).unwrap();
        assert_eq!(c.len(), 2);

        // Increment pool_a, pool_b stays unchanged.
        c.validate_and_update(pool_a, 2, true).unwrap();
        assert_eq!(c.get(&pool_a), Some(2));
        assert_eq!(c.get(&pool_b), Some(100));
    }

    #[test]
    fn ocert_counters_zero_seq_accepted_for_new_pool() {
        let mut c = OcertCounters::new();
        let pool = [0x07; 28];
        assert!(c.validate_and_update(pool, 0, true).is_ok());
        assert_eq!(c.get(&pool), Some(0));
    }

    // ── OcertCounters CBOR codec (sidecar persistence) ───────────────────

    #[test]
    fn ocert_counters_cbor_round_trip_empty() {
        use yggdrasil_ledger::cbor::{CborDecode, CborEncode};
        let original = OcertCounters::new();
        let bytes = original.to_cbor_bytes();
        // Empty CBOR map is a single byte: 0xa0.
        assert_eq!(bytes, vec![0xa0]);
        let decoded = OcertCounters::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn ocert_counters_cbor_round_trip_multiple_pools() {
        use yggdrasil_ledger::cbor::{CborDecode, CborEncode};
        let mut original = OcertCounters::new();
        original.validate_and_update([0x01; 28], 0, true).unwrap();
        original.validate_and_update([0x01; 28], 1, true).unwrap();
        original.validate_and_update([0x01; 28], 2, true).unwrap();
        original.validate_and_update([0x42; 28], 17, true).unwrap();
        original.validate_and_update([0xff; 28], 99, true).unwrap();
        let bytes = original.to_cbor_bytes();
        let decoded = OcertCounters::from_cbor_bytes(&bytes).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(decoded.get(&[0x01; 28]), Some(2));
        assert_eq!(decoded.get(&[0x42; 28]), Some(17));
        assert_eq!(decoded.get(&[0xff; 28]), Some(99));
    }

    #[test]
    fn ocert_counters_cbor_decode_rejects_short_key() {
        use yggdrasil_ledger::cbor::CborDecode;
        // A 1-element map whose key is 16 bytes (not 28) — must reject.
        // 0xa1 = map(1), 0x50 = bytes(16), 16 zeros, 0x00 = unsigned(0).
        let mut bad = vec![0xa1, 0x50];
        bad.extend(std::iter::repeat_n(0u8, 16));
        bad.push(0x00);
        assert!(OcertCounters::from_cbor_bytes(&bad).is_err());
    }

    #[test]
    fn ocert_counters_cbor_encoding_is_deterministic_in_btree_order() {
        use yggdrasil_ledger::cbor::CborEncode;
        // Two counters built via different insertion orders must yield
        // identical CBOR bytes — the BTreeMap iterator order is the
        // canonical wire order, so a future regression to a HashMap
        // backing store would fail here.
        let mut a = OcertCounters::new();
        a.validate_and_update([0x99; 28], 1, true).unwrap();
        a.validate_and_update([0x11; 28], 2, true).unwrap();
        a.validate_and_update([0x55; 28], 3, true).unwrap();

        let mut b = OcertCounters::new();
        b.validate_and_update([0x11; 28], 2, true).unwrap();
        b.validate_and_update([0x55; 28], 3, true).unwrap();
        b.validate_and_update([0x99; 28], 1, true).unwrap();

        assert_eq!(a.to_cbor_bytes(), b.to_cbor_bytes());
    }
}
