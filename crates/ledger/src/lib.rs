//! Ledger-facing state, transaction, and era abstractions.
//!
//! This crate provides typed protocol-level identifiers (`SlotNo`, `BlockNo`,
//! `HeaderHash`, `TxId`, `Point`) alongside era modeling, block/transaction
//! structures, and ledger state tracking.

/// Era modeling and era-local modules.
pub mod eras;
mod error;
/// Ledger state containers and transition entry points.
pub mod state;
/// Transaction and block wrappers.
pub mod tx;
/// Core protocol-level types shared across ledger, storage, and consensus.
pub mod types;

// -- Era re-exports -----------------------------------------------------------
/// Supported Cardano eras represented in the workspace.
pub use eras::Era;

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
pub use types::{BlockNo, EpochNo, HeaderHash, Nonce, Point, SlotNo, TxId};
