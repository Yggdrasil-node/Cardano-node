//! Mempool queue, transaction entry type, and error types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side aggregation of upstream
//! `Ouroboros.Consensus.Mempool.{API, Capacity, Impl.Common,
//! Impl.Update, Init, Query, TxSeq, Update}` — the API,
//! fee-ordered queue data structure, capacity tracking, and entry
//! types are spread across those modules upstream. Yggdrasil
//! unifies them in `queue.rs` with sub-modules `inner.rs` (the
//! Mempool struct + impl) and `shared.rs` (the Arc<RwLock<...>>
//! wrapper for the runtime). The cross-peer dedup state lives in
//! the sibling `tx_state.rs`.

use std::collections::HashMap;

use yggdrasil_ledger::{Era, LedgerError, MultiEraSubmittedTx, ShelleyTxIn, SlotNo, TxId};

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

pub mod inner;
pub mod shared;

pub use inner::Mempool;
pub use shared::SharedMempool;

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
        assert!(matches!(
            err,
            MempoolError::EvictionInsufficientSpace { .. }
        ));
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
