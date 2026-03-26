use yggdrasil_ledger::HeaderHash;

use thiserror::Error;

/// Errors returned by consensus-facing helpers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConsensusError {
    /// The active slot coefficient is outside the valid `[0, 1]` range.
    #[error("invalid active slot coefficient")]
    InvalidActiveSlotCoeff,
    /// A VRF proof was structurally invalid or failed verification.
    #[error("invalid VRF proof")]
    InvalidVrfProof,
    /// The cold-key signature on an operational certificate is invalid.
    #[error("invalid operational certificate signature")]
    InvalidOpCertSignature,
    /// The KES signature on a block header is invalid.
    #[error("invalid KES signature")]
    InvalidKesSignature,
    /// The current KES period is before the certificate's start period.
    #[error("KES period too early: current {current}, cert starts at {cert_start}")]
    KesPeriodTooEarly {
        /// The KES period derived from the current slot.
        current: u64,
        /// The KES period at which the certificate becomes valid.
        cert_start: u64,
    },
    /// The current KES period is at or past the certificate's end period.
    #[error("KES period expired: current {current}, cert ends at {cert_end}")]
    KesPeriodExpired {
        /// The KES period derived from the current slot.
        current: u64,
        /// The exclusive upper bound of the certificate's KES window.
        cert_end: u64,
    },
    /// KES period arithmetic overflowed.
    #[error("KES period overflow")]
    KesPeriodOverflow,
    /// `slots_per_kes_period` was set to zero.
    #[error("invalid slots per KES period (zero)")]
    InvalidSlotsPerKesPeriod,
    /// A rollback was requested that exceeds the security parameter `k`.
    #[error("rollback too deep: requested {requested} blocks, max is {max}")]
    RollbackTooDeep {
        /// The number of blocks requested for rollback.
        requested: u64,
        /// The security parameter `k` (maximum allowed rollback depth).
        max: u64,
    },
    /// The rollback target point was not found in the volatile chain.
    #[error("rollback point not found: slot {slot}, hash {hash:?}")]
    RollbackPointNotFound {
        /// Slot of the requested rollback target.
        slot: u64,
        /// Header hash of the requested rollback target.
        hash: HeaderHash,
    },
    /// A `roll_forward` block number does not follow the current tip.
    #[error("non-contiguous block: expected {expected}, got {got}")]
    NonContiguousBlock {
        /// The block number that was expected.
        expected: u64,
        /// The block number that was received.
        got: u64,
    },
    /// The VRF leader eligibility check failed — the block issuer's VRF
    /// output does not meet the leader threshold for their relative stake.
    #[error("VRF leader eligibility check failed")]
    VrfLeaderCheckFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rollback_too_deep() {
        let e = ConsensusError::RollbackTooDeep {
            requested: 100,
            max: 10,
        };
        let s = format!("{e}");
        assert!(s.contains("100"));
        assert!(s.contains("10"));
    }

    #[test]
    fn display_non_contiguous() {
        let e = ConsensusError::NonContiguousBlock {
            expected: 42,
            got: 99,
        };
        let s = format!("{e}");
        assert!(s.contains("42"));
        assert!(s.contains("99"));
    }

    #[test]
    fn display_kes_period_too_early() {
        let e = ConsensusError::KesPeriodTooEarly {
            current: 5,
            cert_start: 10,
        };
        let s = format!("{e}");
        assert!(s.contains("5"));
        assert!(s.contains("10"));
    }

    #[test]
    fn display_kes_period_expired() {
        let e = ConsensusError::KesPeriodExpired {
            current: 100,
            cert_end: 50,
        };
        let s = format!("{e}");
        assert!(s.contains("100"));
        assert!(s.contains("50"));
    }

    #[test]
    fn display_rollback_point_not_found() {
        let e = ConsensusError::RollbackPointNotFound {
            slot: 42,
            hash: HeaderHash([0xAB; 32]),
        };
        let s = format!("{e}");
        assert!(s.contains("42"));
    }

    #[test]
    fn all_variants_are_displayable() {
        let variants: Vec<ConsensusError> = vec![
            ConsensusError::InvalidActiveSlotCoeff,
            ConsensusError::InvalidVrfProof,
            ConsensusError::InvalidOpCertSignature,
            ConsensusError::InvalidKesSignature,
            ConsensusError::KesPeriodOverflow,
            ConsensusError::InvalidSlotsPerKesPeriod,
            ConsensusError::VrfLeaderCheckFailed,
        ];
        for v in &variants {
            assert!(!format!("{v}").is_empty());
        }
    }

    #[test]
    fn error_is_eq() {
        assert_eq!(
            ConsensusError::InvalidVrfProof,
            ConsensusError::InvalidVrfProof
        );
        assert_ne!(
            ConsensusError::InvalidVrfProof,
            ConsensusError::InvalidKesSignature
        );
    }
}
