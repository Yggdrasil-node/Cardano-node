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
}
