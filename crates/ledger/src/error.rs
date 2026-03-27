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

    #[error("CBOR: {0}")]
    CborDecodeError(String),

    // -- UTxO validation errors ---------------------------------------------

    #[error("block slot {block_slot} does not advance past current tip slot {tip_slot}")]
    SlotNotIncreasing { tip_slot: u64, block_slot: u64 },

    #[error("transaction expired: TTL {ttl} < current slot {slot}")]
    TxExpired { ttl: u64, slot: u64 },

    #[error("input not found in UTxO set")]
    InputNotInUtxo,

    #[error("reference input not found in UTxO set")]
    ReferenceInputNotInUtxo,

    #[error("spending input also appears in reference inputs (Babbage+ disjointness rule)")]
    ReferenceInputContention,

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
        e_max: u64,
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

    #[error(
        "committee update proposal uses expiration epochs beyond the committee term limit (max {max_epoch:?}): {members:?}"
    )]
    ExpirationEpochTooLarge {
        members: Vec<(StakeCredential, EpochNo)>,
        max_epoch: EpochNo,
    },

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

    #[error("hard-fork proposal cannot be validated without a current protocol-version baseline: {0:?}")]
    MissingProtocolVersionForHardFork(crate::eras::conway::ProposalProcedure),

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

    #[error("unsupported plutus purpose in this context: {0}")]
    UnsupportedPlutusPurpose(&'static str),

    #[error("unsupported plutus context feature in this context: {0}")]
    UnsupportedPlutusContext(&'static str),

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

    /// Serialized value in an output exceeds `max_val_size` protocol parameter.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `OutputTooBigUTxO`.
    #[error("output value too big: serialized size {actual} exceeds max {max}")]
    OutputTooBig { actual: usize, max: usize },

    /// A Byron bootstrap address in a Shelley+ transaction output has
    /// attributes larger than the 64-byte limit.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `OutputBootAddrAttrsTooBig`.
    #[error("Byron bootstrap address attributes too big: {size} bytes (max 64)")]
    OutputBootAddrAttrsTooBig { size: usize },

    // -- Network validation errors ------------------------------------------

    /// One or more transaction outputs carry an address whose network ID
    /// does not match the expected network.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetwork`.
    #[error(
        "output address has wrong network: expected {expected}, found {found}"
    )]
    WrongNetwork { expected: u8, found: u8 },

    /// One or more withdrawal reward accounts carry a network ID that does
    /// not match the expected network.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetworkWithdrawal`.
    #[error(
        "withdrawal address has wrong network: expected {expected}, found {found}"
    )]
    WrongNetworkWithdrawal { expected: u8, found: u8 },

    /// The `network_id` field declared in the transaction body (Alonzo+)
    /// does not match the expected network.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `WrongNetworkInTxBody`.
    #[error(
        "network_id in tx body has wrong network: expected {expected}, found {found}"
    )]
    WrongNetworkInTxBody { expected: u8, found: u8 },

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

    #[error(
        "total collateral mismatch: declared {declared} lovelace, \
         but computed balance is {computed}"
    )]
    IncorrectTotalCollateralField { declared: u64, computed: u64 },

    #[error(
        "collateral balance is negative: inputs provide {input_coin} lovelace, \
         but return output requires {return_coin}"
    )]
    CollateralBalanceNegative { input_coin: u64, return_coin: u64 },

    #[error("collateral input is not VKey-locked (script address used as collateral)")]
    CollateralNotVKeyLocked,

    #[error("transaction contains phase-2 scripts but has no collateral inputs")]
    MissingCollateralForScripts,

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

    /// Transaction spending inputs contain duplicates.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `BadInputsUTxO`
    /// (the upstream check subsumes duplicate detection via set conversion).
    #[error("duplicate spending input in transaction")]
    DuplicateInput,

    /// Total reference script size across all referenced UTxO entries exceeds
    /// the maximum allowed per transaction (Conway+ rule).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Ledger` — `ConwayTxRefScriptsSizeTooBig`.
    #[error(
        "total reference script size {actual} exceeds maximum {max_allowed} bytes"
    )]
    TxRefScriptsSizeTooBig {
        actual: usize,
        max_allowed: usize,
    },

    // -- Epoch boundary errors ----------------------------------------------

    #[error("protocol parameters are required but missing")]
    MissingProtocolParameters,
}

