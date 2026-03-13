//! Mempool-facing queue and entry abstractions.

mod queue;

/// Queue wrapper, transaction entry type, and mempool error.
pub use queue::{Mempool, MempoolEntry, MempoolError, MempoolRelayError};
