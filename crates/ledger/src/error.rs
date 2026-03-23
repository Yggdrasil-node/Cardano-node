use thiserror::Error;

use crate::types::{DRep, EpochNo, PoolKeyHash, RewardAccount, StakeCredential};

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

    #[error("block slot {block_slot} does not advance past current tip slot {tip_slot}")]
    SlotNotIncreasing { tip_slot: u64, block_slot: u64 },

    #[error("transaction expired: TTL {ttl} < current slot {slot}")]
    TxExpired { ttl: u64, slot: u64 },

    #[error("input not found in UTxO set")]
    InputNotInUtxo,

    #[error("reference input not found in UTxO set")]
    ReferenceInputNotInUtxo,

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

    #[error("pool cost {cost} is below minPoolCost {min_pool_cost}")]
    PoolCostTooLow { cost: u64, min_pool_cost: u64 },

    #[error("pool margin numerator {numerator} exceeds denominator {denominator}")]
    PoolMarginInvalid { numerator: u64, denominator: u64 },

    #[error("pool reward account network {actual} does not match expected {expected}")]
    PoolRewardAccountNetworkMismatch { actual: u8, expected: u8 },

    #[error("pool metadata URL too long: {length} bytes (max 64)")]
    PoolMetadataUrlTooLong { length: usize },

    #[error("pool retirement epoch {retirement_epoch} exceeds maximum {max_epoch} (current {current_epoch} + eMax {e_max})")]
    PoolRetirementTooFar {
        retirement_epoch: u64,
        current_epoch: u64,
        e_max: u32,
        max_epoch: u64,
    },

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

    #[error("invalid address bytes: {0:02x?}")]
    InvalidAddressBytes(Vec<u8>),

    #[error("invalid address network id: {0}")]
    InvalidAddressNetworkId(u8),

    #[error("invalid Byron address structure: {0:02x?}")]
    InvalidByronAddressStructure(Vec<u8>),

    #[error("invalid Byron address CRC32 checksum")]
    InvalidByronAddressChecksum,

    #[error("proposal deposit incorrect: supplied {supplied}, expected {expected}")]
    ProposalDepositIncorrect { supplied: u64, expected: u64 },

    #[error("proposal return account has wrong network id: account {account:?}, expected network {expected_network}")]
    ProposalProcedureNetworkIdMismatch {
        account: RewardAccount,
        expected_network: u8,
    },

    #[error("treasury withdrawal return account has wrong network id: account {account:?}, expected network {expected_network}")]
    TreasuryWithdrawalsNetworkIdMismatch {
        account: RewardAccount,
        expected_network: u8,
    },

    #[error("treasury withdrawals proposal has zero total withdrawals: {0:?}")]
    ZeroTreasuryWithdrawals(crate::eras::conway::GovAction),

    #[error("current treasury value incorrect: supplied {supplied}, actual {actual}")]
    CurrentTreasuryValueIncorrect { supplied: u64, actual: u64 },

    #[error("governance voters do not exist: {0:?}")]
    VotersDoNotExist(Vec<crate::eras::conway::Voter>),

    #[error("governance actions do not exist: {0:?}")]
    GovActionsDoNotExist(Vec<crate::eras::conway::GovActionId>),

    #[error("malformed governance action proposal: {0:?}")]
    MalformedProposal(crate::eras::conway::GovAction),

    #[error("governance proposal is not allowed during Conway bootstrap: {0:?}")]
    DisallowedProposalDuringBootstrap(crate::eras::conway::ProposalProcedure),

    #[error("governance votes are not allowed during Conway bootstrap: {0:?}")]
    DisallowedVotesDuringBootstrap(Vec<(crate::eras::conway::Voter, crate::eras::conway::GovActionId)>),

    #[error("governance voters are not allowed to vote on these actions: {0:?}")]
    DisallowedVoters(Vec<(crate::eras::conway::Voter, crate::eras::conway::GovActionId)>),

    #[error("committee update proposal adds and removes the same members: {0:?}")]
    ConflictingCommitteeUpdate(Vec<StakeCredential>),

    #[error("committee update proposal uses expiration epochs that are not after the current epoch: {0:?}")]
    ExpirationEpochTooSmall(Vec<(StakeCredential, EpochNo)>),

    #[error("proposal references an invalid previous governance action: {0:?}")]
    InvalidPrevGovActionId(crate::eras::conway::ProposalProcedure),

    #[error("governance voters are voting on expired actions: {0:?}")]
    VotingOnExpiredGovAction(Vec<(crate::eras::conway::Voter, crate::eras::conway::GovActionId)>),

    #[error(
        "hard-fork proposal does not follow the expected protocol-version progression: prev action {prev_action_id:?}, supplied {supplied:?}, expected predecessor {expected:?}"
    )]
    ProposalCantFollow {
        prev_action_id: Option<crate::eras::conway::GovActionId>,
        supplied: (u64, u64),
        expected: (u64, u64),
    },

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
        "era regression: ledger is in {ledger_era:?} (ordinal {ledger_ordinal}), \
         but received block in earlier era {block_era:?} (ordinal {block_ordinal})"
    )]
    /// Hard-fork combinator invariant violated: an incoming block is from an
    /// era that precedes the current ledger era.  Once the ledger advances
    /// past a hard-fork boundary it must never receive blocks from earlier
    /// eras.
    ///
    /// Reference: `Ouroboros.Consensus.HardFork.Combinator` — foreground check.
    BlockEraRegression {
        /// The current ledger era before this block was applied.
        ledger_era: super::eras::Era,
        /// Ordinal of the current ledger era.
        ledger_ordinal: u8,
        /// The era reported by the incoming block.
        block_era: super::eras::Era,
        /// Ordinal of the incoming block's era.
        block_ordinal: u8,
    },

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

    #[error("bootstrap witness signature verification failed for hash {hash:02x?}")]
    InvalidBootstrapWitnessSignature { hash: [u8; 28] },

    #[error("bootstrap witness attributes are not valid CBOR map bytes: {0:02x?}")]
    InvalidBootstrapWitnessAttributes(Vec<u8>),

    // -- Auxiliary data validation errors ------------------------------------

    #[error(
        "auxiliary data hash mismatch: declared {declared:02x?}, computed {computed:02x?}"
    )]
    AuxiliaryDataHashMismatch {
        declared: [u8; 32],
        computed: [u8; 32],
    },

    #[error("auxiliary data hash declared but no auxiliary data present in block")]
    AuxiliaryDataMissing,

    // -- Epoch boundary errors ----------------------------------------------

    #[error("protocol parameters are required but missing")]
    MissingProtocolParameters,
}
