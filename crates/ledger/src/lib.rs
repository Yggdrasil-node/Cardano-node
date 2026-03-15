//! Ledger-facing state, transaction, and era abstractions.
//!
//! This crate provides typed protocol-level identifiers (`SlotNo`, `BlockNo`,
//! `HeaderHash`, `TxId`, `Point`) alongside era modeling, block/transaction
//! structures, and ledger state tracking.

/// Minimal hand-rolled CBOR encoder/decoder for protocol-level types.
pub mod cbor;
/// Era modeling and era-local modules.
pub mod eras;
mod error;
/// PlutusData AST and Script types.
pub mod plutus;
/// Ledger state containers and transition entry points.
pub mod state;
/// Transaction and block wrappers.
pub mod tx;
/// Core protocol-level types shared across ledger, storage, and consensus.
pub mod types;
/// Multi-era UTxO set.
pub mod utxo;

// -- CBOR re-exports ----------------------------------------------------------
/// CBOR encoding and decoding traits and primitives.
pub use cbor::{CborDecode, CborEncode, Decoder, Encoder};

// -- Era re-exports -----------------------------------------------------------
/// Supported Cardano eras represented in the workspace.
pub use eras::Era;
pub use eras::{
    AllegraTxBody, AlonzoBlock, AlonzoTxBody, AlonzoTxOut, AssetName, BabbageBlock, BabbageTxBody, BabbageTxOut,
    BootstrapWitness, ByronBlock, Constitution, ConwayBlock, ConwayTxBody, DatumOption, ExUnits,
    GovAction, GovActionId, MaryTxBody, MaryTxOut, MintAsset, MultiAsset, NativeScript, PolicyId,
    PraosHeader, PraosHeaderBody, ProposalProcedure, Redeemer, ShelleyBlock, ShelleyHeader,
    ShelleyHeaderBody, ShelleyOpCert, ShelleyTx, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut,
    ShelleyUpdate, ShelleyUtxo, ShelleyVkeyWitness, ShelleyVrfCert, ShelleyWitnessSet, Value, Vote,
    Voter, VotingProcedure, VotingProcedures, BYRON_SLOTS_PER_EPOCH, compute_block_body_hash,
};

// -- Error re-exports ---------------------------------------------------------
/// Errors surfaced by ledger-facing helpers.
pub use error::LedgerError;

// -- State re-exports ---------------------------------------------------------
/// Top-level ledger state wrapper.
pub use state::{
    CommitteeAuthorization, CommitteeMemberState, CommitteeState, DrepState, LedgerState,
    LedgerStateCheckpoint, LedgerStateSnapshot, PoolRelayAccessPoint, PoolState,
    RegisteredDrep, RegisteredPool, RewardAccountState,
    RewardAccounts, StakeCredentialState, StakeCredentials,
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
    GenesisDelegateHash, GenesisHash, HeaderHash, Nonce, Point, PointerAddress, PoolKeyHash,
    PoolMetadata, PoolParams, Relay, RewardAccount, ScriptHash, SlotNo, StakeCredential, TxId,
    UnitInterval, VrfKeyHash,
};

// -- Plutus re-exports --------------------------------------------------------
pub use plutus::{PlutusData, Script, ScriptRef};

// -- UTxO re-exports ----------------------------------------------------------
pub use utxo::{MultiEraTxOut, MultiEraUtxo};
