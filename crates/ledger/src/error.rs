use thiserror::Error;

use crate::types::{DRep, PoolKeyHash, RewardAccount, StakeCredential};

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

    #[error("transaction not yet valid: validity start {start} > current slot {slot}")]
    TxNotYetValid { start: u64, slot: u64 },

    #[error("stake pool not registered: {0:02x?}")]
    PoolNotRegistered(PoolKeyHash),

    #[error("stake credential already registered: {0:?}")]
    StakeCredentialAlreadyRegistered(StakeCredential),

    #[error("stake credential not registered: {0:?}")]
    StakeCredentialNotRegistered(StakeCredential),

    #[error("drep already registered: {0:?}")]
    DrepAlreadyRegistered(DRep),

    #[error("drep not registered: {0:?}")]
    DrepNotRegistered(DRep),

    #[error("committee cold credential is unknown: {0:?}")]
    CommitteeIsUnknown(StakeCredential),

    #[error("committee cold credential has previously resigned: {0:?}")]
    CommitteeHasPreviouslyResigned(StakeCredential),

    #[error(
        "stake credential has non-zero reward balance: {credential:?} has {balance} lovelace"
    )]
    StakeCredentialHasRewards {
        credential: StakeCredential,
        balance: u64,
    },

    #[error("reward account not registered: {0:?}")]
    RewardAccountNotRegistered(RewardAccount),

    #[error(
        "withdrawal exceeds reward balance for {account:?}: requested {requested}, available {available}"
    )]
    WithdrawalExceedsBalance {
        account: RewardAccount,
        requested: u64,
        available: u64,
    },

    #[error("unsupported certificate kind in this ledger slice: {0}")]
    UnsupportedCertificate(&'static str),

    #[error(
        "multi-asset not preserved for policy {policy:02x?} / asset {asset_name:02x?}: \
         expected {expected}, produced {produced}"
    )]
    MultiAssetNotPreserved {
        policy: [u8; 28],
        asset_name: Vec<u8>,
        expected: u64,
        produced: u64,
    },
}
