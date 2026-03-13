use std::sync::{Arc, RwLock};

use yggdrasil_ledger::{Era, LedgerError, MultiEraSubmittedTx, SlotNo, TxId};

/// Monotonic transaction index used by TxSubmission mempool snapshots.
pub type MempoolIdx = i64;

/// Zero index for TxSubmission mempool snapshots.
pub const MEMPOOL_ZERO_IDX: MempoolIdx = -1;

/// A mempool entry carrying a transaction identifier and its fee for ordering.
///
/// The `tx_id` is the Blake2b-256 hash of the CBOR-encoded transaction body,
/// matching the upstream Cardano `TxId` representation.
///
/// Reference: `Cardano.Ledger.TxIn` — `TxId`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MempoolEntry {
    /// The era needed to decode the submitted transaction relay payload.
    pub era: Era,
    /// The transaction identifier (Blake2b-256 of CBOR body).
    pub tx_id: TxId,
    /// The transaction fee in lovelace, used for ordering.
    pub fee: u64,
    /// The raw CBOR-encoded transaction body bytes.
    pub body: Vec<u8>,
    /// The raw CBOR-encoded submitted transaction bytes used for relay.
    pub raw_tx: Vec<u8>,
    /// Size of the transaction in bytes (for capacity tracking).
    pub size_bytes: usize,
    /// Time-to-live slot — the transaction is invalid after this slot.
    /// Matches the Shelley `ttl` field semantics: valid while `current_slot <= ttl`.
    pub ttl: SlotNo,
}

/// Error type for converting mempool entries into typed submitted transactions.
#[derive(Debug, thiserror::Error)]
pub enum MempoolRelayError {
    /// The stored submitted transaction bytes could not be decoded for the
    /// entry's era.
    #[error("failed to decode submitted transaction for era {era:?}: {source}")]
    Decode {
        /// Era used to decode the submitted transaction payload.
        era: Era,
        /// Underlying ledger decode failure.
        source: LedgerError,
    },
    /// The entry's stored body bytes do not match the decoded transaction
    /// body.
    #[error("mempool entry body bytes do not match decoded submitted transaction body")]
    BodyMismatch,
    /// The entry's stored `TxId` does not match the decoded transaction body.
    #[error("mempool entry txid {expected} does not match decoded txid {actual}")]
    TxIdMismatch {
        /// TxId stored in the mempool entry.
        expected: TxId,
        /// TxId recomputed from the decoded submitted transaction.
        actual: TxId,
    },
}

impl MempoolEntry {
    /// Build a mempool entry from a typed multi-era submitted transaction.
    pub fn from_multi_era_submitted_tx(
        tx: MultiEraSubmittedTx,
        fee: u64,
        ttl: SlotNo,
    ) -> Self {
        let era = tx.era();
        let tx_id = tx.tx_id();
        let body = tx.body_cbor();
        let raw_tx = tx.raw_cbor();
        let size_bytes = raw_tx.len();
        Self {
            era,
            tx_id,
            fee,
            body,
            raw_tx,
            size_bytes,
            ttl,
        }
    }

    /// Decode the relay payload into a typed multi-era submitted transaction.
    pub fn to_multi_era_submitted_tx(&self) -> Result<MultiEraSubmittedTx, MempoolRelayError> {
        let tx = MultiEraSubmittedTx::from_cbor_bytes_for_era(self.era, &self.raw_tx).map_err(
            |source| MempoolRelayError::Decode {
                era: self.era,
                source,
            },
        )?;
        if tx.body_cbor() != self.body {
            return Err(MempoolRelayError::BodyMismatch);
        }
        let actual = tx.tx_id();
        if actual != self.tx_id {
            return Err(MempoolRelayError::TxIdMismatch {
                expected: self.tx_id,
                actual,
            });
        }
        Ok(tx)
    }
}

/// Error type for mempool operations.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MempoolError {
    /// The transaction is already in the mempool.
    #[error("duplicate transaction: {0:?}")]
    Duplicate(TxId),
    /// Adding this transaction would exceed the mempool capacity.
    #[error("mempool capacity exceeded: {current} + {incoming} > {limit}")]
    CapacityExceeded {
        /// Current total bytes in the mempool.
        current: usize,
        /// Size of the incoming transaction.
        incoming: usize,
        /// Maximum capacity in bytes.
        limit: usize,
    },
    /// The transaction has already expired at the given slot.
    #[error("transaction TTL expired: ttl {ttl:?} < current slot {current_slot:?}")]
    TtlExpired {
        /// The transaction's TTL slot.
        ttl: SlotNo,
        /// The current slot at admission time.
        current_slot: SlotNo,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndexedMempoolEntry {
    idx: MempoolIdx,
    entry: MempoolEntry,
}

/// Pure snapshot view used by the TxSubmission outbound side.
///
/// This mirrors the upstream `Ouroboros.Network.TxSubmission.Mempool.Reader`
/// snapshot terminology while exposing local mempool entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MempoolSnapshot {
    entries: Vec<IndexedMempoolEntry>,
}

