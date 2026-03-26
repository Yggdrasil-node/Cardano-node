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
        assert_eq!(oc.verify(&wrong_vk), Err(ConsensusError::InvalidOpCertSignature));
    }

    #[test]
    fn verify_tampered_hot_vkey_fails() {
        let (mut oc, cold_vk) = mk_valid_opcert(0, 0);
        oc.hot_vkey = SumKesVerificationKey::from_bytes([0xFF; 32]);
        assert_eq!(oc.verify(&cold_vk), Err(ConsensusError::InvalidOpCertSignature));
    }

    #[test]
    fn verify_tampered_sequence_fails() {
        let (mut oc, cold_vk) = mk_valid_opcert(0, 0);
        oc.sequence_number = 999;
        assert_eq!(oc.verify(&cold_vk), Err(ConsensusError::InvalidOpCertSignature));
    }

    #[test]
    fn verify_tampered_kes_period_fails() {
        let (mut oc, cold_vk) = mk_valid_opcert(0, 0);
        oc.kes_period = 999;
        assert_eq!(oc.verify(&cold_vk), Err(ConsensusError::InvalidOpCertSignature));
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
    fn kes_period_of_slot_large_values() {
        // Verify it works without overflow for large slot numbers
        let period = kes_period_of_slot(u64::MAX / 2, 1).unwrap();
        assert_eq!(period, u64::MAX / 2);
    }
}
