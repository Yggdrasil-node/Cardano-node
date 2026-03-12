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
}
