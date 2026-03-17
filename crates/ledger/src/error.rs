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

    #[error("invalid reward account bytes: {0:02x?}")]
    InvalidRewardAccountBytes(Vec<u8>),

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

    // -- Fee validation errors ----------------------------------------------

    #[error("fee too small: minimum {minimum} lovelace, declared {declared}")]
    FeeTooSmall { minimum: u64, declared: u64 },

    #[error(
        "execution units exceed per-tx limit: mem {tx_mem}/{max_mem}, steps {tx_steps}/{max_steps}"
    )]
    ExUnitsExceedTxLimit {
        tx_mem: u64,
        tx_steps: u64,
        max_mem: u64,
        max_steps: u64,
    },

    #[error("transaction too large: {actual} bytes exceeds max {max}")]
    TxTooLarge { actual: usize, max: usize },

    // -- Output validation errors -------------------------------------------

    #[error("output too small: minimum {minimum} lovelace, actual {actual}")]
    OutputTooSmall { minimum: u64, actual: u64 },

    // -- Script validation errors -------------------------------------------

    #[error("native script not satisfied: script hash {hash:02x?}")]
    NativeScriptFailed { hash: [u8; 28] },

    // -- Plutus script validation errors ------------------------------------

    #[error("Plutus script evaluation failed: script hash {hash:02x?}: {reason}")]
    PlutusScriptFailed { hash: [u8; 28], reason: String },

    #[error("Plutus script not found for script hash {hash:02x?}")]
    PlutusScriptNotFound { hash: [u8; 28] },

    #[error("no matching redeemer for script hash {hash:02x?} (purpose {purpose})")]
    MissingRedeemer { hash: [u8; 28], purpose: String },

    #[error("datum not found for spending input (tx {tx_id:02x?} index {index})")]
    MissingDatum { tx_id: [u8; 32], index: u64 },

    #[error("Plutus script decode failed for script hash {hash:02x?}: {reason}")]
    PlutusScriptDecodeError { hash: [u8; 28], reason: String },

    // -- Collateral validation errors ---------------------------------------

    #[error("no collateral inputs in script transaction")]
    NoCollateralInputs,

    #[error("too many collateral inputs: {count} exceeds max {max}")]
    TooManyCollateralInputs { count: usize, max: u32 },

    #[error("collateral input not found in UTxO set")]
    CollateralInputNotInUtxo,

    #[error(
        "insufficient collateral: required {required} lovelace (fee {fee} × {percentage}%), \
         provided {provided}"
    )]
    InsufficientCollateral {
        fee: u64,
        percentage: u64,
        required: u64,
        provided: u64,
    },

    #[error("collateral output contains non-ADA assets")]
    CollateralContainsNonAda,

    // -- Block validation errors --------------------------------------------

    #[error("block body too large: {actual} bytes exceeds max {max}")]
    BlockTooLarge { actual: usize, max: usize },

    #[error(
        "block execution units exceed limit: mem {block_mem}/{max_mem}, \
         steps {block_steps}/{max_steps}"
    )]
    BlockExUnitsExceeded {
        block_mem: u64,
        block_steps: u64,
        max_mem: u64,
        max_steps: u64,
    },

    // -- Witness validation errors ------------------------------------------

    #[error("missing required VKey witness for hash {hash:02x?}")]
    MissingVKeyWitness { hash: [u8; 28] },

    #[error("VKey witness signature verification failed for hash {hash:02x?}")]
    InvalidVKeyWitnessSignature { hash: [u8; 28] },

    // -- Epoch boundary errors ----------------------------------------------

    #[error("protocol parameters are required but missing")]
    MissingProtocolParameters,
}
