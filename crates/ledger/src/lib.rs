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
/// Ledger state containers and transition entry points.
pub mod state;
/// Transaction and block wrappers.
pub mod tx;
/// Core protocol-level types shared across ledger, storage, and consensus.
pub mod types;

// -- CBOR re-exports ----------------------------------------------------------
/// CBOR encoding and decoding traits and primitives.
pub use cbor::{CborDecode, CborEncode, Decoder, Encoder};

// -- Era re-exports -----------------------------------------------------------
/// Supported Cardano eras represented in the workspace.
pub use eras::Era;
pub use eras::{
    AllegraTxBody, AlonzoTxBody, AlonzoTxOut, AssetName, BabbageBlock, BabbageTxBody, BabbageTxOut,
    ByronBlock, ConwayBlock, ConwayTxBody, DatumOption, ExUnits, GovActionId, MaryTxBody,
    MaryTxOut, MintAsset, MultiAsset, NativeScript, PolicyId, ProposalProcedure, Redeemer,
    ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTx, ShelleyTxBody,
    ShelleyTxIn, ShelleyTxOut, ShelleyUtxo, ShelleyVkeyWitness, ShelleyVrfCert, ShelleyWitnessSet,
    Value, Vote, Voter, VotingProcedure, VotingProcedures, BYRON_SLOTS_PER_EPOCH,
};

// -- Error re-exports ---------------------------------------------------------
/// Errors surfaced by ledger-facing helpers.
pub use error::LedgerError;

// -- State re-exports ---------------------------------------------------------
/// Top-level ledger state wrapper.
pub use state::LedgerState;

// -- Tx/Block re-exports ------------------------------------------------------
/// Transaction and block wrapper types.
pub use tx::{Block, BlockHeader, Tx};

// -- Type re-exports ----------------------------------------------------------
pub use types::{
    AddrKeyHash, Address, Anchor, BaseAddress, BlockNo, DCert, DRep, EnterpriseAddress, EpochNo,
    GenesisDelegateHash, GenesisHash, HeaderHash, Nonce, Point, PointerAddress, PoolKeyHash,
    PoolMetadata, PoolParams, Relay, RewardAccount, ScriptHash, SlotNo, StakeCredential, TxId,
    UnitInterval, VrfKeyHash,
};
