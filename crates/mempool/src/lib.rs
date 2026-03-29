#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Mempool-facing queue and entry abstractions.

mod queue;
/// Cross-peer shared TxId deduplication state.
pub mod tx_state;

/// Queue wrapper, transaction entry type, and mempool error.
pub use queue::{
	MEMPOOL_ZERO_IDX, Mempool, MempoolEntry, MempoolError, MempoolIdx,
	MempoolRelayError, MempoolSnapshot, SharedMempool,
	SharedTxSubmissionMempoolReader, TxSubmissionMempoolReader,
};
pub use tx_state::{FilterOutcome, SharedTxState, TxState};
