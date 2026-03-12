use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConsensusError {
    #[error("invalid active slot coefficient")]
    InvalidActiveSlotCoeff,
}
