//! dmq-node in-memory signature mempool.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of upstream
//! `Ouroboros.Network.TxSubmission.Mempool.Simple` — the simple
//! in-memory mempool the DMQ `NodeKernel`
//! (`Diffusion/NodeKernel.hs`, field
//! `mempool :: Mempool m SigId (Sig crypto)`) holds for diffused
//! signatures. dmq-node carries its own copy (the R732
//! dmq-node-local decision — the core `crates/consensus` mempool is
//! concrete over ledger transactions).
//!
//! This module ports the pure [`MempoolSeq`] data structure. The
//! upstream `Mempool` newtype wraps it in a `StrictTVar` for
//! concurrent access, and `getReader` / `getWriter` expose the
//! `TxSubmissionMempoolReader` / `Writer` STM interfaces — that STM
//! shell lands with the dmq-node runtime sub-arc.

use std::collections::BTreeSet;

/// A mempool entry paired with its monotonic insertion index.
///
/// Mirror of upstream `data WithIndex tx`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithIndex<T> {
    /// The entry's insertion index.
    pub idx: i64,
    /// The entry itself.
    pub tx: T,
}

/// The in-memory mempool data structure — a membership set plus an
/// index-ordered sequence of entries.
///
/// Mirror of upstream `data MempoolSeq txid tx`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MempoolSeq<Id: Ord, Tx> {
    /// Cached set of identifiers currently in the mempool.
    mempool_set: BTreeSet<Id>,
    /// All entries, in insertion order, each tagged with its index.
    mempool_seq: Vec<WithIndex<Tx>>,
    /// The next available index — invariant: greater than the index
    /// of the last element of `mempool_seq`.
    next_idx: i64,
}

impl<Id: Ord, Tx> MempoolSeq<Id, Tx> {
    /// An empty mempool. Mirror of upstream `empty` (`next_idx`
    /// starts at `-1` — upstream's `mempoolZeroIdx`, so a snapshot
    /// query "after index -1" returns every entry).
    pub fn empty() -> MempoolSeq<Id, Tx> {
        MempoolSeq {
            mempool_set: BTreeSet::new(),
            mempool_seq: Vec::new(),
            next_idx: -1,
        }
    }

    /// Build a mempool from a list of entries, indexed from `0`.
    ///
    /// Mirror of upstream `new` — `next_idx` becomes the entry count.
    pub fn new(get_id: impl Fn(&Tx) -> Id, txs: Vec<Tx>) -> MempoolSeq<Id, Tx> {
        let mempool_set: BTreeSet<Id> = txs.iter().map(&get_id).collect();
        let count = txs.len() as i64;
        let mempool_seq = txs
            .into_iter()
            .enumerate()
            .map(|(i, tx)| WithIndex { idx: i as i64, tx })
            .collect();
        MempoolSeq {
            mempool_set,
            mempool_seq,
            next_idx: count,
        }
    }

    /// The next index this mempool would assign.
    pub fn next_idx(&self) -> i64 {
        self.next_idx
    }

    /// All entries, in insertion order. Mirror of upstream `read`.
    pub fn read(&self) -> impl Iterator<Item = &Tx> {
        self.mempool_seq.iter().map(|w| &w.tx)
    }

    /// Whether an identifier is in the mempool. Mirror of upstream
    /// `mempoolHasTx`.
    pub fn has_tx(&self, id: &Id) -> bool {
        self.mempool_set.contains(id)
    }

    /// The entry at a given index, if present. Mirror of upstream
    /// `mempoolLookupTx`.
    pub fn lookup_tx(&self, idx: i64) -> Option<&Tx> {
        self.mempool_seq
            .iter()
            .find(|w| w.idx == idx)
            .map(|w| &w.tx)
    }

    /// The entries with an index strictly greater than `idx`. Mirror
    /// of upstream `mempoolTxIdsAfter`.
    pub fn tx_ids_after(&self, idx: i64) -> impl Iterator<Item = &WithIndex<Tx>> {
        self.mempool_seq.iter().filter(move |w| w.idx > idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mempool_has_zero_idx_sentinel() {
        let m: MempoolSeq<String, String> = MempoolSeq::empty();
        assert_eq!(m.next_idx(), -1);
        assert_eq!(m.read().count(), 0);
        assert!(!m.has_tx(&"x".to_string()));
    }

    #[test]
    fn new_indexes_entries_from_zero() {
        let m = MempoolSeq::new(
            String::clone,
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(m.next_idx(), 3);
        let entries: Vec<&String> = m.read().collect();
        assert_eq!(entries, vec!["a", "b", "c"]);
        assert!(m.has_tx(&"b".to_string()));
        assert!(!m.has_tx(&"z".to_string()));
    }

    #[test]
    fn lookup_tx_finds_by_index() {
        let m = MempoolSeq::new(String::clone, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(m.lookup_tx(0), Some(&"a".to_string()));
        assert_eq!(m.lookup_tx(1), Some(&"b".to_string()));
        assert_eq!(m.lookup_tx(2), None);
    }

    #[test]
    fn tx_ids_after_returns_strictly_greater_indices() {
        let m = MempoolSeq::new(
            String::clone,
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        // After the -1 sentinel: every entry.
        assert_eq!(m.tx_ids_after(-1).count(), 3);
        // After index 0: entries 1 and 2.
        let after0: Vec<i64> = m.tx_ids_after(0).map(|w| w.idx).collect();
        assert_eq!(after0, vec![1, 2]);
        assert_eq!(m.tx_ids_after(2).count(), 0);
    }
}
