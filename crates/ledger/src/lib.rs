#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::field_reassign_with_default))]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
//! Ledger-facing state, transaction, and era abstractions.
//!
//! This crate provides typed protocol-level identifiers (`SlotNo`, `BlockNo`,
//! `HeaderHash`, `TxId`, `Point`) alongside era modeling, block/transaction
//! structures, and ledger state tracking.

/// Minimal hand-rolled CBOR encoder/decoder for protocol-level types.
pub mod cbor;
/// Collateral validation for Alonzo+ script transactions.
pub mod collateral;
/// Epoch boundary processing (NEWEPOCH / SNAP / RUPD).
pub mod epoch_boundary;
/// Era modeling and era-local modules.
pub mod eras;
mod error;
/// Fee calculation and validation.
pub mod fees;
/// Minimum UTxO output validation.
pub mod min_utxo;
/// Native script evaluation engine.
pub mod native_script;
/// PlutusData AST and Script types.
pub mod plutus;
/// Plutus Phase-2 script validation bridge (evaluator trait + resolution).
pub mod plutus_validation;
/// Protocol parameters governing transaction and block validation.
pub mod protocol_params;
/// Epoch reward calculation implementing the Shelley reward formula.
pub mod rewards;
/// Stake distribution snapshots and epoch-boundary snapshot rotation.
pub mod stake;
/// Ledger state containers and transition entry points.
pub mod state;
/// Transaction and block wrappers.
pub mod tx;
/// Core protocol-level types shared across ledger, storage, and consensus.
pub mod types;
/// Multi-era UTxO set.
pub mod utxo;
/// Witness sufficiency checks.
pub mod witnesses;

// -- CBOR re-exports ----------------------------------------------------------
/// CBOR encoding and decoding traits and primitives.
pub use cbor::{
    BLOCK_BODY_ELEMENTS_MAX, CborDecode, CborEncode, Decoder, Encoder, check_decoded_count,
    hashmap_with_safe_capacity, vec_with_safe_capacity, vec_with_strict_capacity,
};

// -- Era re-exports -----------------------------------------------------------
/// Supported Cardano eras represented in the workspace.
pub use eras::Era;
pub use eras::{
    AllegraTxBody, AlonzoBlock, AlonzoTxBody, AlonzoTxOut, AssetName, BYRON_SLOTS_PER_EPOCH,
    BabbageBlock, BabbageTxBody, BabbageTxOut, BootstrapWitness, ByronBlock, ByronTx, ByronTxAux,
    ByronTxIn, ByronTxOut, ByronTxWitness, Constitution, ConwayBlock, ConwayTxBody, DatumOption,
    ExUnits, GovAction, GovActionId, MaryTxBody, MaryTxOut, MintAsset, MultiAsset, NativeScript,
    PolicyId, PraosHeader, PraosHeaderBody, ProposalProcedure, Redeemer, ShelleyBlock,
    ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTx, ShelleyTxBody, ShelleyTxIn,
    ShelleyTxOut, ShelleyUpdate, ShelleyUtxo, ShelleyVkeyWitness, ShelleyVrfCert,
    ShelleyWitnessSet, Value, Vote, Voter, VotingProcedure, VotingProcedures,
    compute_block_body_hash,
};

// -- Error re-exports ---------------------------------------------------------
/// Errors surfaced by ledger-facing helpers.
pub use error::LedgerError;

// -- State re-exports ---------------------------------------------------------
/// Top-level ledger state wrapper.
pub use state::{
    AccountingState, CommitteeAuthorization, CommitteeMemberState, CommitteeState, DepositPot,
    DrepState, EnactOutcome, EnactState, GenesisDelegationState, GovernanceActionState,
    InstantaneousRewards, LedgerState, LedgerStateCheckpoint, LedgerStateSnapshot,
    PoolRelayAccessPoint, PoolState, PpupSlotContext, RegisteredDrep, RegisteredPool,
    RewardAccountState, RewardAccounts, StakeCredentialState, StakeCredentials,
    accumulate_mir_from_certs, pv_can_follow,
};

// -- Tx/Block re-exports ------------------------------------------------------
/// Transaction and block wrapper types.
pub use tx::{
    AlonzoCompatibleSubmittedTx, Block, BlockHeader, MultiEraSubmittedTx,
    ShelleyCompatibleSubmittedTx, Tx, compute_tx_id,
};

// -- Type re-exports ----------------------------------------------------------
pub use types::{
    AddrKeyHash, Address, Anchor, BaseAddress, BlockNo, DCert, DRep, EnterpriseAddress, EpochNo,
    GenesisDelegateHash, GenesisHash, HeaderHash, MirPot, MirTarget, Nonce, Point, PointerAddress,
    PoolKeyHash, PoolMetadata, PoolParams, Relay, RewardAccount, ScriptHash, SlotNo,
    StakeCredential, Tip, TxId, UnitInterval, VrfKeyHash,
};

// -- Plutus re-exports --------------------------------------------------------
pub use plutus::{PlutusData, Script, ScriptRef};

// -- UTxO re-exports ----------------------------------------------------------
pub use utxo::{MAX_REF_SCRIPT_SIZE_PER_TX, MultiEraTxOut, MultiEraUtxo};

// -- Stake distribution re-exports --------------------------------------------
pub use stake::{
    Delegations, IndividualStake, PoolStakeDistribution, StakeSnapshot, StakeSnapshots,
    augment_pool_dist_with_proposal_deposits, compute_drep_stake_distribution,
    compute_proposal_deposits_per_credential, compute_stake_snapshot,
};

// -- Reward re-exports --------------------------------------------------------
pub use rewards::{
    EpochRewardDistribution, EpochRewardPot, PoolRewardBreakdown, RewardParams,
    compute_epoch_reward_pot, compute_epoch_rewards, compute_pool_reward, max_pool_reward,
};

// -- Epoch boundary re-exports ------------------------------------------------
pub use epoch_boundary::{EpochBoundaryEvent, apply_epoch_boundary, retire_pools_with_refunds};

// -- Protocol params re-exports -----------------------------------------------
pub use protocol_params::{
    DRepVotingThresholds, PoolVotingThresholds, ProtocolParameterUpdate, ProtocolParameters,
};

// -- Fee re-exports -----------------------------------------------------------
pub use fees::{
    min_fee_linear, script_fee, total_min_fee, validate_fee, validate_tx_ex_units, validate_tx_size,
};

// -- Min-UTxO re-exports ------------------------------------------------------
pub use min_utxo::{
    validate_all_outputs_min_utxo, validate_min_utxo, validate_output_boot_addr_attrs,
    validate_output_not_too_big,
};

// -- Native script re-exports -------------------------------------------------
pub use native_script::{NativeScriptContext, evaluate_native_script, native_script_hash};

// -- Collateral re-exports ----------------------------------------------------
pub use collateral::validate_collateral;

// -- Witness re-exports -------------------------------------------------------
pub use witnesses::{validate_vkey_witnesses, vkey_hash, witness_vkey_hash_set};
