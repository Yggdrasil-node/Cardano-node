//! Volatile chain state tracking with rollback depth enforcement.
//!
//! The `ChainState` tracks a sliding window of recent chain points and
//! enforces the Ouroboros security parameter `k` — the maximum number of
//! blocks that may be rolled back.  Points older than `k` from the tip
//! are considered *stable* and eligible for promotion to immutable storage.
//!
//! Reference: the upstream volatile DB and `SecurityParam` in
//! `Ouroboros.Consensus.Config.SecurityParam` and
//! `Ouroboros.Consensus.Storage.VolatileDB`.

use yggdrasil_ledger::{BlockNo, HeaderHash, Point, SlotNo};

use crate::error::ConsensusError;

/// The Ouroboros security parameter `k`, defining the maximum rollback
/// depth.  A chain suffix of at most `k` blocks is considered volatile;
/// any block further from the tip is stable (immutable).
///
/// Reference: `SecurityParam` in
/// `Ouroboros.Consensus.Config.SecurityParam`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityParam(pub u64);

/// An entry in the volatile chain state, recording a block's position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainEntry {
    /// The block's header hash.
    pub hash: HeaderHash,
    /// The slot in which the block was issued.
    pub slot: SlotNo,
    /// The block's height (block number).
    pub block_no: BlockNo,
    /// The previous block's header hash from this block's header.
    ///
    /// When `Some`, [`ChainState::roll_forward`] validates that this
    /// matches the current tip's header hash, enforcing the CHAINHEAD
    /// prev-hash invariant.  `None` is accepted for the first block
    /// (genesis) and for test convenience when prev-hash tracking
    /// is not needed.
    ///
    /// Reference: `blockPrevHash` in
    /// `Ouroboros.Consensus.Block.Abstract`.
    pub prev_hash: Option<HeaderHash>,
}

/// Tracks the volatile tip of the chain with rollback depth enforcement.
///
/// Maintains an ordered list of the most recent chain points.  The
/// maximum rollback depth is bounded by the security parameter `k`.
/// Entries beyond `k` from the tip are *stable* and can be flushed to
/// immutable storage via [`ChainState::drain_stable`].
///
/// Reference: the interaction between the volatile DB, the immutable DB,
/// and `SecurityParam` in `Ouroboros.Consensus.Storage.ChainDB`.
#[derive(Clone, Debug)]
pub struct ChainState {
    /// The security parameter (maximum rollback depth).
    k: SecurityParam,
    /// Ordered volatile chain entries, oldest first.
    entries: Vec<ChainEntry>,
}

impl ChainState {
    /// Create a new empty `ChainState` with the given security parameter.
    pub fn new(k: SecurityParam) -> Self {
        Self {
            k,
            entries: Vec::new(),
        }
    }

    /// The security parameter for this chain state.
    pub fn security_param(&self) -> SecurityParam {
        self.k
    }

    /// The current volatile tip, or `Point::Origin` if the chain is empty.
    pub fn tip(&self) -> Point {
        match self.entries.last() {
            Some(entry) => Point::BlockPoint(entry.slot, entry.hash),
            None => Point::Origin,
        }
    }

    /// The current block height at the tip, or `None` if the chain is empty.
    pub fn tip_block_no(&self) -> Option<BlockNo> {
        self.entries.last().map(|e| e.block_no)
    }

