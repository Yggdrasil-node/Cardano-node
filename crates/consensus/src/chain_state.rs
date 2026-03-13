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
    /// Returns an error if the block number does not strictly follow the
    /// current tip's block number.
    pub fn roll_forward(&mut self, entry: ChainEntry) -> Result<(), ConsensusError> {
        if let Some(last) = self.entries.last() {
            if entry.block_no.0 != last.block_no.0 + 1 {
                return Err(ConsensusError::NonContiguousBlock {
                    expected: last.block_no.0 + 1,
                    got: entry.block_no.0,
                });
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
