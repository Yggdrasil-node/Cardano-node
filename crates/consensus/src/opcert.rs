//! Operational certificate (OpCert) types, KES period helpers, and
//! sequence-number counter tracking.
//!
//! An operational certificate binds a cold (offline) Ed25519 key to a hot
//! KES verification key for a window of KES periods.  The cold key signs
//! `hot_vkey || sequence_number || kes_period` to produce the certificate
//! signature.
//!
//! Sub-modules:
//!
//! - [`cert`] — `OpCert` struct + `kes_period_of_slot` + `check_kes_period`.
//! - [`counter`] — `OcertCounters` + `OcertCounterRule` (era-aware monotonic
//!   counter validation).
//!
//! Reference: `Cardano.Protocol.TPraos.OCert` (struct + helpers) +
//! `Cardano.Protocol.TPraos.Rules.OCert` (TPraos counter rule) +
//! `Ouroboros.Consensus.Protocol.Praos` (Praos counter rule).

pub mod cert;
pub mod counter;

pub use cert::{OpCert, check_kes_period, kes_period_of_slot};
pub use counter::{OcertCounterRule, OcertCounters};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ConsensusError;
    use yggdrasil_crypto::ed25519::{Signature, SigningKey, VerificationKey};
    use yggdrasil_crypto::sum_kes::SumKesVerificationKey;

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

    /// `clear()` empties the counter map and the next sequence number
    /// from any pool is accepted (subject to the existing `pool_in_dist`
    /// permissive rule). Pins the rollback-reset contract: after a chain
    /// rollback, OpCerts that were rejected as `OcertCounterTooOld`
    /// against the pre-rollback high-water mark must now be accepted as
    /// "first-seen" because the fork's chain may legitimately have lower
    /// sequence numbers from the same pool.
    #[test]
    fn ocert_counters_clear_resets_to_empty_and_accepts_next_block_as_first_seen() {
        let mut c = OcertCounters::new();
        let pool = [0xAA; 28];
        // Advance to seq 5.
        c.validate_and_update(pool, 0, true).unwrap();
        c.validate_and_update(pool, 1, true).unwrap();
        c.validate_and_update(pool, 2, true).unwrap();
        c.validate_and_update(pool, 3, true).unwrap();
        c.validate_and_update(pool, 4, true).unwrap();
        c.validate_and_update(pool, 5, true).unwrap();
        assert_eq!(c.get(&pool), Some(5));

        // Pre-clear: replaying seq 2 fails as TooOld (5 → 2).
        assert!(c.validate_and_update(pool, 2, true).is_err());

        // Reset.
        c.clear();
        assert!(c.is_empty());
        assert_eq!(c.get(&pool), None);

        // Post-clear: same lower-seq block is now accepted as first-seen
        // (the alt-chain scenario after a fork past the high-water mark).
        c.validate_and_update(pool, 2, true).unwrap();
        assert_eq!(c.get(&pool), Some(2));
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

    // ── Era-aware counter rule tests ──────────────────────────────────────

    #[test]
    fn ocert_rule_for_pv_major_picks_tpraos_below_vasil() {
        // PV majors 2..=6 = Shelley/Allegra/Mary/Alonzo (TPraos era).
        for pv in 2..=6u64 {
            assert_eq!(
                OcertCounterRule::for_pv_major(pv),
                OcertCounterRule::TPraos,
                "PV {} should be TPraos",
                pv
            );
        }
    }

    #[test]
    fn ocert_rule_for_pv_major_picks_praos_at_and_above_vasil() {
        // PV major 7 = Babbage/Vasil onward = Praos.
        for pv in 7..=12u64 {
            assert_eq!(
                OcertCounterRule::for_pv_major(pv),
                OcertCounterRule::Praos,
                "PV {} should be Praos",
                pv
            );
        }
    }

    #[test]
    fn tpraos_counter_accepts_arbitrary_forward_jump() {
        // Pre-Babbage TPraos rule: only `stored ≤ new_seq` is enforced.
        // A jump of more than +1 should pass under TPraos.
        // Reference: `Cardano.Protocol.TPraos.Rules.OCert.ocertTransition`:
        //   m <= n ?! CounterTooSmallOCERT m n
        // (no upper-bound check, unlike Praos).
        let mut c = OcertCounters::new();
        c.validate_and_update_with_rule([0x42; 28], 5, true, OcertCounterRule::TPraos)
            .expect("initial registration");
        // Jump 5 → 10 (delta = 5) — Praos would reject, TPraos accepts.
        c.validate_and_update_with_rule([0x42; 28], 10, true, OcertCounterRule::TPraos)
            .expect("forward jump should be accepted under TPraos");
        assert_eq!(c.get(&[0x42; 28]), Some(10));
    }

    #[test]
    fn praos_counter_rejects_forward_jump_over_one() {
        // Babbage+ Praos rule: `stored ≤ new_seq ≤ stored + 1`.
        // Same forward jump from the test above must fail under Praos.
        // Reference: `Ouroboros.Consensus.Protocol.Praos`:
        //   m <= n ?! CounterTooSmallOCERT m n
        //   n <= m + 1 ?! CounterOverIncrementedOCERT m n
        let mut c = OcertCounters::new();
        c.validate_and_update_with_rule([0x42; 28], 5, true, OcertCounterRule::Praos)
            .expect("initial registration");
        let err = c
            .validate_and_update_with_rule([0x42; 28], 10, true, OcertCounterRule::Praos)
            .expect_err("forward jump should be rejected under Praos");
        assert!(matches!(err, ConsensusError::OcertCounterTooFar { .. }));
        // Counter must NOT advance after a rejected update.
        assert_eq!(c.get(&[0x42; 28]), Some(5));
    }

    #[test]
    fn tpraos_counter_still_rejects_backwards_movement() {
        // Lower-bound check `stored ≤ new_seq` is enforced under BOTH rules.
        let mut c = OcertCounters::new();
        c.validate_and_update_with_rule([0x42; 28], 10, true, OcertCounterRule::TPraos)
            .unwrap();
        let err = c
            .validate_and_update_with_rule([0x42; 28], 9, true, OcertCounterRule::TPraos)
            .expect_err("backward move should be rejected");
        assert!(matches!(err, ConsensusError::OcertCounterTooOld { .. }));
    }

    #[test]
    fn praos_counter_accepts_increment_by_one() {
        // The standard valid case under Praos: counter advances by exactly 1.
        let mut c = OcertCounters::new();
        c.validate_and_update_with_rule([0x42; 28], 5, true, OcertCounterRule::Praos)
            .unwrap();
        c.validate_and_update_with_rule([0x42; 28], 6, true, OcertCounterRule::Praos)
            .expect("increment-by-one should be accepted under Praos");
        assert_eq!(c.get(&[0x42; 28]), Some(6));
    }

    #[test]
    fn validate_and_update_alias_uses_praos_rule() {
        // The unparametrised `validate_and_update` MUST behave identically to
        // the Praos branch — this protects existing callers (preview / post-
        // Vasil mainnet) from accidental relaxation.
        let mut c = OcertCounters::new();
        c.validate_and_update([0x42; 28], 5, true).unwrap();
        let err = c
            .validate_and_update([0x42; 28], 7, true)
            .expect_err("unparametrised validate_and_update must enforce the Praos upper bound");
        assert!(matches!(err, ConsensusError::OcertCounterTooFar { .. }));
    }
}
