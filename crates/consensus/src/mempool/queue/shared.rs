//! Shared wrapper for concurrent mempool access.
//!
//! Mirrors upstream `Ouroboros.Consensus.Mempool` STM-wrapped API —
//! the runtime-facing handle that preserves the underlying `Mempool`
//! policy type while allowing multiple tasks to take snapshots and
//! mutate the queue safely.
//!
//! Single public type:
//!
//! - `SharedMempool` — `Arc<RwLock<Mempool>>` plus a tokio `Notify`
//!   handle so `LocalTxMonitor::AwaitAcquire` can block until the
//!   mempool snapshot has changed.
//!
//! Extracted from `mempool/queue.rs` in R273d (Phase γ §R273 fourth slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. The Arc<RwLock<Mempool>> wrapper exists
//! only in Yggdrasil's runtime model — upstream uses STM `TVar` and
//! embeds the wrapping in `Mempool.hs` directly. This Yggdrasil-side
//! file isolates the concurrency wrapper for clarity.

use std::sync::{Arc, RwLock};

use yggdrasil_ledger::{ProtocolParameters, ShelleyTxIn, SlotNo, TxId};

use super::{
    Mempool, MempoolEntry, MempoolError, MempoolSnapshot, SharedTxSubmissionMempoolReader,
};

/// Shared wrapper for concurrent mempool access.
///
/// This is a thin runtime-facing handle that preserves `Mempool` as the queue
/// policy type while allowing multiple tasks to take snapshots and mutate the
/// queue safely.
#[derive(Clone, Debug)]
pub struct SharedMempool {
    inner: Arc<RwLock<Mempool>>,
    /// Notified whenever the mempool contents change (insert / remove / purge).
    /// Used by `LocalTxMonitor` `AwaitAcquire` to block until the mempool
    /// snapshot has changed, matching upstream behavior.
    change_notify: Arc<tokio::sync::Notify>,
}

impl SharedMempool {
    /// Create a shared mempool from an existing queue instance.
    pub fn new(mempool: Mempool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(mempool)),
            change_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Create a new shared mempool with the given maximum capacity in bytes.
    pub fn with_capacity(max_bytes: usize) -> Self {
        Self::new(Mempool::with_capacity(max_bytes))
    }

    /// Wait until the mempool contents change.
    ///
    /// Upstream: `Ouroboros.Network.Protocol.LocalTxMonitor.Server` —
    /// `MsgAwaitAcquire` blocks until the mempool generation counter
    /// increases.
    pub async fn wait_for_change(&self) {
        self.change_notify.notified().await;
    }

    /// Insert an entry if it does not already exist and fits within capacity.
    pub fn insert(&self, entry: MempoolEntry) -> Result<(), MempoolError> {
        let result = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .insert(entry);
        if result.is_ok() {
            self.change_notify.notify_waiters();
        }
        result
    }

    /// Like [`Self::insert`] but, on capacity overflow, evicts the
    /// lowest-fee tail to make room — see
    /// [`Mempool::insert_with_eviction`] for the upstream-aligned
    /// semantics. Notifies snapshot waiters when ANY change occurred
    /// (eviction or insertion).
    pub fn insert_with_eviction(&self, entry: MempoolEntry) -> Result<Vec<TxId>, MempoolError> {
        let result = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .insert_with_eviction(entry);
        if result.is_ok() {
            self.change_notify.notify_waiters();
        }
        result
    }

    /// Insert an entry with TTL validation against the current slot.
    pub fn insert_checked(
        &self,
        entry: MempoolEntry,
        current_slot: SlotNo,
        protocol_params: Option<&ProtocolParameters>,
    ) -> Result<(), MempoolError> {
        let result = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .insert_checked(entry, current_slot, protocol_params);
        if result.is_ok() {
            self.change_notify.notify_waiters();
        }
        result
    }

    /// Like [`Self::insert_checked`] but routes through
    /// [`Mempool::insert_checked_with_eviction`] so capacity-overflow
    /// failures fall back to evicting the lowest-fee tail rather than
    /// rejecting the incoming transaction outright. Returns the list
    /// of evicted `TxId`s on success.
    pub fn insert_checked_with_eviction(
        &self,
        entry: MempoolEntry,
        current_slot: SlotNo,
        protocol_params: Option<&ProtocolParameters>,
    ) -> Result<Vec<TxId>, MempoolError> {
        let result = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .insert_checked_with_eviction(entry, current_slot, protocol_params);
        if result.is_ok() {
            self.change_notify.notify_waiters();
        }
        result
    }

    /// Remove and return the highest-fee entry, if any.
    pub fn pop_best(&self) -> Option<MempoolEntry> {
        let result = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .pop_best();
        if result.is_some() {
            self.change_notify.notify_waiters();
        }
        result
    }

