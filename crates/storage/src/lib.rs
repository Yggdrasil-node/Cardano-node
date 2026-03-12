//! Storage-facing abstractions for immutable blocks, volatile rollback windows,
//! and ledger snapshots.
//!
//! Each storage concern exposes a trait (`ImmutableStore`, `VolatileStore`,
//! `LedgerStore`) backed by an in-memory implementation suitable for tests
//! and early integration while on-disk formats are stabilized.

/// Errors shared by all storage backends.
pub mod error;
/// Append-only immutable block storage.
pub mod immutable_db;
/// Ledger snapshot storage.
pub mod ledger_db;
/// Rollback-aware volatile block storage.
pub mod volatile_db;

// -- Error re-exports ---------------------------------------------------------
pub use error::StorageError;

// -- Trait re-exports ---------------------------------------------------------
pub use immutable_db::ImmutableStore;
pub use ledger_db::LedgerStore;
pub use volatile_db::VolatileStore;

// -- In-memory implementation re-exports --------------------------------------
pub use immutable_db::InMemoryImmutable;
pub use ledger_db::InMemoryLedgerStore;
pub use volatile_db::InMemoryVolatile;
