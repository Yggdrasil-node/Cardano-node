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

    #[test]
    fn kes_period_of_slot_mainnet() {
        // Mainnet: slotsPerKESPeriod = 129600 (36 hours)
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
}
