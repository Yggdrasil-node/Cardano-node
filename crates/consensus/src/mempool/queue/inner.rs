//! Fee-ordered mempool queue policy.
//!
//! Mirrors upstream `Ouroboros.Consensus.Mempool.API::Mempool` and the
//! `Impl.Update` insert / remove / purge logic.
//!
//! Single public type:
//!
//! - `Mempool` — fee-ordered queue with capacity tracking and duplicate
//!   detection. Entries sorted by descending fee so `pop_best` returns
//!   the highest-fee transaction first.
//!
//! Extracted from `mempool/queue.rs` in R273d (Phase γ §R273 fourth slice).

use std::collections::{HashMap, HashSet};

use yggdrasil_ledger::{
    LedgerError, ProtocolParameters, ShelleyTxIn, SlotNo, TxId, validate_fee, validate_tx_ex_units,
    validate_tx_size,
};

use super::{
    IndexedMempoolEntry, MempoolEntry, MempoolError, MempoolIdx, MempoolSnapshot,
    TxSubmissionMempoolReader,
};

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
    pub(super) max_bytes: usize,
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
        let next_bytes = self.current_bytes.checked_add(entry.size_bytes).ok_or(
            MempoolError::CapacityExceeded {
                current: self.current_bytes,
                incoming: entry.size_bytes,
                limit: self.max_bytes,
            },
        )?;
        if self.max_bytes > 0 && next_bytes > self.max_bytes {
            return Err(MempoolError::CapacityExceeded {
                current: self.current_bytes,
                incoming: entry.size_bytes,
                limit: self.max_bytes,
            });
        }
        let tx_id = entry.tx_id;
        self.current_bytes = next_bytes;
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
        // Descending fee: negate via Reverse-style key (`u64::MAX - fee`
        // would alias) — use sort_by_key on `Reverse(fee)`.
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.entry.fee));
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
    pub fn insert_with_eviction(&mut self, entry: MempoolEntry) -> Result<Vec<TxId>, MempoolError> {
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
        let projected = self.current_bytes.checked_add(entry.size_bytes).ok_or(
            MempoolError::CapacityExceeded {
                current: self.current_bytes,
                incoming: entry.size_bytes,
                limit: self.max_bytes,
            },
        )?;
        if self.max_bytes == 0 || projected <= self.max_bytes {
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