    /// Remove a transaction by its identifier. Returns `true` if found.
    pub fn remove_by_id(&self, tx_id: &TxId) -> bool {
        let removed = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .remove_by_id(tx_id);
        if removed {
            self.change_notify.notify_waiters();
        }
        removed
    }

    /// Remove all confirmed transactions and return the number removed.
    pub fn remove_confirmed(&self, confirmed_tx_ids: &[TxId]) -> usize {
        let count = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .remove_confirmed(confirmed_tx_ids);
        if count > 0 {
            self.change_notify.notify_waiters();
        }
        count
    }

    /// Remove mempool entries that conflict with consumed UTxO inputs.
    pub fn remove_conflicting_inputs(&self, consumed_inputs: &[ShelleyTxIn]) -> usize {
        let count = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .remove_conflicting_inputs(consumed_inputs);
        if count > 0 {
            self.change_notify.notify_waiters();
        }
        count
    }

    /// Remove all expired transactions and return the number removed.
    pub fn purge_expired(&self, current_slot: SlotNo) -> usize {
        let count = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .purge_expired(current_slot);
        if count > 0 {
            self.change_notify.notify_waiters();
        }
        count
    }

    /// Re-validate remaining transactions after a block has been applied.
    ///
    /// Removes entries whose inputs are no longer available or whose TTL has
    /// expired.  Returns the number of entries removed.
    pub fn revalidate(
        &self,
        current_slot: SlotNo,
        available_inputs: &std::collections::HashSet<ShelleyTxIn>,
    ) -> usize {
        let count = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .revalidate(current_slot, available_inputs);
        if count > 0 {
            self.change_notify.notify_waiters();
        }
        count
    }

    /// Re-validate all mempool entries against updated protocol parameters.
    ///
    /// Called at epoch boundaries when protocol parameters may have changed.
    /// Removes any entry that no longer satisfies the new fee, size, or
    /// ExUnits constraints.  Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Update` —
    /// `syncWithLedger` / `revalidateTx` epoch reconciliation pass.
    pub fn purge_invalid_for_params(
        &self,
        current_slot: SlotNo,
        params: &ProtocolParameters,
    ) -> usize {
        let count = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .purge_invalid_for_params(current_slot, params);
        if count > 0 {
            self.change_notify.notify_waiters();
        }
        count
    }

    /// Re-validate remaining mempool entries using a caller-provided
    /// ledger validation callback.
    ///
    /// Upstream `syncWithLedger` re-applies every remaining mempool tx
    /// against the post-block ledger state to evict stale transactions.
    /// The closure should capture a mutable reference to a scratch ledger
    /// state so that effects accumulate across entries.
    ///
    /// Returns the number of entries evicted.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` —
    /// `revalidateTxsFor`.
    pub fn revalidate_with_ledger<F>(&self, validate: F) -> usize
    where
        F: FnMut(&MempoolEntry) -> bool,
    {
        let count = self
            .inner
            .write()
            .expect("shared mempool poisoned")
            .revalidate_with_ledger(validate);
        if count > 0 {
            self.change_notify.notify_waiters();
        }
        count
    }

    /// Check whether a transaction with the given id exists in the mempool.
    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.inner
            .read()
            .expect("shared mempool poisoned")
            .contains(tx_id)
    }

    /// Number of transactions currently in the mempool.
    pub fn len(&self) -> usize {
        self.inner.read().expect("shared mempool poisoned").len()
    }

    /// Whether the mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.inner
            .read()
            .expect("shared mempool poisoned")
            .is_empty()
    }

    /// Current aggregate size of all entries in bytes.
    pub fn size_bytes(&self) -> usize {
        self.inner
            .read()
            .expect("shared mempool poisoned")
            .size_bytes()
    }

    /// Maximum capacity of the mempool in bytes (0 = unlimited).
    pub fn capacity(&self) -> usize {
        self.inner
            .read()
            .expect("shared mempool poisoned")
            .max_bytes
    }

    /// Create a pure owned snapshot of the current mempool contents.
    pub fn snapshot(&self) -> MempoolSnapshot {
        self.inner
            .read()
            .expect("shared mempool poisoned")
            .snapshot()
    }

    /// Build a TxSubmission reader for snapshot-based outbound serving.
    pub fn txsubmission_mempool_reader(&self) -> SharedTxSubmissionMempoolReader {
        SharedTxSubmissionMempoolReader {
            mempool: self.clone(),
        }
    }
}

impl Default for SharedMempool {
    fn default() -> Self {
        Self::new(Mempool::default())
    }
}
