use thiserror::Error;

/// Errors returned by ledger-facing helpers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LedgerError {
    #[error("unsupported era: {0:?}")]
    UnsupportedEra(super::eras::Era),

    // -- CBOR errors --------------------------------------------------------

    #[error("CBOR: unexpected end of input")]
    CborUnexpectedEof,

    #[error("CBOR: type mismatch (expected major {expected}, got {actual})")]
    CborTypeMismatch { expected: u8, actual: u8 },

    #[error("CBOR: invalid additional info {0}")]
    CborInvalidAdditionalInfo(u8),

    #[error("CBOR: invalid length (expected {expected}, got {actual})")]
    CborInvalidLength { expected: usize, actual: usize },

    #[error("CBOR: {0} trailing bytes after value")]
    CborTrailingBytes(usize),

    // -- UTxO validation errors ---------------------------------------------

    #[error("transaction expired: TTL {ttl} < current slot {slot}")]
    TxExpired { ttl: u64, slot: u64 },

    #[error("input not found in UTxO set")]
    InputNotInUtxo,

    #[error(
        "value not preserved: consumed {consumed} lovelace != produced {produced} + fee {fee}"
    )]
    ValueNotPreserved {
        consumed: u64,
        produced: u64,
        fee: u64,
    },

    #[error("no inputs in transaction")]
    NoInputs,

    #[error("no outputs in transaction")]
    NoOutputs,
}
