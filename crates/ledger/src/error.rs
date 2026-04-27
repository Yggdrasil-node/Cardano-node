use thiserror::Error;

use crate::types::{AddrKeyHash, DRep, EpochNo, PoolKeyHash, RewardAccount, StakeCredential};

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

    #[error("CBOR: nesting depth exceeds maximum of {max}")]
    CborNestingTooDeep { max: usize },

    #[error("CBOR: decoded count {count} exceeds per-message bound {max}")]
    DecodedCountTooLarge { count: u64, max: usize },

    #[error("ledger arithmetic overflow at {site}")]
    ValueOverflow { site: &'static str },

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

    #[error("value not preserved: consumed {consumed} lovelace != produced {produced} + fee {fee}")]
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

    /// Upstream: `VRFKeyHashAlreadyRegistered` — in Conway, a pool's VRF key
    /// must not already be in use by another registered pool.
    /// Reference: `Cardano.Ledger.Shelley.Rules.Pool` —
    /// `hardforkConwayDisallowDuplicatedVRFKeys`.
    #[error("VRF key hash already registered by pool {existing_pool:02x?}")]
    VrfKeyAlreadyRegistered {
        pool: PoolKeyHash,
        vrf_key: [u8; 32],
        existing_pool: PoolKeyHash,
    },

    #[error("pool cost {cost} is below minPoolCost {min_pool_cost}")]
    PoolCostTooLow { cost: u64, min_pool_cost: u64 },

    #[error("pool margin numerator {numerator} exceeds denominator {denominator}")]
    PoolMarginInvalid { numerator: u64, denominator: u64 },

    #[error("pool reward account network {actual} does not match expected {expected}")]
    PoolRewardAccountNetworkMismatch { actual: u8, expected: u8 },

    #[error("pool metadata URL too long: {length} bytes (max 64)")]
    PoolMetadataUrlTooLong { length: usize },

    /// DEPRECATED: The upstream POOL rule (`Cardano.Ledger.Shelley.Rules.Pool`)
    /// intentionally does NOT enforce pool-owner registration. This variant is
    /// retained for CBOR backward compatibility with older checkpoints but is
    /// no longer produced at runtime.
    #[error("pool owner not registered as stake credential: {owner:02x?}")]
    PoolOwnerNotRegistered { owner: AddrKeyHash },

    /// CDDL `pool_owners = set<addr_keyhash>` — duplicate owner entries in
    /// a pool registration certificate.
    #[error("duplicate pool owner: {owner:02x?}")]
    DuplicatePoolOwner { owner: AddrKeyHash },

    #[error(
        "pool retirement epoch {retirement_epoch} exceeds maximum {max_epoch} (current {current_epoch} + eMax {e_max})"
    )]
    PoolRetirementTooFar {
        retirement_epoch: u64,
        current_epoch: u64,
        e_max: u64,
        max_epoch: u64,
    },

    /// Upstream: `StakePoolRetirementWrongEpochPOOL` — `cEpoch < e` not satisfied.
    #[error(
        "pool retirement epoch {retirement_epoch} must be strictly after current epoch {current_epoch}"
    )]
    PoolRetirementTooEarly {
        retirement_epoch: u64,
        current_epoch: u64,
    },

    /// Upstream: `GenesisKeyNotInMappingDELEG` — the genesis key hash in a
    /// `GenesisDelegation` certificate must exist in the current genesis
    /// delegates mapping.
    #[error("genesis key not in delegate mapping: {genesis_hash:02x?}")]
    GenesisKeyNotInMapping { genesis_hash: [u8; 28] },

    /// Upstream: `DuplicateGenesisDelegateDELEG` — the delegate key hash in
    /// a `GenesisDelegation` certificate must not already be delegated to by
    /// another genesis key.
    #[error("duplicate genesis delegate: {delegate_hash:02x?}")]
    DuplicateGenesisDelegate { delegate_hash: [u8; 28] },

    /// Upstream: `DuplicateGenesisVRFDELEG` — the VRF key hash in a
    /// `GenesisDelegation` certificate must not already be delegated to by
    /// another genesis key.
    #[error("duplicate genesis VRF key: {vrf_hash:02x?}")]
    DuplicateGenesisVrf { vrf_hash: [u8; 32] },

    #[error("stake credential already registered: {0:?}")]
    StakeCredentialAlreadyRegistered(StakeCredential),

    #[error("stake credential not registered: {0:?}")]
    StakeCredentialNotRegistered(StakeCredential),

    #[error("drep already registered: {0:?}")]
    DrepAlreadyRegistered(DRep),

    #[error("drep not registered: {0:?}")]
    DrepNotRegistered(DRep),

    /// Upstream: `DelegateeNotRegisteredDELEG` — delegation target DRep is
    /// not currently registered.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Deleg`.
    #[error("delegatee DRep not registered: {0:?}")]
    DelegateeDRepNotRegistered(DRep),

    /// Upstream: `UnelectedCommitteeVoters` — votes by committee hot
    /// credentials that are not authorized by any currently-elected,
    /// non-resigned committee member.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Gov`.
    #[error("unelected committee voters: {0:?}")]
    UnelectedCommitteeVoters(Vec<StakeCredential>),

    #[error("committee cold credential is unknown: {0:?}")]
    CommitteeIsUnknown(StakeCredential),

    #[error("committee cold credential has previously resigned: {0:?}")]
    CommitteeHasPreviouslyResigned(StakeCredential),

    #[error("stake credential has non-zero reward balance: {credential:?} has {balance} lovelace")]
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

    #[error(
        "proposal return account has wrong network id: account {account:?}, expected network {expected_network}"
    )]
    ProposalProcedureNetworkIdMismatch {
        account: RewardAccount,
        expected_network: u8,
    },

    #[error(
        "treasury withdrawal return account has wrong network id: account {account:?}, expected network {expected_network}"
    )]
    TreasuryWithdrawalsNetworkIdMismatch {
        account: RewardAccount,
        expected_network: u8,
    },

    #[error("treasury withdrawals proposal has zero total withdrawals: {0:?}")]
    ZeroTreasuryWithdrawals(crate::eras::conway::GovAction),

    #[error("current treasury value incorrect: supplied {supplied}, actual {actual}")]
    CurrentTreasuryValueIncorrect { supplied: u64, actual: u64 },

    /// Conway transaction declares a `treasury_donation` of zero.
    ///
    /// Upstream rejects this as `ZeroDonation` — the field must either be
    /// absent or carry a strictly positive amount.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Utxo` — `validateZeroDonation`.
    #[error("treasury donation must be non-zero when present")]
    ZeroDonation,

    #[error("governance voters do not exist: {0:?}")]
    VotersDoNotExist(Vec<crate::eras::conway::Voter>),

    #[error("governance actions do not exist: {0:?}")]
    GovActionsDoNotExist(Vec<crate::eras::conway::GovActionId>),

    #[error("malformed governance action proposal: {0:?}")]
    MalformedProposal(crate::eras::conway::GovAction),

    #[error("governance proposal is not allowed during Conway bootstrap: {0:?}")]
    DisallowedProposalDuringBootstrap(crate::eras::conway::ProposalProcedure),

    #[error("governance votes are not allowed during Conway bootstrap: {0:?}")]
    DisallowedVotesDuringBootstrap(
        Vec<(crate::eras::conway::Voter, crate::eras::conway::GovActionId)>,
    ),

    #[error("governance voters are not allowed to vote on these actions: {0:?}")]
    DisallowedVoters(Vec<(crate::eras::conway::Voter, crate::eras::conway::GovActionId)>),

    #[error("committee update proposal adds and removes the same members: {0:?}")]
    ConflictingCommitteeUpdate(Vec<StakeCredential>),

    #[error(
        "committee update proposal quorum is not a well-formed unit interval (numerator={numerator}, denominator={denominator})"
    )]
    WellFormedUnitIntervalRatification { numerator: u64, denominator: u64 },

    #[error(
        "committee update proposal uses expiration epochs that are not after the current epoch: {0:?}"
    )]
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
        "hard-fork proposal cannot be validated without a current protocol-version baseline: {0:?}"
    )]
    MissingProtocolVersionForHardFork(crate::eras::conway::ProposalProcedure),

    /// Upstream: `InvalidGuardrailsScriptHash` — the guardrails (policy)
    /// script hash carried by a `ParameterChange` or `TreasuryWithdrawals`
    /// proposal does not match the constitution's guardrails script hash.
    #[error(
        "invalid guardrails script hash: proposal has {proposal_hash:02x?}, constitution has {constitution_hash:02x?}"
    )]
    InvalidGuardrailsScriptHash {
        /// The guardrails script hash in the proposal (or `None`).
        proposal_hash: Option<[u8; 28]>,
        /// The guardrails script hash of the current constitution (or `None`).
        constitution_hash: Option<[u8; 28]>,
    },

    /// Upstream: `ProposalReturnAccountDoesNotExist` — the proposal's
    /// return (deposit refund) address is not a registered stake credential.
    #[error("proposal return account does not exist: {0:?}")]
    ProposalReturnAccountDoesNotExist(RewardAccount),

    /// Upstream: `TreasuryWithdrawalReturnAccountsDoNotExist` — one or more
    /// treasury withdrawal target accounts are not registered.
    #[error("treasury withdrawal return accounts do not exist: {0:?}")]
    TreasuryWithdrawalReturnAccountsDoNotExist(Vec<RewardAccount>),

    #[error(
        "withdrawal exceeds reward balance for {account:?}: requested {requested}, available {available}"
    )]
    WithdrawalExceedsBalance {
        account: RewardAccount,
        requested: u64,
        available: u64,
    },

    /// Upstream: `WithdrawalsNotInRewardsCERTS` — Conway requires each
    /// withdrawal to drain the full reward account balance (no partial
    /// withdrawals).  The `Withdrawals` map must match every account's
    /// balance exactly.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Certs` — `conwayTransition`.
    #[error(
        "withdrawal does not drain reward account {account:?}: requested {requested}, balance {balance}"
    )]
    WithdrawalNotFullDrain {
        account: RewardAccount,
        requested: u64,
        balance: u64,
    },

    /// Upstream: legacy `IncorrectDepositDELEG` — used for Conway key-deposit
    /// mismatches during bootstrap phase (PV < 10).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Deleg`.
    #[error("incorrect key deposit in certificate: supplied {supplied}, expected {expected}")]
    IncorrectDepositDELEG { supplied: u64, expected: u64 },

    /// Upstream: `DepositIncorrectDELEG` — used for Conway key-deposit
    /// mismatches during post-bootstrap phase (PV >= 10) via
    /// `hardforkConwayDELEGIncorrectDepositsAndRefunds`.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Deleg`.
    #[error("incorrect key deposit (post-bootstrap): supplied {supplied}, expected {expected}")]
    DepositIncorrectDELEG { supplied: u64, expected: u64 },

    /// Upstream: `IncorrectDepositDELEG` (refund variant) — the refund
    /// amount carried in a Conway `UnRegCert` does not match the credential's
    /// stored deposit.  Used when PV < 10 (bootstrap phase).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Deleg`.
    #[error(
        "incorrect key deposit refund in certificate: supplied {supplied}, expected {expected}"
    )]
    IncorrectKeyDepositRefund { supplied: u64, expected: u64 },

    /// Upstream: `RefundIncorrectDELEG` — the refund amount carried in a
    /// Conway `UnRegCert` does not match the credential's stored deposit.
    /// Used when PV >= 10 (post-bootstrap Conway) via
    /// `hardforkConwayDELEGIncorrectDepositsAndRefunds`.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Deleg`.
    #[error(
        "incorrect key deposit refund (post-bootstrap): supplied {supplied}, expected {expected}"
    )]
    RefundIncorrectDELEG { supplied: u64, expected: u64 },

    /// Upstream: `ConwayDRepIncorrectDeposit` — the deposit amount in a
    /// `ConwayRegDRep` certificate does not match `ppDRepDeposit`.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.GovCert`.
    #[error("DRep deposit incorrect: supplied {supplied}, expected {expected}")]
    DrepIncorrectDeposit { supplied: u64, expected: u64 },

    /// Upstream: `ConwayDRepIncorrectRefund` — the refund amount in a
    /// `ConwayUnRegDRep` certificate does not match the DRep's stored deposit.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.GovCert`.
    #[error("DRep refund incorrect: supplied {supplied}, expected {expected}")]
    DrepIncorrectRefund { supplied: u64, expected: u64 },

    #[error("unsupported certificate kind in this ledger slice: {0}")]
    UnsupportedCertificate(&'static str),

    #[error("update field (key 6) not allowed in Conway era")]
    UpdateNotAllowedConway,

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

    /// A transaction output contains a multi-asset entry with zero quantity.
    ///
    /// Zero-valued tokens are disallowed from Mary onward.
    /// Reference: `Cardano.Ledger.Mary.Value` — non-zero invariant.
    #[error("zero-valued multi-asset output: policy {policy_id:?} asset {asset_name:?}")]
    ZeroValuedMultiAssetOutput {
        policy_id: [u8; 28],
        asset_name: Vec<u8>,
    },

    // -- Network validation errors ------------------------------------------
    /// One or more transaction outputs carry an address whose network ID
    /// does not match the expected network.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetwork`.
    #[error("output address has wrong network: expected {expected}, found {found}")]
    WrongNetwork { expected: u8, found: u8 },

    /// One or more withdrawal reward accounts carry a network ID that does
    /// not match the expected network.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `WrongNetworkWithdrawal`.
    #[error("withdrawal address has wrong network: expected {expected}, found {found}")]
    WrongNetworkWithdrawal { expected: u8, found: u8 },

    /// The `network_id` field declared in the transaction body (Alonzo+)
    /// does not match the expected network.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `WrongNetworkInTxBody`.
    #[error("network_id in tx body has wrong network: expected {expected}, found {found}")]
    WrongNetworkInTxBody { expected: u8, found: u8 },

    // -- Script validation errors -------------------------------------------
    #[error("native script not satisfied: script hash {hash:02x?}")]
    NativeScriptFailed { hash: [u8; 28] },

    // -- Plutus script validation errors ------------------------------------
    #[error("Plutus script evaluation failed: script hash {hash:02x?}: {reason}")]
    PlutusScriptFailed { hash: [u8; 28], reason: String },

    #[error("Plutus script not found for script hash {hash:02x?}")]
    PlutusScriptNotFound { hash: [u8; 28] },

    #[error("required script witness not found for script hash {hash:02x?}")]
    MissingScriptWitness { hash: [u8; 28] },

    /// A script witness was provided but is not required by any input,
    /// certificate, withdrawal, mint, vote, or proposal in the transaction.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.extraneousScriptWitnessesUTXOW`.
    #[error("extraneous script witness: script hash {hash:02x?} not required by transaction")]
    ExtraneousScriptWitness { hash: [u8; 28] },

    /// A Plutus script in the transaction witness set could not be deserialized
    /// into valid UPLC.
    ///
    /// Reference: `Cardano.Ledger.Babbage.Rules.Utxow` — `MalformedScriptWitnesses`.
    #[error("malformed Plutus script witness(es): {0:02x?}")]
    MalformedScriptWitnesses(Vec<[u8; 28]>),

    /// A reference script in a transaction output could not be deserialized
    /// into valid UPLC.
    ///
    /// Reference: `Cardano.Ledger.Babbage.Rules.Utxow` — `MalformedReferenceScripts`.
    #[error("malformed reference script(s): {0:02x?}")]
    MalformedReferenceScripts(Vec<[u8; 28]>),

    /// The upper validity bound of a Plutus-bearing transaction exceeds the
    /// epoch-info forecast horizon (`current_slot + stability_window`).
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `OutsideForecast`.
    #[error("outside forecast: upper validity bound slot {slot} exceeds forecast limit {limit}")]
    OutsideForecast { slot: u64, limit: u64 },

    #[error("no matching redeemer for script hash {hash:02x?} (purpose {purpose})")]
    MissingRedeemer { hash: [u8; 28], purpose: String },

    /// A redeemer was provided for a purpose that is not backed by a Plutus
    /// script (e.g. a VKey-locked input or a native-script purpose).
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.hasExactSetOfRedeemers`.
    #[error("extra redeemer: tag {tag} index {index} does not target a Plutus script purpose")]
    ExtraRedeemer { tag: u8, index: u64 },

    /// A Plutus script uses a language version whose cost model is not present
    /// in the protocol parameters.  The transaction is rejected before any CEK
    /// evaluation takes place (Phase-1 rejection).
    ///
    /// `language` is the CDDL cost-model key: 0 = PlutusV1, 1 = PlutusV2,
    /// 2 = PlutusV3.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Plutus.Evaluate.collectPlutusScriptsWithContext`
    /// — `NoCostModel` variant of `CollectError`.
    #[error("no cost model for Plutus language {language}")]
    NoCostModel { language: u8 },

    /// A datum in the witness set is not required by any Plutus spending input
    /// and its hash does not appear on any transaction output (or reference-
    /// input UTxO in Babbage+).
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.validateRequiredDatums`
    /// (`NotAllowedSupplementalDatums`).
    #[error("witness datum with hash {hash:02x?} is not allowed as supplemental")]
    NotAllowedSupplementalDatums { hash: [u8; 32] },

    /// A Plutus-locked spending input's datum hash is not present in the
    /// transaction witness datum map — the script cannot be executed.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums`
    /// (`MissingRequiredDatums`).
    #[error("required datum hash {hash:02x?} not present in witness datums")]
    MissingRequiredDatums { hash: [u8; 32] },

    /// A Plutus-script-locked spending input (PlutusV1 or PlutusV2) lacks a datum
    /// or inline datum. The UTxO is unspendable without datum information.
    ///
    /// PlutusV3 scripts do NOT require a datum (CIP-0069).
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxow.missingRequiredDatums`
    /// and `Cardano.Ledger.Alonzo.UTxO.getInputDataHashesTxBody`.
    #[error(
        "spending input (tx {tx_id:02x?} index {index}) is locked by a PlutusV1/V2 script but has no datum or datum hash"
    )]
    UnspendableUTxONoDatumHash { tx_id: [u8; 32], index: u64 },

    /// An Alonzo-era transaction output is sent to a Plutus script address
    /// but does not include a `datum_hash`.  The output would be permanently
    /// unspendable.
    ///
    /// Babbage+ relaxes this via inline datums.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` —
    ///   `validateOutputMissingDatumHashForScriptOutputs`.
    #[error("Alonzo output to script address has no datum hash: address {address:02x?}")]
    MissingDatumHashOnScriptOutput { address: Vec<u8> },

    #[error("datum not found for spending input (tx {tx_id:02x?} index {index})")]
    MissingDatum { tx_id: [u8; 32], index: u64 },

    #[error("Plutus script decode failed for script hash {hash:02x?}: {reason}")]
    PlutusScriptDecodeError { hash: [u8; 28], reason: String },

    #[error("script integrity hash mismatch: declared {declared:02x?}, computed {computed:02x?}")]
    PPViewHashesDontMatch {
        declared: [u8; 32],
        computed: [u8; 32],
    },

    /// Post-PV11 variant of `PPViewHashesDontMatch`.
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Utxo` — at protocol version >= 11
    /// the error is reported as `ScriptIntegrityHashMismatch` instead of
    /// `PPViewHashesDontMatch`.
    #[error(
        "script integrity hash mismatch (PV>=11): declared {declared:02x?}, computed {computed:02x?}"
    )]
    ScriptIntegrityHashMismatch {
        declared: [u8; 32],
        computed: [u8; 32],
    },

    /// Upstream `PPViewHashesDontMatch` direction: transaction includes Plutus
    /// redeemers but does not declare a `script_data_hash`.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` —
    /// `ppViewHashesDontMatch` / `validateScriptsNeedIntegrity`.
    #[error("script integrity hash required but absent (redeemers present)")]
    MissingRequiredScriptIntegrityHash,

    /// Upstream `PPViewHashesDontMatch` direction: transaction declares a
    /// `script_data_hash` but has no Plutus redeemers or scripts.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Utxo`.
    #[error("unexpected script integrity hash declared (no redeemers)")]
    UnexpectedScriptIntegrityHash { declared: [u8; 32] },

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

    /// Serialized block header exceeds `max_block_header_size` protocol
    /// parameter.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Bbody` — `HeaderLeqBBodySize`.
    #[error("block header too large: {actual} bytes exceeds max {max}")]
    HeaderTooLarge { actual: usize, max: usize },

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

    /// A transaction containing a MIR certificate (DCert tag 6) does not have
    /// enough genesis delegate key signatures.  The quorum requires
    /// `genesis_update_quorum` genesis delegate witnesses but only
    /// `present` were found.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` — `MIRInsufficientGenesisSigsUTXOW`.
    #[error(
        "MIR certificate requires {required} genesis delegate signatures but only {present} were provided"
    )]
    MIRInsufficientGenesisSigs { required: usize, present: usize },

    /// MIR certificate submitted too late in the epoch — must arrive before
    /// `firstSlot(nextEpoch) - stabilityWindow`.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `MIRCertificateTooLateinEpochDELEG`.
    #[error("MIR certificate too late in epoch: slot {slot} >= deadline {deadline}")]
    MIRCertificateTooLateInEpoch { slot: u64, deadline: u64 },

    /// Pre-Alonzo: negative `DeltaCoin` values in MIR `StakeAddressesMIR`
    /// are not allowed before the Alonzo hard fork.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `MIRNegativesNotCurrentlyAllowed`.
    #[error("MIR negative deltas not allowed pre-Alonzo")]
    MIRNegativesNotCurrentlyAllowed,

    /// Alonzo+: after merging with existing IR map, a credential has a
    /// combined negative value.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `MIRProducesNegativeUpdate`.
    #[error("MIR produces negative update for credential")]
    MIRProducesNegativeUpdate,

    /// The total MIR rewards from a given pot exceed the pot balance
    /// (adjusted for existing MIR commitments and pot-to-pot deltas in
    /// Alonzo+).
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `InsufficientForInstantaneousRewardsDELEG`.
    #[error("MIR insufficient pot balance: {pot:?} has {available} but {required} needed")]
    MIRInsufficientPotBalance {
        pot: crate::MirPot,
        available: u64,
        required: u64,
    },

    /// Pre-Alonzo: `SendToOppositePot` MIR transfers are not allowed.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `MIRTransferNotCurrentlyAllowed`.
    #[error("MIR transfer (SendToOppositePot) not allowed pre-Alonzo")]
    MIRTransferNotCurrentlyAllowed,

    /// MIR transfer amount is negative.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `MIRNegativeTransfer`.
    #[error("MIR negative transfer for pot {pot:?}: {amount}")]
    MIRNegativeTransfer { pot: crate::MirPot, amount: i64 },

    /// MIR transfer amount exceeds the pot balance after accounting for
    /// existing MIR commitments.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — `InsufficientForTransferDELEG`.
    #[error("MIR insufficient for transfer: {pot:?} has {available} but {required} needed")]
    MIRInsufficientForTransfer {
        pot: crate::MirPot,
        available: u64,
        required: u64,
    },

    // -- Auxiliary data validation errors ------------------------------------
    #[error("auxiliary data hash mismatch: declared {declared:02x?}, computed {computed:02x?}")]
    AuxiliaryDataHashMismatch {
        declared: [u8; 32],
        computed: [u8; 32],
    },

    #[error("auxiliary data hash declared but no auxiliary data present in block")]
    AuxiliaryDataMissing,

    /// Auxiliary data is present in the transaction but the tx body does not
    /// declare the corresponding hash.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` —
    /// `validateMissingTxBodyMetadataHash` / `MissingTxBodyMetadataHash`.
    #[error("auxiliary data present but tx body missing auxiliary_data_hash")]
    MissingTxBodyMetadataHash,

    /// Transaction auxiliary data contains out-of-range metadatum values
    /// (byte strings or text strings exceeding 64 bytes).
    ///
    /// Active when protocol version > (2, 0) — i.e. from Allegra onwards.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxow` —
    /// `validateMetadata` / `InvalidMetadata`;
    /// `Cardano.Ledger.Metadata` — `validMetadatum`.
    #[error("auxiliary data contains out-of-range metadatum values (bytes/text > 64)")]
    InvalidMetadata,

    /// Transaction spending inputs contain duplicates.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `BadInputsUTxO`
    /// (the upstream check subsumes duplicate detection via set conversion).
    #[error("duplicate spending input in transaction")]
    DuplicateInput,

    /// Submitted transaction has `is_valid = false`.
    ///
    /// Only a block producer may include transactions with `is_valid = false`
    /// (after observing Phase-2 script failure during block forging).
    /// Submitted transactions must always have `is_valid = true`.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Tx` — `IsValid`.
    #[error("submitted transaction has is_valid = false (Phase-2 script failure)")]
    SubmittedTxIsInvalid,

    /// The block producer's `is_valid` claim does not match the node's own
    /// Phase-2 script re-evaluation.
    ///
    /// Reference: `Cardano.Ledger.Alonzo.Rules.Bbody` — `ValidationTagMismatch`.
    #[error("Phase-2 validation tag mismatch: block says {claimed}, re-evaluation says {actual}")]
    ValidationTagMismatch {
        /// The `is_valid` flag from the block producer.
        claimed: bool,
        /// The result of local Phase-2 re-evaluation.
        actual: bool,
    },

    /// Total reference script size across all referenced UTxO entries exceeds
    /// the maximum allowed per transaction (Conway+ rule).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Ledger` — `ConwayTxRefScriptsSizeTooBig`.
    #[error("total reference script size {actual} exceeds maximum {max_allowed} bytes")]
    TxRefScriptsSizeTooBig { actual: usize, max_allowed: usize },

    /// Total reference script size across all transactions in a block exceeds
    /// the block-level maximum (Conway BBODY rule).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Bbody` — `BodyRefScriptsSizeTooBig`.
    #[error("block total reference script size {actual} exceeds block maximum {max_allowed} bytes")]
    BodyRefScriptsSizeTooBig { actual: usize, max_allowed: usize },

    /// A withdrawal from a key-hash reward account was attempted but the
    /// account does not have a DRep delegation (Conway post-bootstrap rule).
    ///
    /// Reference: `Cardano.Ledger.Conway.Rules.Ledger` —
    /// `ConwayWdrlNotDelegatedToDRep`.
    #[error("withdrawal credential {credential:02x?} is not delegated to a DRep")]
    WithdrawalNotDelegatedToDRep { credential: [u8; 28] },

    // -- Epoch boundary errors ----------------------------------------------
    #[error("protocol parameters are required but missing")]
    MissingProtocolParameters,

    // -- PPUP (protocol parameter update proposal) validation errors --------
    /// A protocol parameter update was proposed by a key hash that is not a
    /// recognized genesis delegate.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Ppup` — `NonGenesisUpdatePPUP`.
    #[error("PPUP proposer {proposer:02x?} is not a genesis delegate")]
    NonGenesisUpdatePPUP { proposer: [u8; 28] },

    /// A protocol parameter update targets the wrong epoch.
    ///
    /// Before the stability window boundary (`tooLate` slot), the target must
    /// equal the current epoch; after it, the target must equal `current + 1`.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Ppup` — `PPUpdateWrongEpoch`.
    #[error(
        "PPUP wrong epoch: current {current_epoch}, target {target_epoch}, \
         expected {expected_epoch} ({voting_period})"
    )]
    PPUpdateWrongEpoch {
        current_epoch: u64,
        target_epoch: u64,
        expected_epoch: u64,
        voting_period: &'static str,
    },

    /// A protocol parameter update proposes a protocol version that does not
    /// follow the current protocol version according to `pvCanFollow` rules:
    /// either increment major by 1 (setting minor to 0), or keep major and
    /// increment minor by 1.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Ppup` — `PVCannotFollowPPUP`.
    #[error(
        "PPUP proposed protocol version ({proposed_major}.{proposed_minor}) cannot follow \
         current ({current_major}.{current_minor})"
    )]
    PVCannotFollowPPUP {
        current_major: u64,
        current_minor: u64,
        proposed_major: u64,
        proposed_minor: u64,
    },

    /// The transaction's `mint` field contains the ADA policy ID (`[0u8; 28]`).
    ///
    /// The formal ledger spec requires `adaPolicy ∉ supp mint tx`.
    /// In the Haskell implementation this is enforced by construction
    /// (the `MultiAsset` type cannot represent ADA), but our Rust
    /// representation uses a plain `BTreeMap<[u8; 28], …>` which can hold
    /// the all-zeros policy ID, so we enforce it at validation time.
    ///
    /// Reference: `Cardano.Ledger.Mary.Rules.Utxo` — formal spec predicate
    /// `adaPolicy ∉ supp mint tx` (Mary through Conway).
    #[error("transaction attempts to mint or burn ADA (policy ID is the zero hash)")]
    TriesToForgeADA,

    /// An asset name exceeds the CDDL maximum of 32 bytes.
    ///
    /// CDDL: `asset_name = bytes .size (0..32)`
    ///
    /// Reference: `Cardano.Ledger.Mary.Value` — asset name size constraint.
    #[error("asset name too long ({actual} bytes, max 32)")]
    AssetNameTooLong { actual: usize },
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
        let e = LedgerError::CborTypeMismatch {
            expected: 0,
            actual: 2,
        };
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
        let e = LedgerError::CborInvalidLength {
            expected: 3,
            actual: 5,
        };
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
        let e = LedgerError::TxExpired {
            ttl: 100,
            slot: 200,
        };
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
        assert_eq!(
            LedgerError::NoInputs.to_string(),
            "no inputs in transaction"
        );
    }

    #[test]
    fn no_outputs_display() {
        assert_eq!(
            LedgerError::NoOutputs.to_string(),
            "no outputs in transaction"
        );
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
        let e = LedgerError::FeeTooSmall {
            minimum: 200_000,
            declared: 100_000,
        };
        let s = e.to_string();
        assert!(s.contains("200000"));
        assert!(s.contains("100000"));
    }

    #[test]
    fn output_too_small_display() {
        let e = LedgerError::OutputTooSmall {
            minimum: 1_000_000,
            actual: 500_000,
        };
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
        let e = LedgerError::ProposalDepositIncorrect {
            supplied: 500,
            expected: 1000,
        };
        let s = e.to_string();
        assert!(s.contains("supplied 500"));
        assert!(s.contains("expected 1000"));
    }

    #[test]
    fn block_too_large_display() {
        let e = LedgerError::BlockTooLarge {
            actual: 100_000,
            max: 65_536,
        };
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
        let e = LedgerError::PoolCostTooLow {
            cost: 100,
            min_pool_cost: 340_000_000,
        };
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