    /// The number of entries in the volatile window.
    pub fn volatile_len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the chain state is empty (at origin).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Roll forward by appending a new block to the chain tip.
    ///
    /// Enforces three CHAINHEAD invariants:
    /// 1. **Block number contiguous** — `entry.block_no == tip.block_no + 1`
    /// 2. **Slot strictly increasing** — `entry.slot > tip.slot`
    /// 3. **Prev-hash matches tip** — when `entry.prev_hash` is `Some`,
    ///    it must equal the current tip's header hash
    ///
    /// Reference: `tickChainDepState` and `chainSelectionForBlock` in
    /// `Ouroboros.Consensus`.
    pub fn roll_forward(&mut self, entry: ChainEntry) -> Result<(), ConsensusError> {
        if let Some(last) = self.entries.last() {
            if entry.block_no.0 != last.block_no.0 + 1 {
                return Err(ConsensusError::NonContiguousBlock {
                    expected: last.block_no.0 + 1,
                    got: entry.block_no.0,
                });
            }
            if entry.slot.0 <= last.slot.0 {
                return Err(ConsensusError::SlotNotIncreasing {
                    tip_slot: last.slot.0,
                    block_slot: entry.slot.0,
                });
            }
            if let Some(prev) = entry.prev_hash {
                if prev != last.hash {
                    return Err(ConsensusError::PrevHashMismatch {
                        expected: last.hash,
                        got: prev,
                    });
                }
            }
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Roll backward to the given point.
    ///
    /// All entries *after* the rollback target are removed.  Returns an
    /// error if the rollback would exceed `k` blocks or if the target
    /// point is not found in the volatile window (and is not `Origin`).
    ///
    /// Rolling back to `Origin` clears the entire volatile chain.
    pub fn roll_backward(&mut self, target: &Point) -> Result<(), ConsensusError> {
        match target {
            Point::Origin => {
                let depth = self.entries.len() as u64;
                if depth > self.k.0 {
                    return Err(ConsensusError::RollbackTooDeep {
                        requested: depth,
                        max: self.k.0,
                    });
                }
                self.entries.clear();
                Ok(())
            }
            Point::BlockPoint(slot, hash) => {
                let pos = self
                    .entries
                    .iter()
                    .rposition(|e| e.slot == *slot && e.hash == *hash);
                match pos {
                    Some(idx) => {
                        let depth = (self.entries.len() - 1 - idx) as u64;
                        if depth > self.k.0 {
                            return Err(ConsensusError::RollbackTooDeep {
                                requested: depth,
                                max: self.k.0,
                            });
                        }
                        self.entries.truncate(idx + 1);
                        Ok(())
                    }
                    None => Err(ConsensusError::RollbackPointNotFound {
                        slot: slot.0,
                        hash: *hash,
                    }),
                }
            }
        }
    }

    /// Returns the number of stable entries — those beyond `k` from the
    /// tip that are eligible for promotion to immutable storage.
    pub fn stable_count(&self) -> usize {
        self.entries.len().saturating_sub(self.k.0 as usize)
    }

    /// Drain stable entries from the front of the volatile window.
    ///
    /// Returns entries that are more than `k` blocks behind the current
    /// tip and removes them from the volatile chain.  After draining,
    /// `volatile_len() <= k`.
    pub fn drain_stable(&mut self) -> Vec<ChainEntry> {
        let n = self.stable_count();
        if n == 0 {
            return Vec::new();
        }
        self.entries.drain(..n).collect()
    }

    /// Returns a reference to the current volatile entries (oldest first).
    pub fn volatile_entries(&self) -> &[ChainEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_entry(block_no: u64, slot: u64) -> ChainEntry {
        let mut hash = [0u8; 32];
        hash[0] = block_no as u8;
        hash[1] = slot as u8;
        ChainEntry {
            hash: HeaderHash(hash),
            slot: SlotNo(slot),
            block_no: BlockNo(block_no),
            prev_hash: None,
        }
    }

    // ── Constructor / accessors ──────────────────────────────────────

    #[test]
    fn new_chain_state_is_empty() {
        let cs = ChainState::new(SecurityParam(10));
        assert!(cs.is_empty());
        assert_eq!(cs.volatile_len(), 0);
        assert_eq!(cs.tip(), Point::Origin);
        assert_eq!(cs.tip_block_no(), None);
        assert_eq!(cs.security_param(), SecurityParam(10));
    }

    // ── roll_forward ─────────────────────────────────────────────────

    #[test]
    fn roll_forward_first_block_any_block_no() {
        let mut cs = ChainState::new(SecurityParam(5));
        // First block can be any block_no (no predecessor check).
        let entry = mk_entry(42, 100);
        cs.roll_forward(entry.clone()).unwrap();
        assert_eq!(cs.volatile_len(), 1);
        assert_eq!(cs.tip(), Point::BlockPoint(SlotNo(100), entry.hash));
        assert_eq!(cs.tip_block_no(), Some(BlockNo(42)));
    }

    #[test]
    fn roll_forward_contiguous_blocks() {
        let mut cs = ChainState::new(SecurityParam(10));
        for i in 0..5 {
            cs.roll_forward(mk_entry(i, i * 20)).unwrap();
        }
        assert_eq!(cs.volatile_len(), 5);
        assert_eq!(cs.tip_block_no(), Some(BlockNo(4)));
    }

    #[test]
    fn roll_forward_rejects_non_contiguous() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(0, 0)).unwrap();
        let err = cs.roll_forward(mk_entry(5, 20)).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::NonContiguousBlock {
                expected: 1,
                got: 5,
            }
        );
    }