impl MempoolSnapshot {
    /// Return all transaction ids after the provided index, oldest to newest.
    pub fn mempool_txids_after(&self, idx: MempoolIdx) -> Vec<(TxId, MempoolIdx, usize)> {
        let mut txids = self
            .entries
            .iter()
            .filter(|entry| entry.idx > idx)
            .map(|entry| (entry.entry.tx_id, entry.idx, entry.entry.size_bytes))
            .collect::<Vec<_>>();
        txids.sort_by_key(|(_, next_idx, _)| *next_idx);
        txids
    }

    /// Look up a transaction entry by its mempool index.
    pub fn mempool_lookup_tx(&self, idx: MempoolIdx) -> Option<&MempoolEntry> {
        self.entries
            .iter()
            .find(|entry| entry.idx == idx)
            .map(|entry| &entry.entry)
    }

    /// Determine whether the snapshot contains the given transaction id.
    pub fn mempool_has_tx(&self, tx_id: &TxId) -> bool {
        self.entries.iter().any(|entry| &entry.entry.tx_id == tx_id)
    }

    /// Look up a transaction entry by transaction id.
    pub fn mempool_lookup_tx_by_id(&self, tx_id: &TxId) -> Option<&MempoolEntry> {
        self.entries
            .iter()
            .find(|entry| &entry.entry.tx_id == tx_id)
            .map(|entry| &entry.entry)
    }
}

/// Reader for obtaining TxSubmission mempool snapshots.
///
/// Reference: `Ouroboros.Network.TxSubmission.Mempool.Reader`.
#[derive(Clone, Copy, Debug)]
pub struct TxSubmissionMempoolReader<'a> {
    mempool: &'a Mempool,
}

impl<'a> TxSubmissionMempoolReader<'a> {
    /// Grab a pure snapshot of the current mempool contents.
    pub fn mempool_get_snapshot(&self) -> MempoolSnapshot {
        self.mempool.snapshot()
    }

    /// Index value that returns all transactions when used with
    /// `mempool_txids_after`.
    pub fn mempool_zero_idx(&self) -> MempoolIdx {
        MEMPOOL_ZERO_IDX
    }
}

/// Snapshot reader backed by a shared mempool handle.
#[derive(Clone, Debug)]
pub struct SharedTxSubmissionMempoolReader {
    mempool: SharedMempool,
}

impl SharedTxSubmissionMempoolReader {
    /// Grab a pure snapshot of the current shared mempool contents.
    pub fn mempool_get_snapshot(&self) -> MempoolSnapshot {
        self.mempool.snapshot()
    }

    /// Index value that returns all transactions when used with
    /// `mempool_txids_after`.
    pub fn mempool_zero_idx(&self) -> MempoolIdx {
        MEMPOOL_ZERO_IDX
    }
}

/// A fee-ordered mempool with capacity tracking and duplicate detection.
///
/// Entries are ordered by descending fee so that `pop_best` always returns
/// the highest-fee transaction. Size tracking prevents unbounded growth.
///
/// Reference: `Ouroboros.Consensus.Mempool.API` — `Mempool`.
#[derive(Clone, Debug, Default)]
pub struct Mempool {
    entries: Vec<IndexedMempoolEntry>,
    /// Maximum aggregate size in bytes (0 = unlimited).
    max_bytes: usize,
    /// Current total size in bytes of all entries.
    current_bytes: usize,
    /// Next monotonic index assigned to an inserted transaction.
    next_idx: MempoolIdx,
}

