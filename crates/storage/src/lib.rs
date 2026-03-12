//! Storage-facing abstractions for immutable blocks, volatile rollback windows,
//! and ledger snapshots.

/// Append-only immutable block storage helpers.
pub mod immutable_db;
/// Ledger snapshot storage helpers.
pub mod ledger_db;
/// Rollback-aware volatile block storage helpers.
pub mod volatile_db;

/// Append-only immutable block storage wrapper.
pub use immutable_db::ImmutableBlockStore;
/// Ledger snapshot storage wrapper.
pub use ledger_db::LedgerSnapshotStore;
/// Rollback-aware volatile block storage wrapper.
pub use volatile_db::VolatileBlockStore;
