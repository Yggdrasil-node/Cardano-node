//! Ledger-facing state, transaction, and era abstractions.

/// Era modeling and era-local modules.
pub mod eras;
mod error;
/// Ledger state containers and transition entry points.
pub mod state;
/// Transaction and block wrappers.
pub mod tx;

/// Supported Cardano eras represented in the workspace.
pub use eras::Era;
/// Errors surfaced by ledger-facing helpers.
pub use error::LedgerError;
/// Top-level ledger state wrapper.
pub use state::LedgerState;
/// Transaction and block wrapper types.
pub use tx::{Block, Tx};