impl Mempool {
    /// Create a new mempool with the given maximum capacity in bytes.
    ///
    /// A `max_bytes` of 0 means no capacity limit.
    pub fn with_capacity(max_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_bytes,
            current_bytes: 0,
            next_idx: 0,
        }
    }

    /// Insert an entry if it does not already exist and fits within capacity.
    ///
    /// Keeps the queue ordered by descending fee.
    pub fn insert(&mut self, entry: MempoolEntry) -> Result<(), MempoolError> {
        if self.entries.iter().any(|e| e.entry.tx_id == entry.tx_id) {
            return Err(MempoolError::Duplicate(entry.tx_id));
        }
        if self.max_bytes > 0 && self.current_bytes + entry.size_bytes > self.max_bytes {
            return Err(MempoolError::CapacityExceeded {
                current: self.current_bytes,
                incoming: entry.size_bytes,
                limit: self.max_bytes,
            });
        }
        self.current_bytes += entry.size_bytes;
        self.entries.push(IndexedMempoolEntry {
            idx: self.next_idx,
            entry,
        });
        self.next_idx += 1;
        self.entries
            .sort_by(|left, right| right.entry.fee.cmp(&left.entry.fee));
        Ok(())
    }

    /// Remove and return the highest-fee entry, if any.
    pub fn pop_best(&mut self) -> Option<MempoolEntry> {
        if self.entries.is_empty() {
            None
        } else {
            let entry = self.entries.remove(0);
            self.current_bytes -= entry.entry.size_bytes;
            Some(entry.entry)
        }
    }

    /// Remove a transaction by its identifier. Returns `true` if found.
    pub fn remove_by_id(&mut self, tx_id: &TxId) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| &e.entry.tx_id == tx_id) {
            let entry = self.entries.remove(pos);
            self.current_bytes -= entry.entry.size_bytes;
            true
        } else {
            false
        }
    }

    /// Check whether a transaction with the given id exists in the mempool.
    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.entries.iter().any(|e| &e.entry.tx_id == tx_id)
    }

    /// Number of transactions currently in the mempool.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current aggregate size of all entries in bytes.
    pub fn size_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Iterator over entries in fee-descending order.
    pub fn iter(&self) -> impl Iterator<Item = &MempoolEntry> {
        self.entries.iter().map(|entry| &entry.entry)
    }

    /// Create a pure owned snapshot of the current mempool contents.
    pub fn snapshot(&self) -> MempoolSnapshot {
        MempoolSnapshot {
            entries: self.entries.clone(),
        }
    }

    /// Build a TxSubmission reader for snapshot-based outbound serving.
    pub fn txsubmission_mempool_reader(&self) -> TxSubmissionMempoolReader<'_> {
        TxSubmissionMempoolReader { mempool: self }
    }

    /// Remove all transactions whose identifiers appear in the given block's
    /// transaction list, as they have been confirmed on-chain.
    ///
    /// Returns the number of entries removed.
    pub fn remove_confirmed(&mut self, confirmed_tx_ids: &[TxId]) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            if confirmed_tx_ids.contains(&e.entry.tx_id) {
                self.current_bytes -= e.entry.size_bytes;
                false
            } else {
                true
            }
        });
        before - self.entries.len()
    }

    /// Insert an entry with TTL validation against the current slot.
    ///
    /// The transaction is rejected if its TTL has already passed
    /// (`current_slot > entry.ttl`). Otherwise it proceeds through the
    /// normal `insert` checks (duplicate detection, capacity).
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — TTL check.
    pub fn insert_checked(
        &mut self,
        entry: MempoolEntry,
        current_slot: SlotNo,
    ) -> Result<(), MempoolError> {
        if current_slot > entry.ttl {
            return Err(MempoolError::TtlExpired {
                ttl: entry.ttl,
                current_slot,
            });
        }
        self.insert(entry)
    }

    /// Remove all transactions whose TTL has passed at the given slot.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — re-validation.
    pub fn purge_expired(&mut self, current_slot: SlotNo) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| {
            if current_slot > e.entry.ttl {
                self.current_bytes -= e.entry.size_bytes;
                false
            } else {
                true
            }
        });
        before - self.entries.len()
    }
}

/// Shared wrapper for concurrent mempool access.
///
/// This is a thin runtime-facing handle that preserves `Mempool` as the queue
/// policy type while allowing multiple tasks to take snapshots and mutate the
/// queue safely.
#[derive(Clone, Debug, Default)]
pub struct SharedMempool {
    inner: Arc<RwLock<Mempool>>,
}

