use thiserror::Error;

/// Errors returned by consensus-facing helpers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConsensusError {
    #[error("invalid active slot coefficient")]
    InvalidActiveSlotCoeff,
}
