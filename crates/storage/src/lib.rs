//! Storage-facing abstractions for immutable blocks, volatile rollback windows,
//! and ledger snapshots.
//!
//! Each storage concern exposes a trait (`ImmutableStore`, `VolatileStore`,
//! `LedgerStore`) backed by both an in-memory implementation (for tests and
//! early integration) and a file-backed implementation (for durable storage).

/// Errors shared by all storage backends.
pub mod error;
/// Minimal ChainDB-style coordination across storage backends.
pub mod chain_db;
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
/// Rollback-aware volatile block storage.
pub mod volatile_db;

// -- Error re-exports ---------------------------------------------------------
pub use error::StorageError;

// -- Coordination re-exports --------------------------------------------------
pub use chain_db::{
	ChainDb, ChainDbRecovery, LedgerCheckpointRetention, LedgerRecoveryOutcome,
};

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