    #[test]
    fn roll_forward_rejects_same_block_no() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(3, 10)).unwrap();
        let err = cs.roll_forward(mk_entry(3, 20)).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::NonContiguousBlock {
                expected: 4,
                got: 3,
            }
        );
    }

    #[test]
    fn roll_forward_rejects_non_increasing_slot() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(0, 10)).unwrap();
        // same slot
        let err = cs.roll_forward(mk_entry(1, 10)).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::SlotNotIncreasing {
                tip_slot: 10,
                block_slot: 10,
            }
        );
    }

    #[test]
    fn roll_forward_rejects_decreasing_slot() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(0, 20)).unwrap();
        let err = cs.roll_forward(mk_entry(1, 15)).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::SlotNotIncreasing {
                tip_slot: 20,
                block_slot: 15,
            }
        );
    }

    #[test]
    fn roll_forward_validates_prev_hash_when_present() {
        let mut cs = ChainState::new(SecurityParam(10));
        let first = mk_entry(0, 0);
        let first_hash = first.hash;
        cs.roll_forward(first).unwrap();

        // Correct prev_hash.
        let mut second = mk_entry(1, 10);
        second.prev_hash = Some(first_hash);
        cs.roll_forward(second).unwrap();
        assert_eq!(cs.volatile_len(), 2);
    }

    #[test]
    fn roll_forward_rejects_wrong_prev_hash() {
        let mut cs = ChainState::new(SecurityParam(10));
        let first = mk_entry(0, 0);
        cs.roll_forward(first).unwrap();

        let mut second = mk_entry(1, 10);
        second.prev_hash = Some(HeaderHash([0xFF; 32])); // wrong
        let err = cs.roll_forward(second).unwrap_err();
        assert!(matches!(err, ConsensusError::PrevHashMismatch { .. }));
    }

    #[test]
    fn roll_forward_skips_prev_hash_check_when_none() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(0, 0)).unwrap();
        // prev_hash = None → no check
        let second = mk_entry(1, 10);
        assert!(second.prev_hash.is_none());
        cs.roll_forward(second).unwrap();
        assert_eq!(cs.volatile_len(), 2);
    }

    // ── roll_backward ────────────────────────────────────────────────

    #[test]
    fn roll_backward_to_origin_clears_chain() {
        let mut cs = ChainState::new(SecurityParam(10));
        for i in 0..3 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        cs.roll_backward(&Point::Origin).unwrap();
        assert!(cs.is_empty());
        assert_eq!(cs.tip(), Point::Origin);
    }

    #[test]
    fn roll_backward_to_origin_exceeds_k() {
        let mut cs = ChainState::new(SecurityParam(2));
        for i in 0..5 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        let err = cs.roll_backward(&Point::Origin).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::RollbackTooDeep {
                requested: 5,
                max: 2,
            }
        );
    }

    #[test]
    fn roll_backward_to_block_point() {
        let mut cs = ChainState::new(SecurityParam(10));
        let entries: Vec<_> = (0..5).map(|i| mk_entry(i, i * 10)).collect();
        for e in &entries {
            cs.roll_forward(e.clone()).unwrap();
        }
        // Roll back to entry 2
        let target = Point::BlockPoint(entries[2].slot, entries[2].hash);
        cs.roll_backward(&target).unwrap();
        assert_eq!(cs.volatile_len(), 3);
        assert_eq!(cs.tip_block_no(), Some(BlockNo(2)));
    }

    #[test]
    fn roll_backward_to_tip_is_noop() {
        let mut cs = ChainState::new(SecurityParam(5));
        for i in 0..3 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        let e2 = mk_entry(2, 20);
        let target = Point::BlockPoint(e2.slot, e2.hash);
        cs.roll_backward(&target).unwrap();
        assert_eq!(cs.volatile_len(), 3);
    }

    #[test]
    fn roll_backward_exceeds_k() {
        let mut cs = ChainState::new(SecurityParam(2));
        let entries: Vec<_> = (0..5).map(|i| mk_entry(i, i * 10)).collect();
        for e in &entries {
            cs.roll_forward(e.clone()).unwrap();
        }
        // Rolling back to entry 0 = depth 4, but k=2
        let target = Point::BlockPoint(entries[0].slot, entries[0].hash);
        let err = cs.roll_backward(&target).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::RollbackTooDeep {
                requested: 4,
                max: 2,
            }
        );
    }

    #[test]
    fn roll_backward_point_not_found() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(0, 0)).unwrap();
        let bogus_hash = HeaderHash([0xFF; 32]);
        let target = Point::BlockPoint(SlotNo(999), bogus_hash);
        let err = cs.roll_backward(&target).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::RollbackPointNotFound {
                slot: 999,
                hash: bogus_hash,
            }
        );
    }

    // ── stable_count / drain_stable ──────────────────────────────────

    #[test]
    fn stable_count_zero_when_under_k() {
        let mut cs = ChainState::new(SecurityParam(5));
        for i in 0..3 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        assert_eq!(cs.stable_count(), 0);
    }

    #[test]
    fn stable_count_positive_when_over_k() {
        let mut cs = ChainState::new(SecurityParam(3));
        for i in 0..7 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        // 7 entries, k=3 → 4 stable
        assert_eq!(cs.stable_count(), 4);
    }

    #[test]
    fn drain_stable_removes_oldest() {
        let mut cs = ChainState::new(SecurityParam(2));
        for i in 0..5 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        let stable = cs.drain_stable();
        assert_eq!(stable.len(), 3);
        assert_eq!(stable[0].block_no, BlockNo(0));
        assert_eq!(stable[1].block_no, BlockNo(1));
        assert_eq!(stable[2].block_no, BlockNo(2));
        assert_eq!(cs.volatile_len(), 2);
        assert_eq!(cs.tip_block_no(), Some(BlockNo(4)));
    }

    #[test]
    fn drain_stable_empty_when_nothing_stable() {
        let mut cs = ChainState::new(SecurityParam(10));
        cs.roll_forward(mk_entry(0, 0)).unwrap();
        let stable = cs.drain_stable();
        assert!(stable.is_empty());
        assert_eq!(cs.volatile_len(), 1);
    }

    #[test]
    fn drain_stable_repeated_drains() {
        let mut cs = ChainState::new(SecurityParam(1));
        for i in 0..4 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        let s1 = cs.drain_stable();
        assert_eq!(s1.len(), 3);
        assert_eq!(cs.volatile_len(), 1);
        // Add more blocks
        cs.roll_forward(mk_entry(4, 40)).unwrap();
        cs.roll_forward(mk_entry(5, 50)).unwrap();
        let s2 = cs.drain_stable();
        assert_eq!(s2.len(), 2);
        assert_eq!(cs.volatile_len(), 1);
    }

    // ── volatile_entries ─────────────────────────────────────────────

    #[test]
    fn volatile_entries_ordered_oldest_first() {
        let mut cs = ChainState::new(SecurityParam(10));
        for i in 0..3 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        let entries = cs.volatile_entries();
        assert_eq!(entries[0].block_no, BlockNo(0));
        assert_eq!(entries[1].block_no, BlockNo(1));
        assert_eq!(entries[2].block_no, BlockNo(2));
    }

    // ── roll_forward after rollback ──────────────────────────────────

    #[test]
    fn roll_forward_after_rollback() {
        let mut cs = ChainState::new(SecurityParam(10));
        let entries: Vec<_> = (0..5).map(|i| mk_entry(i, i * 10)).collect();
        for e in &entries {
            cs.roll_forward(e.clone()).unwrap();
        }
        let target = Point::BlockPoint(entries[2].slot, entries[2].hash);
        cs.roll_backward(&target).unwrap();
        // Now tip is block_no=2, next must be 3
        let new_entry = mk_entry(3, 35);
        cs.roll_forward(new_entry).unwrap();
        assert_eq!(cs.volatile_len(), 4);
        assert_eq!(cs.tip_block_no(), Some(BlockNo(3)));
    }

    // ── edge: k=0 ───────────────────────────────────────────────────

    #[test]
    fn k_zero_every_block_is_stable() {
        let mut cs = ChainState::new(SecurityParam(0));
        for i in 0..3 {
            cs.roll_forward(mk_entry(i, i * 10)).unwrap();
        }
        assert_eq!(cs.stable_count(), 3);
        let stable = cs.drain_stable();
        assert_eq!(stable.len(), 3);
        assert_eq!(cs.volatile_len(), 0);
    }

    #[test]
    fn k_zero_rollback_to_origin_fails() {
        let mut cs = ChainState::new(SecurityParam(0));
        cs.roll_forward(mk_entry(0, 0)).unwrap();
        let err = cs.roll_backward(&Point::Origin).unwrap_err();
        assert_eq!(
            err,
            ConsensusError::RollbackTooDeep {
                requested: 1,
                max: 0,
            }
        );
    }
}
