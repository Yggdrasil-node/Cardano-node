#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Storage-facing abstractions for immutable blocks, volatile rollback windows,
//! and ledger snapshots.
//!
//! Each storage concern exposes a trait (`ImmutableStore`, `VolatileStore`,
//! `LedgerStore`) backed by both an in-memory implementation (for tests and
//! early integration) and a file-backed implementation (for durable storage).

/// Minimal ChainDB-style coordination across storage backends.
pub mod chain_db;
/// Errors shared by all storage backends.
pub mod error;
/// File-backed immutable block storage.
pub mod file_immutable;
/// File-backed ledger snapshot storage.
pub mod file_ledger;
/// File-backed volatile block storage.
pub mod file_volatile;
/// Append-only immutable block storage.
pub mod immutable_db;
/// Ledger snapshot storage.
pub mod ledger_db;
/// Sidecar persistence for opaque consensus state files (OpCert counters).
pub mod ocert_sidecar;
/// Rollback-aware volatile block storage.
pub mod volatile_db;

// -- Error re-exports ---------------------------------------------------------
pub use error::StorageError;

// -- Sidecar re-exports -------------------------------------------------------
pub use ocert_sidecar::{
    NONCE_STATE_FILENAME, OCERT_COUNTERS_FILENAME, load_nonce_state, load_ocert_counters,
    save_nonce_state, save_ocert_counters,
};

// -- Coordination re-exports --------------------------------------------------
pub use chain_db::{ChainDb, ChainDbRecovery, LedgerCheckpointRetention, LedgerRecoveryOutcome};

// -- Trait re-exports ---------------------------------------------------------
pub use immutable_db::ImmutableStore;
pub use ledger_db::LedgerStore;
pub use volatile_db::VolatileStore;

// -- In-memory implementation re-exports --------------------------------------
pub use immutable_db::InMemoryImmutable;
pub use ledger_db::InMemoryLedgerStore;
pub use volatile_db::InMemoryVolatile;

// -- File-backed implementation re-exports ------------------------------------
pub use file_immutable::FileImmutable;
pub use file_ledger::FileLedgerStore;
pub use file_volatile::FileVolatile;
