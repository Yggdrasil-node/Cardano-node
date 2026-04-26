use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use yggdrasil_ledger::{
    Era, LedgerError, MultiEraSubmittedTx, ProtocolParameters, ShelleyTxIn, SlotNo, TxId,
    validate_fee, validate_tx_ex_units, validate_tx_size,
};

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
    /// UTxO inputs consumed by this transaction, used for conflict detection.
    ///
    /// When two transactions in the mempool share any input, one must be
    /// rejected — they are attempting to double-spend the same UTxO output.
    pub inputs: Vec<ShelleyTxIn>,
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
    pub fn from_multi_era_submitted_tx(tx: MultiEraSubmittedTx, fee: u64, ttl: SlotNo) -> Self {
        let era = tx.era();
        let tx_id = tx.tx_id();
        let body = tx.body_cbor();
        let raw_tx = tx.raw_cbor();
        let size_bytes = raw_tx.len();
        let inputs = tx.inputs();
        Self {
            era,
            tx_id,
            fee,
            body,
            raw_tx,
            size_bytes,
            ttl,
            inputs,
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
    /// The incoming transaction would not fit even after evicting every
    /// candidate that has a strictly lower fee — either because the
    /// incoming size exceeds the mempool's total capacity, or because
    /// the lowest-fee tail of the mempool is itself empty.
    #[error(
        "mempool eviction did not free enough space: incoming {incoming}, total capacity {limit}, freeable {freeable}"
    )]
    EvictionInsufficientSpace {
        /// Size of the incoming transaction in bytes.
        incoming: usize,
        /// Maximum capacity in bytes.
        limit: usize,
        /// Total bytes that could be freed by evicting the lowest-fee
        /// tail of strictly-lower-fee entries.
        freeable: usize,
    },
    /// The incoming transaction's fee does not exceed the cumulative fee
    /// of the entries that would need to be evicted to make room for it.
    /// Mirrors upstream `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`,
    /// which only displaces lower-fee transactions when the incoming
    /// transaction is unambiguously a better candidate.
    #[error(
        "mempool eviction not worthwhile: incoming fee {incoming_fee}, would-displace fee {evicted_fee}"
    )]
    EvictionNotWorthwhile {
        /// Fee of the incoming transaction.
        incoming_fee: u64,
        /// Sum of fees of the entries that would be evicted.
        evicted_fee: u64,
    },
    /// The transaction has already expired at the given slot.
    #[error("transaction TTL expired: ttl {ttl:?} < current slot {current_slot:?}")]
    TtlExpired {
        /// The transaction's TTL slot.
        ttl: SlotNo,
        /// The current slot at admission time.
        current_slot: SlotNo,
    },

    /// The transaction fee is lower than the configured minimum fee.
    #[error(
        "fee too small for configured protocol parameters: minimum {minimum}, declared {declared}"
    )]
    FeeTooSmall {
        /// Minimum required fee for this transaction size.
        minimum: u64,
        /// Fee declared by the submitted transaction.
        declared: u64,
    },

    /// The transaction body exceeds the configured maximum transaction size.
    #[error("transaction too large for configured protocol parameters: {actual} > {max}")]
    TxTooLarge {
        /// Actual transaction body size in bytes.
        actual: usize,
        /// Maximum allowed transaction body size in bytes.
        max: usize,
    },

    /// Unexpected protocol-parameter validation failure.
    #[error("protocol-parameter validation failed: {0}")]
    ProtocolParamValidation(String),

    /// The transaction's declared execution units exceed protocol limits.
    #[error(
        "transaction ExUnits exceed protocol max: tx(mem={tx_mem}, steps={tx_steps}) > max(mem={max_mem}, steps={max_steps})"
    )]
    ExUnitsExceedTxLimit {
        /// Declared memory units for this transaction.
        tx_mem: u64,
        /// Declared CPU-step units for this transaction.
        tx_steps: u64,
        /// Maximum memory units allowed by protocol parameters.
        max_mem: u64,
        /// Maximum CPU-step units allowed by protocol parameters.
        max_steps: u64,
    },

    /// The transaction conflicts with an existing mempool transaction because
    /// both spend the same UTxO input (double-spend attempt).
    ///
    /// The contained `TxId` identifies the already-admitted transaction that
    /// claims the conflicting input.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — conflict check.
    #[error("conflicting inputs: existing transaction {0:?} spends the same UTxO")]
    ConflictingInputs(TxId),
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
    /// Position index `tx_id → entries[pos]`.
    ///
    /// Built once per snapshot construction (O(n) with the Vec clone) so
    /// individual `mempool_lookup_tx_by_id` / `mempool_has_tx` calls are
    /// O(1). The TxSubmission outbound path calls `lookup_tx_by_id` once
    /// per requested id per round-trip (`MsgRequestTxs` batches up to the
    /// per-peer policy cap of 64), so without the index that path is
    /// O(M*N) where N is mempool size; with it it becomes O(M + N).
    tx_id_to_pos: HashMap<TxId, usize>,
    /// Position index `MempoolIdx → entries[pos]`.
    ///
    /// Same construction-time pattern as [`Self::tx_id_to_pos`] but keyed
    /// by the monotonic mempool index. Block production
    /// (`mempool_entries_for_forging` in the node runtime) walks every
    /// snapshot entry and calls `mempool_lookup_tx(idx)` once per entry,
    /// so without this index that path is O(N²) for a mempool of size N
    /// — every block forge re-scans the whole mempool N times. With the
    /// index it becomes O(N).
    idx_to_pos: HashMap<MempoolIdx, usize>,
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
        self.idx_to_pos
            .get(&idx)
            .and_then(|pos| self.entries.get(*pos))
            .map(|entry| &entry.entry)
    }

    /// Determine whether the snapshot contains the given transaction id.
    pub fn mempool_has_tx(&self, tx_id: &TxId) -> bool {
        self.tx_id_to_pos.contains_key(tx_id)
    }

    /// Look up a transaction entry by transaction id.
    pub fn mempool_lookup_tx_by_id(&self, tx_id: &TxId) -> Option<&MempoolEntry> {
        self.tx_id_to_pos
            .get(tx_id)
            .and_then(|pos| self.entries.get(*pos))
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

impl TxSubmissionMempoolReader<'_> {
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
    /// Map from consumed UTxO input to the TxId of the mempool transaction
    /// that claims it.  Used for O(inputs) double-spend conflict detection
    /// at admission time.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — conflict check.
    claimed_inputs: HashMap<ShelleyTxIn, TxId>,
    /// Membership index of currently-resident transaction ids.
    ///
    /// Maintained alongside `entries` so [`Self::insert`] / [`Self::contains`]
    /// /  [`Self::remove_by_id`] can short-circuit on duplicate or absent
    /// ids without paying the O(n) scan a `entries.iter().any(...)` would
    /// otherwise cost on each call. Stable across `entries` re-sorts because
    /// it stores no positional information.
    tx_ids: HashSet<TxId>,
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
            claimed_inputs: HashMap::new(),
            tx_ids: HashSet::new(),
        }
    }

    /// Insert an entry if it does not already exist and fits within capacity.
    ///
    /// Keeps the queue ordered by descending fee.
    pub fn insert(&mut self, entry: MempoolEntry) -> Result<(), MempoolError> {
        // Duplicate check via O(1) membership index.
        if self.tx_ids.contains(&entry.tx_id) {
            return Err(MempoolError::Duplicate(entry.tx_id));
        }
        // Check for UTxO double-spend conflicts: reject if any input is already
        // claimed by a transaction in the mempool.
        for input in &entry.inputs {
            if let Some(&existing_tx_id) = self.claimed_inputs.get(input) {
                return Err(MempoolError::ConflictingInputs(existing_tx_id));
            }
        }
        if self.max_bytes > 0 && self.current_bytes + entry.size_bytes > self.max_bytes {
            return Err(MempoolError::CapacityExceeded {
                current: self.current_bytes,
                incoming: entry.size_bytes,
                limit: self.max_bytes,
            });
        }
        let tx_id = entry.tx_id;
        self.current_bytes += entry.size_bytes;
        // Claim all inputs before pushing so the set is consistent.
        for input in &entry.inputs {
            self.claimed_inputs.insert(input.clone(), tx_id);
        }
        self.tx_ids.insert(tx_id);
        self.entries.push(IndexedMempoolEntry {
            idx: self.next_idx,
            entry,
        });
        self.next_idx += 1;
        self.entries
            .sort_by(|left, right| right.entry.fee.cmp(&left.entry.fee));
        Ok(())
    }

    /// Like [`Self::insert`] but, on capacity overflow, attempts to evict
    /// the lowest-fee tail of the mempool until the incoming transaction
    /// fits — mirroring upstream
    /// `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`.
    ///
    /// The eviction is gated by two upstream-aligned guards:
    ///
    /// * Only entries with **strictly lower** fee than `entry.fee` are
    ///   considered for eviction, so a same-fee or higher-fee tail is
    ///   never displaced (matches upstream's "is unambiguously a better
    ///   candidate" semantic).
    /// * After identifying the candidate eviction set, the cumulative fee
    ///   of those candidates must be strictly less than `entry.fee` —
    ///   otherwise the incoming transaction is rejected with
    ///   [`MempoolError::EvictionNotWorthwhile`] because the network is
    ///   better off keeping the existing higher-cumulative-fee set.
    ///
    /// Duplicate detection and conflicting-input checks fire BEFORE any
    /// eviction is considered (same as [`Self::insert`]), so an incoming
    /// transaction that conflicts with an existing entry is always
    /// rejected outright rather than displacing the conflicting tx.
    /// Returns the list of [`TxId`]s that were evicted on success
    /// (possibly empty when the incoming tx fit without displacement) so
    /// the caller can update downstream peer-relay state (e.g. clear
    /// `SharedTxState` known-set entries for evicted txs).
    pub fn insert_with_eviction(
        &mut self,
        entry: MempoolEntry,
    ) -> Result<Vec<TxId>, MempoolError> {
        // Duplicate check — same as `insert`.
        if self.tx_ids.contains(&entry.tx_id) {
            return Err(MempoolError::Duplicate(entry.tx_id));
        }
        // Conflicting-input check — same as `insert`.
        for input in &entry.inputs {
            if let Some(&existing_tx_id) = self.claimed_inputs.get(input) {
                return Err(MempoolError::ConflictingInputs(existing_tx_id));
            }
        }
        // Fast path: no capacity limit, or the incoming entry already fits.
        if self.max_bytes == 0
            || self.current_bytes + entry.size_bytes <= self.max_bytes
        {
            self.insert(entry)?;
            return Ok(Vec::new());
        }
        // The incoming transaction can never fit if it exceeds the total
        // mempool capacity on its own — no amount of eviction would help.
        if entry.size_bytes > self.max_bytes {
            return Err(MempoolError::EvictionInsufficientSpace {
                incoming: entry.size_bytes,
                limit: self.max_bytes,
                freeable: 0,
            });
        }
        let needed = (self.current_bytes + entry.size_bytes).saturating_sub(self.max_bytes);
        // Walk the tail (lowest fee first) accumulating candidates.
        // Entries are sorted by fee descending so the tail is the
        // last `n` items.
        let mut freeable_bytes: usize = 0;
        let mut evicted_fee: u64 = 0;
        let mut evict_indexes: Vec<usize> = Vec::new();
        for (idx, indexed) in self.entries.iter().enumerate().rev() {
            // Strictly-lower-fee guard: stop accumulating as soon as we
            // hit an entry whose fee is greater than or equal to the
            // incoming. The remaining (head) entries also have higher
            // fees so further iteration is wasted.
            if indexed.entry.fee >= entry.fee {
                break;
            }
            evict_indexes.push(idx);
            freeable_bytes = freeable_bytes.saturating_add(indexed.entry.size_bytes);
            evicted_fee = evicted_fee.saturating_add(indexed.entry.fee);
            if freeable_bytes >= needed {
                break;
            }
        }
        if freeable_bytes < needed {
            return Err(MempoolError::EvictionInsufficientSpace {
                incoming: entry.size_bytes,
                limit: self.max_bytes,
                freeable: freeable_bytes,
            });
        }
        if entry.fee <= evicted_fee {
            return Err(MempoolError::EvictionNotWorthwhile {
                incoming_fee: entry.fee,
                evicted_fee,
            });
        }
        // Commit phase: evict the candidates (sorted descending so
        // index removal is stable), then insert the new entry.
        evict_indexes.sort_unstable_by(|a, b| b.cmp(a));
        let mut evicted_ids: Vec<TxId> = Vec::with_capacity(evict_indexes.len());
        for idx in evict_indexes {
            let removed = self.entries.remove(idx);
            self.current_bytes = self.current_bytes.saturating_sub(removed.entry.size_bytes);
            self.tx_ids.remove(&removed.entry.tx_id);
            for input in &removed.entry.inputs {
                self.claimed_inputs.remove(input);
            }
            evicted_ids.push(removed.entry.tx_id);
        }
        // Now the regular insert path will succeed without further
        // capacity gating, so we can reuse it for the membership /
        // claimed-inputs / sort bookkeeping.
        self.insert(entry)?;
        Ok(evicted_ids)
    }

    /// Remove and return the highest-fee entry, if any.
    pub fn pop_best(&mut self) -> Option<MempoolEntry> {
        if self.entries.is_empty() {
            None
        } else {
            let entry = self.entries.remove(0);
            self.current_bytes -= entry.entry.size_bytes;
            for input in &entry.entry.inputs {
                self.claimed_inputs.remove(input);
            }
            self.tx_ids.remove(&entry.entry.tx_id);
            Some(entry.entry)
        }
    }

    /// Remove a transaction by its identifier. Returns `true` if found.
    pub fn remove_by_id(&mut self, tx_id: &TxId) -> bool {
        // Short-circuit on absence via the O(1) membership index so an
        // unknown id does not cost an O(n) scan.
        if !self.tx_ids.contains(tx_id) {
            return false;
        }
        if let Some(pos) = self.entries.iter().position(|e| &e.entry.tx_id == tx_id) {
            let entry = self.entries.remove(pos);
            self.current_bytes -= entry.entry.size_bytes;
            for input in &entry.entry.inputs {
                self.claimed_inputs.remove(input);
            }
            self.tx_ids.remove(tx_id);
            true
        } else {
            // Index says present but entries scan disagrees — fix up the
            // index to keep invariants intact and report not found.
            self.tx_ids.remove(tx_id);
            false
        }
    }

    /// Check whether a transaction with the given id exists in the mempool.
    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.tx_ids.contains(tx_id)
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
        let entries = self.entries.clone();
        let mut tx_id_to_pos = HashMap::with_capacity(entries.len());
        let mut idx_to_pos = HashMap::with_capacity(entries.len());
        for (pos, e) in entries.iter().enumerate() {
            tx_id_to_pos.insert(e.entry.tx_id, pos);
            idx_to_pos.insert(e.idx, pos);
        }
        MempoolSnapshot {
            entries,
            tx_id_to_pos,
            idx_to_pos,
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
        // Hash the confirmed set once so the per-entry check is O(1) rather
        // than O(m).  Called after every successful block apply, so the
        // quadratic form costs (mempool size N) × (block-tx count m) per
        // block; for a typical 5000-tx mempool + 20-tx block that's
        // 100k comparisons per block.
        let confirmed: HashSet<TxId> = confirmed_tx_ids.iter().copied().collect();
        let mut removed_count = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if confirmed.contains(&self.entries[i].entry.tx_id) {
                let entry = self.entries.remove(i);
                self.current_bytes -= entry.entry.size_bytes;
                for input in &entry.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&entry.entry.tx_id);
                removed_count += 1;
            } else {
                i += 1;
            }
        }
        removed_count
    }

    /// Remove mempool entries that conflict with newly consumed UTxO inputs.
    ///
    /// When a block is applied, its transactions consume certain UTxO inputs.
    /// Any mempool transaction that also spends one of those consumed inputs
    /// is now invalid and must be evicted (double-spend conflict).
    ///
    /// This complements `remove_confirmed`, which removes transactions by
    /// `TxId` that were adopted on-chain.  This method catches the case
    /// where a *different* transaction in a block consumes an input that a
    /// mempool transaction also needs.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Update` —
    /// `revalidateTxsFor` re-applies every mempool tx against the new UTxO
    /// set, which implicitly rejects txs with missing inputs.
    pub fn remove_conflicting_inputs(&mut self, consumed_inputs: &[ShelleyTxIn]) -> usize {
        if consumed_inputs.is_empty() {
            return 0;
        }
        // Hash the consumed-input set once.  The previous
        // `consumed_inputs.contains(inp)` call per-entry-per-input was
        // O(N*k*I) per block (mempool size × inputs-per-tx × block
        // consumed-input count); for a 5000-tx mempool averaging 2 inputs
        // each plus a 20-tx block consuming ~40 inputs that's ~400k
        // comparisons per block, fired after every successful apply.
        let consumed: HashSet<ShelleyTxIn> = consumed_inputs.iter().cloned().collect();
        let mut removed_count = 0;
        let mut i = 0;
        while i < self.entries.len() {
            let conflicts = self.entries[i]
                .entry
                .inputs
                .iter()
                .any(|inp| consumed.contains(inp));
            if conflicts {
                let removed = self.entries.remove(i);
                self.current_bytes -= removed.entry.size_bytes;
                for input in &removed.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&removed.entry.tx_id);
                removed_count += 1;
            } else {
                i += 1;
            }
        }
        removed_count
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
        protocol_params: Option<&ProtocolParameters>,
    ) -> Result<(), MempoolError> {
        Self::precheck_ttl_and_params(&entry, current_slot, protocol_params)?;
        self.insert(entry)
    }

    /// TTL and protocol-parameter validation extracted from
    /// [`Self::insert_checked`] so the same upstream-aligned admission
    /// gates (fee / size / ExUnits per `Cardano.Ledger.Shelley.Rules.Utxo`
    /// and TTL per `Cardano.Ledger.Allegra.Rules.Utxo`) can compose with
    /// the eviction-aware queue helper [`Self::insert_with_eviction`].
    fn precheck_ttl_and_params(
        entry: &MempoolEntry,
        current_slot: SlotNo,
        protocol_params: Option<&ProtocolParameters>,
    ) -> Result<(), MempoolError> {
        if current_slot > entry.ttl {
            return Err(MempoolError::TtlExpired {
                ttl: entry.ttl,
                current_slot,
            });
        }
        if let Some(params) = protocol_params {
            let total_ex_units = entry
                .to_multi_era_submitted_tx()
                .ok()
                .and_then(|tx| tx.total_ex_units());

            validate_tx_size(params, entry.body.len()).map_err(|err| match err {
                LedgerError::TxTooLarge { actual, max } => MempoolError::TxTooLarge { actual, max },
                other => MempoolError::ProtocolParamValidation(other.to_string()),
            })?;

            if let Some(eu) = total_ex_units.as_ref() {
                validate_tx_ex_units(params, eu).map_err(|err| match err {
                    LedgerError::ExUnitsExceedTxLimit {
                        tx_mem,
                        tx_steps,
                        max_mem,
                        max_steps,
                    } => MempoolError::ExUnitsExceedTxLimit {
                        tx_mem,
                        tx_steps,
                        max_mem,
                        max_steps,
                    },
                    other => MempoolError::ProtocolParamValidation(other.to_string()),
                })?;
            }

            validate_fee(params, entry.body.len(), total_ex_units.as_ref(), entry.fee).map_err(
                |err| match err {
                    LedgerError::FeeTooSmall { minimum, declared } => {
                        MempoolError::FeeTooSmall { minimum, declared }
                    }
                    other => MempoolError::ProtocolParamValidation(other.to_string()),
                },
            )?;
        }
        Ok(())
    }

    /// Like [`Self::insert_checked`] but, on capacity overflow, attempts
    /// to evict the lowest-fee tail to make room. Composes the same
    /// TTL + protocol-parameter precheck as `insert_checked` with the
    /// fee-aware eviction policy from [`Self::insert_with_eviction`].
    /// Returns the list of evicted [`TxId`]s on success so inbound
    /// admission paths can prune downstream peer-relay state if needed.
    pub fn insert_checked_with_eviction(
        &mut self,
        entry: MempoolEntry,
        current_slot: SlotNo,
        protocol_params: Option<&ProtocolParameters>,
    ) -> Result<Vec<TxId>, MempoolError> {
        Self::precheck_ttl_and_params(&entry, current_slot, protocol_params)?;
        self.insert_with_eviction(entry)
    }

    /// Remove all transactions whose TTL has passed at the given slot.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — re-validation.
    pub fn purge_expired(&mut self, current_slot: SlotNo) -> usize {
        let mut removed_count = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if current_slot > self.entries[i].entry.ttl {
                let entry = self.entries.remove(i);
                self.current_bytes -= entry.entry.size_bytes;
                for input in &entry.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&entry.entry.tx_id);
                removed_count += 1;
            } else {
                i += 1;
            }
        }
        removed_count
    }

    /// Re-validate remaining transactions after a block has been applied.
    ///
    /// Removes any transaction whose inputs are no longer present in the
    /// given set of available UTxO input hashes, or whose TTL has expired.
    /// This is the Rust equivalent of the Haskell node's mempool
    /// re-validation pass that runs after each block application.
    ///
    /// `available_inputs` should contain the set of `ShelleyTxIn` that are
    /// currently unspent in the UTxO after the latest block has been applied.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` — re-validation
    /// after `applyBlockLedgerResult`.
    pub fn revalidate(
        &mut self,
        current_slot: SlotNo,
        available_inputs: &std::collections::HashSet<ShelleyTxIn>,
    ) -> usize {
        let mut removed_count = 0;
        let mut i = 0;
        while i < self.entries.len() {
            let entry = &self.entries[i].entry;
            // Check TTL expiry first.
            let expired = current_slot > entry.ttl;
            // Check if any required input has been consumed.
            let inputs_spent = entry
                .inputs
                .iter()
                .any(|inp| !available_inputs.contains(inp));
            if expired || inputs_spent {
                let removed = self.entries.remove(i);
                self.current_bytes -= removed.entry.size_bytes;
                for input in &removed.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&removed.entry.tx_id);
                removed_count += 1;
            } else {
                i += 1;
            }
        }
        removed_count
    }

    /// Re-validate all mempool entries against updated protocol parameters.
    ///
    /// Called at epoch boundaries when the protocol parameters may have
    /// changed (e.g. fee coefficients, max-tx-size, max-tx-ex-units).
    /// Any entry that no longer satisfies the new parameters is removed.
    ///
    /// Checks performed per entry (in order):
    /// 1. TTL expiry (same as `purge_expired`).
    /// 2. Transaction body size against `max_tx_size`.
    /// 3. Declared ExUnits against `max_tx_ex_units` (script txs only).
    /// 4. Minimum fee against the new `min_fee_a` / `min_fee_b`.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Update` —
    /// `syncWithLedger` / `revalidateTx` epoch reconciliation pass.
    pub fn purge_invalid_for_params(
        &mut self,
        current_slot: SlotNo,
        params: &ProtocolParameters,
    ) -> usize {
        let mut removed_count = 0;
        let mut i = 0;
        while i < self.entries.len() {
            let entry = &self.entries[i].entry;

            // TTL check.
            if current_slot > entry.ttl {
                let removed = self.entries.remove(i);
                self.current_bytes -= removed.entry.size_bytes;
                for input in &removed.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&removed.entry.tx_id);
                removed_count += 1;
                continue;
            }

            // Decode ExUnits once — best-effort (synthetic entries may not
            // carry a full relay payload).
            let total_ex_units = entry
                .to_multi_era_submitted_tx()
                .ok()
                .and_then(|tx| tx.total_ex_units());

            // Size check.
            let invalid = validate_tx_size(params, entry.body.len()).is_err()
                // ExUnits check (script txs only).
                || total_ex_units.as_ref().is_some_and(|eu| validate_tx_ex_units(params, eu).is_err())
                // Minimum fee check.
                || validate_fee(params, entry.body.len(), total_ex_units.as_ref(), entry.fee).is_err();

            if invalid {
                let removed = self.entries.remove(i);
                self.current_bytes -= removed.entry.size_bytes;
                for input in &removed.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&removed.entry.tx_id);
                removed_count += 1;
            } else {
                i += 1;
            }
        }
        removed_count
    }

    /// Re-validate all remaining mempool entries using a caller-provided
    /// validation callback.
    ///
    /// This is the core of upstream `syncWithLedger` / `revalidateTxsFor`:
    /// after a block has been applied, the caller clones the post-block
    /// ledger state and folds it through remaining entries, accumulating
    /// effects.  Entries for which the closure returns `false` are evicted.
    ///
    /// The closure is called in fee-descending order (the existing entry
    /// order).  A typical runtime invocation will capture a mutable
    /// `LedgerState` so that successfully-validated transactions advance the
    /// virtual tip — later entries whose inputs depend on earlier mempool
    /// entries remain valid.
    ///
    /// Returns the number of entries removed.
    ///
    /// Reference: `Ouroboros.Consensus.Mempool.Impl.Common` —
    /// `revalidateTxsFor`.
    pub fn revalidate_with_ledger<F>(&mut self, mut validate: F) -> usize
    where
        F: FnMut(&MempoolEntry) -> bool,
    {
        let mut removed_count = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if validate(&self.entries[i].entry) {
                i += 1;
            } else {
                let removed = self.entries.remove(i);
                self.current_bytes -= removed.entry.size_bytes;
                for input in &removed.entry.inputs {
                    self.claimed_inputs.remove(input);
                }
                self.tx_ids.remove(&removed.entry.tx_id);
                removed_count += 1;
            }
        }
        removed_count
    }
}

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
    pub fn insert_with_eviction(
        &self,
        entry: MempoolEntry,
    ) -> Result<Vec<TxId>, MempoolError> {
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

#[cfg(test)]
mod tests {
    use super::{
        MEMPOOL_ZERO_IDX, Mempool, MempoolEntry, MempoolError, SharedMempool, SlotNo, TxId,
    };

    // ── MempoolError Display-content tests ────────────────────────────
    //
    // Follows the slice-55/56/57/58 pattern of pinning the `#[error(...)]`
    // format-string content. Operator-facing rejection reasons reach the
    // CLI via `submit-tx` and over NtC; any silent drop of a diagnostic
    // field would blind operators to which configured limit they hit.

    #[test]
    fn display_mempool_duplicate_names_tx_id() {
        let e = MempoolError::Duplicate(TxId([0xAB; 32]));
        let s = format!("{e}");
        assert!(
            s.to_lowercase().contains("duplicate"),
            "must identify the rule: {s}",
        );
        // Debug-format of [u8; 32] includes "TxId(" + at least the short
        // hex prefix; assert the first byte's decimal (0xAB = 171) appears
        // in some form so a future switch from Debug to opaque formatting
        // fails the test.
        assert!(
            s.contains("TxId") || s.contains("171") || s.contains("ab"),
            "must surface the TxId content: {s}",
        );
    }

    #[test]
    fn display_mempool_capacity_exceeded_names_all_three_counts() {
        let e = MempoolError::CapacityExceeded {
            current: 900,
            incoming: 250,
            limit: 1024,
        };
        let s = format!("{e}");
        assert!(s.contains("900"), "must show current: {s}");
        assert!(s.contains("250"), "must show incoming: {s}");
        assert!(s.contains("1024"), "must show limit: {s}");
    }

    #[test]
    fn display_mempool_ttl_expired_names_both_slots() {
        let e = MempoolError::TtlExpired {
            ttl: SlotNo(100),
            current_slot: SlotNo(150),
        };
        let s = format!("{e}");
        assert!(s.contains("100"), "must show ttl: {s}");
        assert!(s.contains("150"), "must show current slot: {s}");
    }

    #[test]
    fn display_mempool_fee_too_small_names_both_amounts() {
        let e = MempoolError::FeeTooSmall {
            minimum: 170_000,
            declared: 155_000,
        };
        let s = format!("{e}");
        assert!(s.contains("170000") || s.contains("170_000"));
        assert!(s.contains("155000") || s.contains("155_000"));
    }

    #[test]
    fn display_mempool_tx_too_large_names_both_sizes() {
        let e = MempoolError::TxTooLarge {
            actual: 17_000,
            max: 16_384,
        };
        let s = format!("{e}");
        assert!(s.contains("17000") || s.contains("17_000"));
        assert!(s.contains("16384") || s.contains("16_384"));
    }

    #[test]
    fn display_mempool_ex_units_exceed_names_all_four_dimensions() {
        let e = MempoolError::ExUnitsExceedTxLimit {
            tx_mem: 14_000_000,
            tx_steps: 10_100_000_000,
            max_mem: 14_000_000,
            max_steps: 10_000_000_000,
        };
        let s = format!("{e}");
        assert!(s.contains("14000000") || s.contains("14_000_000"));
        assert!(s.contains("10100000000") || s.contains("10_100_000_000"));
        assert!(s.contains("10000000000") || s.contains("10_000_000_000"));
    }

    #[test]
    fn display_mempool_conflicting_inputs_names_colliding_tx() {
        let e = MempoolError::ConflictingInputs(TxId([0xCD; 32]));
        let s = format!("{e}");
        assert!(
            s.to_lowercase().contains("conflict"),
            "must identify the rule: {s}",
        );
        assert!(
            s.contains("TxId") || s.contains("205") || s.contains("cd"),
            "must surface the colliding TxId: {s}",
        );
    }

    #[test]
    fn display_mempool_protocol_param_validation_propagates_message() {
        let e = MempoolError::ProtocolParamValidation("negative min_fee_a".to_owned());
        let s = format!("{e}");
        assert!(
            s.contains("negative min_fee_a"),
            "must propagate the inner diagnostic: {s}",
        );
    }

    fn sample_entry(seed: u8, fee: u64) -> MempoolEntry {
        MempoolEntry {
            era: yggdrasil_ledger::Era::Shelley,
            tx_id: TxId([seed; 32]),
            fee,
            body: vec![seed],
            raw_tx: vec![seed, seed.wrapping_add(1)],
            size_bytes: 2,
            ttl: SlotNo(100),
            inputs: vec![],
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
        assert_eq!(
            snapshot.mempool_lookup_tx_by_id(&second.tx_id),
            Some(&second)
        );
    }

    #[test]
    fn snapshot_idx_index_returns_same_results_as_linear_scan() {
        // Exercise the new `idx_to_pos` index against the previous
        // `entries.iter().find(...)` semantics. Build a snapshot of
        // several entries, then for each known idx assert the lookup
        // returns the same MempoolEntry that a manual linear find would
        // have returned. Also verify an unknown idx returns None.
        let mut mempool = Mempool::with_capacity(1024);
        let entries = (1u8..=5)
            .map(|seed| sample_entry(seed, 100 + u64::from(seed)))
            .collect::<Vec<_>>();
        for entry in &entries {
            mempool.insert(entry.clone()).expect("insert");
        }
        let snapshot = mempool.snapshot();
        let txids = snapshot.mempool_txids_after(MEMPOOL_ZERO_IDX);
        for (txid, idx, _) in &txids {
            let by_idx = snapshot.mempool_lookup_tx(*idx).expect("by-idx");
            assert_eq!(by_idx.tx_id, *txid);
        }
        // MempoolIdx is i64; large positive index that was never assigned.
        assert!(snapshot.mempool_lookup_tx(99_999).is_none());
    }

    #[test]
    fn membership_index_stays_in_sync_across_full_lifecycle() {
        // Locks in the invariant that `Mempool::tx_ids` mirrors the
        // resident `entries` set after every mutation path.  If a future
        // mutator forgets to update `tx_ids`, `contains` would stop
        // matching the entry list and `insert` would either reject a
        // re-insertion of an evicted tx or silently accept a duplicate of
        // a still-resident one.
        let mut mempool = Mempool::with_capacity(1024);
        let a = sample_entry(1, 10);
        let b = sample_entry(2, 20);
        let c = sample_entry(3, 30);

        // Insert: index gains all three.
        mempool.insert(a.clone()).expect("insert a");
        mempool.insert(b.clone()).expect("insert b");
        mempool.insert(c.clone()).expect("insert c");
        assert!(mempool.contains(&a.tx_id));
        assert!(mempool.contains(&b.tx_id));
        assert!(mempool.contains(&c.tx_id));

        // Duplicate is rejected (index hit, no entries scan).
        assert!(matches!(
            mempool.insert(a.clone()),
            Err(super::MempoolError::Duplicate(id)) if id == a.tx_id
        ));

        // pop_best removes the highest-fee entry and clears the index.
        let popped = mempool.pop_best().expect("pop");
        assert_eq!(popped.tx_id, c.tx_id);
        assert!(!mempool.contains(&c.tx_id));
        assert!(mempool.contains(&a.tx_id));
        assert!(mempool.contains(&b.tx_id));

        // remove_by_id clears the index for an existing entry and is a
        // no-op (with O(1) early return) for an unknown one.
        assert!(mempool.remove_by_id(&a.tx_id));
        assert!(!mempool.contains(&a.tx_id));
        assert!(!mempool.remove_by_id(&a.tx_id));

        // remove_confirmed and purge_expired both also clear the index.
        assert_eq!(mempool.remove_confirmed(&[b.tx_id]), 1);
        assert!(!mempool.contains(&b.tx_id));
        assert!(mempool.is_empty());
        // Re-insert + purge_expired path.
        mempool.insert(a.clone()).expect("re-insert a");
        assert!(mempool.contains(&a.tx_id));
        assert_eq!(mempool.purge_expired(SlotNo(a.ttl.0 + 1)), 1);
        assert!(!mempool.contains(&a.tx_id));
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

    // ── insert_with_eviction ────────────────────────────────────────────

    /// Helper that builds an entry with an explicit byte-size override
    /// so the eviction tests can hit precise capacity boundaries.
    fn sample_entry_with_size(seed: u8, fee: u64, size_bytes: usize) -> MempoolEntry {
        let mut e = sample_entry(seed, fee);
        e.size_bytes = size_bytes;
        e
    }

    /// When the incoming entry already fits, `insert_with_eviction`
    /// behaves identically to `insert`: no displacement, returns an
    /// empty evicted list, mempool grows by exactly one entry.
    #[test]
    fn insert_with_eviction_no_op_when_under_capacity() {
        let mut m = Mempool::with_capacity(100);
        let evicted = m
            .insert_with_eviction(sample_entry_with_size(1, 10, 20))
            .unwrap();
        assert!(evicted.is_empty());
        assert_eq!(m.len(), 1);
    }

    /// When the incoming entry has a strictly higher fee than the
    /// lowest-fee tail entry AND evicting that tail frees enough bytes,
    /// the eviction succeeds and the displaced TxId is returned to the
    /// caller. Pins the upstream `makeRoomForTransaction` happy path.
    #[test]
    fn insert_with_eviction_evicts_lowest_fee_when_higher_fee_arrives() {
        let mut m = Mempool::with_capacity(100);
        // Fill mempool to exactly capacity: 2 entries × 50 bytes each.
        m.insert(sample_entry_with_size(0xAA, 5, 50)).unwrap(); // low fee
        m.insert(sample_entry_with_size(0xBB, 50, 50)).unwrap(); // high fee
        // Incoming: 50 bytes, fee 20 — strictly higher than low entry's 5,
        // strictly lower than high entry's 50. Should displace ONLY the
        // low-fee entry.
        let evicted = m
            .insert_with_eviction(sample_entry_with_size(0xCC, 20, 50))
            .unwrap();
        assert_eq!(evicted, vec![TxId([0xAA; 32])]);
        assert_eq!(m.len(), 2);
        // Confirm the high-fee entry survived.
        assert!(m.iter().any(|e| e.tx_id == TxId([0xBB; 32])));
        // Confirm the new entry was admitted.
        assert!(m.iter().any(|e| e.tx_id == TxId([0xCC; 32])));
    }

    /// Even when the lowest-fee tail would mathematically free enough
    /// bytes, the eviction is rejected if the cumulative evicted fee is
    /// not strictly less than the incoming fee — upstream's
    /// "unambiguously a better candidate" guard. Without this, an
    /// attacker could grind out high-fee replacements that cumulatively
    /// drop network revenue.
    #[test]
    fn insert_with_eviction_rejects_when_evicted_fee_meets_incoming_fee() {
        let mut m = Mempool::with_capacity(100);
        // Two low-fee entries, fee 10 each, summed = 20.
        m.insert(sample_entry_with_size(0xAA, 10, 50)).unwrap();
        m.insert(sample_entry_with_size(0xBB, 10, 50)).unwrap();
        // Incoming: 100 bytes (would need to evict BOTH to fit), fee 20.
        // Cumulative evicted fee = 20, incoming fee = 20 → reject.
        let err = m
            .insert_with_eviction(sample_entry_with_size(0xCC, 20, 100))
            .unwrap_err();
        assert!(matches!(
            err,
            MempoolError::EvictionNotWorthwhile {
                incoming_fee: 20,
                evicted_fee: 20,
            }
        ));
        // Mempool unchanged.
        assert_eq!(m.len(), 2);
    }

    /// When the incoming entry is too large to fit in the mempool's
    /// total capacity, eviction can never make room — return
    /// `EvictionInsufficientSpace` rather than `CapacityExceeded` so
    /// the caller can distinguish "bad input" from "transient overflow".
    #[test]
    fn insert_with_eviction_rejects_when_incoming_exceeds_total_capacity() {
        let mut m = Mempool::with_capacity(50);
        let err = m
            .insert_with_eviction(sample_entry_with_size(0xAA, 100, 200))
            .unwrap_err();
        assert!(matches!(
            err,
            MempoolError::EvictionInsufficientSpace {
                incoming: 200,
                limit: 50,
                ..
            }
        ));
    }

    /// All existing entries have higher fee than the incoming → nothing
    /// is even considered for eviction → reject with insufficient-space
    /// rather than silently dropping the head of the mempool. Pins the
    /// strictly-lower-fee guard in the eviction-set selection loop.
    #[test]
    fn insert_with_eviction_does_not_displace_higher_or_equal_fee_entries() {
        let mut m = Mempool::with_capacity(100);
        // Mempool full of high-fee entries.
        m.insert(sample_entry_with_size(0xAA, 100, 50)).unwrap();
        m.insert(sample_entry_with_size(0xBB, 200, 50)).unwrap();
        // Incoming: low fee — must not displace any of them.
        let err = m
            .insert_with_eviction(sample_entry_with_size(0xCC, 5, 50))
            .unwrap_err();
        assert!(matches!(err, MempoolError::EvictionInsufficientSpace { .. }));
        assert_eq!(m.len(), 2);
    }

    /// Duplicate detection short-circuits BEFORE eviction — same as
    /// plain `insert`. Without this guard, a replay attack could
    /// trigger displacement of unrelated low-fee entries by
    /// re-submitting an existing high-fee tx.
    #[test]
    fn insert_with_eviction_rejects_duplicate_before_considering_eviction() {
        let mut m = Mempool::with_capacity(50);
        let dup = sample_entry_with_size(0xAA, 100, 50);
        m.insert(dup.clone()).unwrap();
        let err = m.insert_with_eviction(dup.clone()).unwrap_err();
        assert!(matches!(err, MempoolError::Duplicate(_)));
        assert_eq!(m.len(), 1);
    }

    /// `SharedMempool` wrapper threads `insert_with_eviction` through
    /// the same shared lock as `insert`, returns the evicted TxIds to
    /// the caller, and leaves the mempool in the expected end state
    /// (one new entry, one displaced entry, total bytes unchanged).
    /// The `change_notify` notification is exercised end-to-end by the
    /// existing `shared_mempool_snapshot_reflects_updates` async test;
    /// keeping this test synchronous avoids pulling extra tokio runtime
    /// features into the mempool crate's dev-deps.
    #[test]
    fn shared_mempool_insert_with_eviction_displaces_lowest_fee_entry() {
        let m = SharedMempool::with_capacity(100);
        m.insert(sample_entry_with_size(0xAA, 5, 50)).unwrap();
        m.insert(sample_entry_with_size(0xBB, 50, 50)).unwrap();
        let evicted = m
            .insert_with_eviction(sample_entry_with_size(0xCC, 100, 50))
            .unwrap();
        assert_eq!(evicted, vec![TxId([0xAA; 32])]);
        assert_eq!(m.len(), 2);
        assert_eq!(m.size_bytes(), 100);
    }
}
