#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Mempool — fee-ordered transaction queue with TTL eviction, cross-peer
//! TxId deduplication, and ledger revalidation.
//!
//! Folded into `yggdrasil-consensus` in R256 (Phase A) so the workspace
//! mirrors upstream `Ouroboros.Consensus.Mempool.*` organization. Pre-R256
//! Yggdrasil shipped this as the standalone `yggdrasil-mempool` crate; the
//! split was a vestigial build-graph isolation choice with no upstream
//! parallel.
//!
//! Reference: `Ouroboros.Consensus.Mempool` (top-level re-export),
//! `Ouroboros.Consensus.Mempool.{API, Capacity, Impl.{Common, Update},
//! Init, Query, TxSeq, Update}`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell that
//! aggregates upstream `Ouroboros.Consensus.Mempool.*` modules:
//! `API`, `Capacity`, `Impl/Common`, `Impl/Update`, `Init`,
//! `Query`, `TxSeq`, `Update`. Yggdrasil collapses these into a
//! single namespace with two sub-modules: `queue.rs` (mempool
//! data + capacity tracking + entry/error types) and `tx_state.rs`
//! (cross-peer shared state for TxId deduplication, mirroring
//! `Ouroboros.Network.TxSubmission.Inbound.V2.State`).

mod queue;
/// Cross-peer shared TxId deduplication state.
pub mod tx_state;

/// Queue wrapper, transaction entry type, and mempool error.
pub use queue::{
    MEMPOOL_ZERO_IDX, Mempool, MempoolEntry, MempoolError, MempoolIdx, MempoolRelayError,
    MempoolSnapshot, SharedMempool, SharedTxSubmissionMempoolReader, TxSubmissionMempoolReader,
};
pub use tx_state::{FilterOutcome, SharedTxState, SizeInBytes, TxState};