impl SharedMempool {
    /// Create a shared mempool from an existing queue instance.
    pub fn new(mempool: Mempool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(mempool)),
        }
    }

    /// Create a new shared mempool with the given maximum capacity in bytes.
    pub fn with_capacity(max_bytes: usize) -> Self {
        Self::new(Mempool::with_capacity(max_bytes))
    }

    /// Insert an entry if it does not already exist and fits within capacity.
    pub fn insert(&self, entry: MempoolEntry) -> Result<(), MempoolError> {
        self.inner.write().expect("shared mempool poisoned").insert(entry)
    }

    /// Insert an entry with TTL validation against the current slot.
    pub fn insert_checked(
        &self,
        entry: MempoolEntry,
        current_slot: SlotNo,
    ) -> Result<(), MempoolError> {
        self.inner
            .write()
            .expect("shared mempool poisoned")
            .insert_checked(entry, current_slot)
    }

    /// Remove and return the highest-fee entry, if any.
    pub fn pop_best(&self) -> Option<MempoolEntry> {
        self.inner.write().expect("shared mempool poisoned").pop_best()
    }

    /// Remove a transaction by its identifier. Returns `true` if found.
    pub fn remove_by_id(&self, tx_id: &TxId) -> bool {
        self.inner
            .write()
            .expect("shared mempool poisoned")
            .remove_by_id(tx_id)
    }

    /// Remove all confirmed transactions and return the number removed.
    pub fn remove_confirmed(&self, confirmed_tx_ids: &[TxId]) -> usize {
        self.inner
            .write()
            .expect("shared mempool poisoned")
            .remove_confirmed(confirmed_tx_ids)
    }

    /// Remove all expired transactions and return the number removed.
    pub fn purge_expired(&self, current_slot: SlotNo) -> usize {
        self.inner
            .write()
            .expect("shared mempool poisoned")
            .purge_expired(current_slot)
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
        self.inner.read().expect("shared mempool poisoned").is_empty()
    }

    /// Current aggregate size of all entries in bytes.
    pub fn size_bytes(&self) -> usize {
        self.inner
            .read()
            .expect("shared mempool poisoned")
            .size_bytes()
    }

    /// Create a pure owned snapshot of the current mempool contents.
    pub fn snapshot(&self) -> MempoolSnapshot {
        self.inner.read().expect("shared mempool poisoned").snapshot()
    }

    /// Build a TxSubmission reader for snapshot-based outbound serving.
    pub fn txsubmission_mempool_reader(&self) -> SharedTxSubmissionMempoolReader {
        SharedTxSubmissionMempoolReader {
            mempool: self.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Mempool, MempoolEntry, SharedMempool, SlotNo, TxId, MEMPOOL_ZERO_IDX};

    fn sample_entry(seed: u8, fee: u64) -> MempoolEntry {
        MempoolEntry {
            era: yggdrasil_ledger::Era::Shelley,
            tx_id: TxId([seed; 32]),
            fee,
            body: vec![seed],
            raw_tx: vec![seed, seed.wrapping_add(1)],
            size_bytes: 2,
            ttl: SlotNo(100),
        }
    }

    #[test]
    fn txsubmission_reader_uses_monotonic_snapshot_order() {
        let mut mempool = Mempool::with_capacity(1024);
        let first = sample_entry(1, 10);
        let second = sample_entry(2, 200);

        mempool.insert(first.clone()).expect("insert first");
        mempool.insert(second.clone()).expect("insert second");

        let reader = mempool.txsubmission_mempool_reader();
        let snapshot = reader.mempool_get_snapshot();
        let txids = snapshot.mempool_txids_after(reader.mempool_zero_idx());

        assert_eq!(txids.len(), 2);
        assert_eq!(txids[0].0, first.tx_id);
        assert_eq!(txids[1].0, second.tx_id);
        assert_eq!(snapshot.mempool_lookup_tx(txids[0].1), Some(&first));
        assert_eq!(snapshot.mempool_lookup_tx(txids[1].1), Some(&second));
        assert!(snapshot.mempool_has_tx(&first.tx_id));
        assert_eq!(snapshot.mempool_lookup_tx_by_id(&second.tx_id), Some(&second));
    }

    #[test]
    fn shared_mempool_snapshot_reflects_updates() {
        let shared = SharedMempool::with_capacity(1024);
        let first = sample_entry(1, 10);
        let second = sample_entry(2, 200);

        shared.insert(first.clone()).expect("insert first");
        let snapshot = shared.snapshot();
        shared.insert(second.clone()).expect("insert second");

        let initial_txids = snapshot.mempool_txids_after(MEMPOOL_ZERO_IDX);
        assert_eq!(initial_txids.len(), 1);
        assert_eq!(initial_txids[0].0, first.tx_id);

        let updated_txids = shared
            .txsubmission_mempool_reader()
            .mempool_get_snapshot()
            .mempool_txids_after(MEMPOOL_ZERO_IDX);
        assert_eq!(updated_txids.len(), 2);
        assert_eq!(updated_txids[0].0, first.tx_id);
        assert_eq!(updated_txids[1].0, second.tx_id);
    }
}
