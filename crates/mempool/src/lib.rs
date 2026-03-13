//! Mempool-facing queue and entry abstractions.

mod queue;

/// Queue wrapper, transaction entry type, and mempool error.
pub use queue::{
	MEMPOOL_ZERO_IDX, Mempool, MempoolEntry, MempoolError, MempoolIdx,
	MempoolRelayError, MempoolSnapshot, TxSubmissionMempoolReader,
};
