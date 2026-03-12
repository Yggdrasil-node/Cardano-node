use thiserror::Error;

/// Errors returned by ledger-facing helpers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LedgerError {
    #[error("unsupported era: {0:?}")]
    UnsupportedEra(super::eras::Era),
}