// ─────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::eras::Era;

    #[test]
    fn unsupported_era_display() {
        let e = LedgerError::UnsupportedEra(Era::Byron);
        assert!(e.to_string().contains("unsupported era"));
    }

    #[test]
    fn cbor_unexpected_eof_display() {
        let e = LedgerError::CborUnexpectedEof;
        assert_eq!(e.to_string(), "CBOR: unexpected end of input");
    }

    #[test]
    fn cbor_type_mismatch_display() {
        let e = LedgerError::CborTypeMismatch { expected: 0, actual: 2 };
        let s = e.to_string();
        assert!(s.contains("expected major 0"));
        assert!(s.contains("got 2"));
    }

    #[test]
    fn cbor_invalid_additional_info_display() {
        let e = LedgerError::CborInvalidAdditionalInfo(31);
        assert!(e.to_string().contains("31"));
    }

    #[test]
    fn cbor_invalid_length_display() {
        let e = LedgerError::CborInvalidLength { expected: 3, actual: 5 };
        let s = e.to_string();
        assert!(s.contains("expected 3"));
        assert!(s.contains("got 5"));
    }

    #[test]
    fn cbor_trailing_bytes_display() {
        let e = LedgerError::CborTrailingBytes(10);
        assert!(e.to_string().contains("10 trailing bytes"));
    }

    #[test]
    fn cbor_decode_error_display() {
        let e = LedgerError::CborDecodeError("custom error".into());
        assert!(e.to_string().contains("custom error"));
    }

    #[test]
    fn tx_expired_display() {
        let e = LedgerError::TxExpired { ttl: 100, slot: 200 };
        let s = e.to_string();
        assert!(s.contains("TTL 100"));
        assert!(s.contains("slot 200"));
    }

    #[test]
    fn input_not_in_utxo_display() {
        let e = LedgerError::InputNotInUtxo;
        assert_eq!(e.to_string(), "input not found in UTxO set");
    }

    #[test]
    fn value_not_preserved_display() {
        let e = LedgerError::ValueNotPreserved {
            consumed: 100,
            produced: 80,
            fee: 10,
        };
        let s = e.to_string();
        assert!(s.contains("100"));
        assert!(s.contains("80"));
        assert!(s.contains("10"));
    }

    #[test]
    fn no_inputs_display() {
        assert_eq!(LedgerError::NoInputs.to_string(), "no inputs in transaction");
    }

    #[test]
    fn no_outputs_display() {
        assert_eq!(LedgerError::NoOutputs.to_string(), "no outputs in transaction");
    }

    #[test]
    fn pool_not_registered_display() {
        let e = LedgerError::PoolNotRegistered([0xaa; 28]);
        assert!(e.to_string().contains("stake pool not registered"));
    }

    #[test]
    fn stake_credential_already_registered_display() {
        let cred = StakeCredential::AddrKeyHash([0x01; 28]);
        let e = LedgerError::StakeCredentialAlreadyRegistered(cred);
        assert!(e.to_string().contains("already registered"));
    }

    #[test]
    fn stake_credential_not_registered_display() {
        let cred = StakeCredential::ScriptHash([0x02; 28]);
        let e = LedgerError::StakeCredentialNotRegistered(cred);
        assert!(e.to_string().contains("not registered"));
    }

    #[test]
    fn drep_already_registered_display() {
        let e = LedgerError::DrepAlreadyRegistered(DRep::AlwaysAbstain);
        assert!(e.to_string().contains("drep already registered"));
    }

    #[test]
    fn drep_not_registered_display() {
        let e = LedgerError::DrepNotRegistered(DRep::AlwaysNoConfidence);
        assert!(e.to_string().contains("drep not registered"));
    }

    #[test]
    fn fee_too_small_display() {
        let e = LedgerError::FeeTooSmall { minimum: 200_000, declared: 100_000 };
        let s = e.to_string();
        assert!(s.contains("200000"));
        assert!(s.contains("100000"));
    }

    #[test]
    fn output_too_small_display() {
        let e = LedgerError::OutputTooSmall { minimum: 1_000_000, actual: 500_000 };
        assert!(e.to_string().contains("1000000"));
    }

    #[test]
    fn missing_protocol_parameters_display() {
        let e = LedgerError::MissingProtocolParameters;
        assert!(e.to_string().contains("protocol parameters"));
    }

    #[test]
    fn error_equality() {
        let e1 = LedgerError::CborUnexpectedEof;
        let e2 = LedgerError::CborUnexpectedEof;
        assert_eq!(e1, e2);

        let e3 = LedgerError::InputNotInUtxo;
        assert_ne!(e1, e3);
    }

    #[test]
    fn proposal_deposit_incorrect_display() {
        let e = LedgerError::ProposalDepositIncorrect { supplied: 500, expected: 1000 };
        let s = e.to_string();
        assert!(s.contains("supplied 500"));
        assert!(s.contains("expected 1000"));
    }

    #[test]
    fn block_too_large_display() {
        let e = LedgerError::BlockTooLarge { actual: 100_000, max: 65_536 };
        assert!(e.to_string().contains("100000"));
    }

    #[test]
    fn no_collateral_inputs_display() {
        let e = LedgerError::NoCollateralInputs;
        assert!(e.to_string().contains("no collateral inputs"));
    }

    #[test]
    fn collateral_not_vkey_locked_display() {
        let e = LedgerError::CollateralNotVKeyLocked;
        assert!(e.to_string().contains("not VKey-locked"));
    }

    #[test]
    fn missing_collateral_for_scripts_display() {
        let e = LedgerError::MissingCollateralForScripts;
        assert!(e.to_string().contains("phase-2 scripts"));
    }

    #[test]
    fn auxiliary_data_hash_mismatch_display() {
        let e = LedgerError::AuxiliaryDataHashMismatch {
            declared: [0x01; 32],
            computed: [0x02; 32],
        };
        assert!(e.to_string().contains("auxiliary data hash mismatch"));
    }

    #[test]
    fn pool_cost_too_low_display() {
        let e = LedgerError::PoolCostTooLow { cost: 100, min_pool_cost: 340_000_000 };
        let s = e.to_string();
        assert!(s.contains("100"));
        assert!(s.contains("340000000"));
    }

    #[test]
    fn block_era_regression_display() {
        let e = LedgerError::BlockEraRegression {
            ledger_era: Era::Conway,
            ledger_ordinal: 6,
            block_era: Era::Shelley,
            block_ordinal: 1,
        };
        assert!(e.to_string().contains("era regression"));
    }

    #[test]
    fn missing_vkey_witness_display() {
        let e = LedgerError::MissingVKeyWitness { hash: [0xab; 28] };
        assert!(e.to_string().contains("missing required VKey witness"));
    }

    #[test]
    fn withdrawal_exceeds_balance_display() {
        let ra = RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0x01; 28]),
        };
        let e = LedgerError::WithdrawalExceedsBalance {
            account: ra,
            requested: 100,
            available: 50,
        };
        let s = e.to_string();
        assert!(s.contains("withdrawal"));
        assert!(s.contains("100"));
        assert!(s.contains("50"));
    }

    #[test]
    fn multi_asset_not_preserved_display() {
        let e = LedgerError::MultiAssetNotPreserved {
            policy: [0x01; 28],
            asset_name: vec![0x02],
            expected: 100,
            produced: 90,
        };
        assert!(e.to_string().contains("multi-asset not preserved"));
    }

    #[test]
    fn error_is_debug() {
        let e = LedgerError::CborUnexpectedEof;
        let dbg = format!("{e:?}");
        assert!(dbg.contains("CborUnexpectedEof"));
    }
}
