//! Mempool-facing queue and entry abstractions.

mod queue;

/// Queue wrapper and transaction entry type.
pub use queue::{Mempool, MempoolEntry};
