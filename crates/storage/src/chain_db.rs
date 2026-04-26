use yggdrasil_ledger::{
    Block, CborDecode, CborEncode, LedgerState, LedgerStateCheckpoint, Point, SlotNo,
};

use crate::{ImmutableStore, LedgerStore, StorageError, VolatileStore};

/// Recovery metadata derived from the coordinated storage state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainDbRecovery {
    /// Best known chain point across immutable and volatile storage.
    pub tip: Point,
    /// Latest ledger snapshot at or before the current best known point.
    pub ledger_snapshot_slot: Option<SlotNo>,
}

/// Result of rebuilding typed ledger state from coordinated storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerRecoveryOutcome {
    /// Restored ledger state after replaying immutable and volatile suffixes.
    pub ledger_state: LedgerState,
    /// Point that the restored ledger state has reached.
    pub point: Point,
    /// Slot of the checkpoint used for recovery, if one was available.
    pub checkpoint_slot: Option<SlotNo>,
    /// Number of volatile blocks replayed after the checkpoint.
    pub replayed_volatile_blocks: usize,
}

/// Snapshot-retention metadata after persisting a typed ledger checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerCheckpointRetention {
    /// Number of checkpoints retained after pruning.
    pub retained_snapshots: usize,
    /// Number of checkpoints pruned by the retention pass.
    pub pruned_snapshots: usize,
}

fn point_for_block(block: &Block) -> Point {
    Point::BlockPoint(block.header.slot_no, block.header.hash)
}

fn volatile_suffix_after<V: VolatileStore>(
    volatile: &V,
    replay_from_exclusive: &Point,
) -> Result<Vec<Block>, StorageError> {
    let volatile_tip = volatile.tip();
    if volatile_tip == Point::Origin {
        return Ok(Vec::new());
    }

    let mut blocks = volatile.prefix_up_to(&volatile_tip)?;
    if *replay_from_exclusive == Point::Origin {
        return Ok(blocks);
    }

    if let Some(pos) = blocks
        .iter()
        .position(|block| point_for_block(block) == *replay_from_exclusive)
    {
        Ok(blocks.split_off(pos + 1))
    } else {
        Ok(blocks)
    }
}

/// Minimal ChainDB-style coordinator for immutable, volatile, and ledger
/// snapshot storage.
///
/// This type intentionally stays below consensus and node orchestration. It
/// owns only storage coordination concerns: choosing a best-known tip,
/// promoting stable volatile prefixes into immutable storage, and pruning
/// ledger snapshots after rollback.
#[derive(Clone, Debug)]
pub struct ChainDb<I, V, L> {
    immutable: I,
    volatile: V,
    ledger: L,
}

impl<I, V, L> ChainDb<I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    /// Builds a new coordinated storage view from its component stores.
    pub fn new(immutable: I, volatile: V, ledger: L) -> Self {
        Self {
            immutable,
            volatile,
            ledger,
        }
    }

    /// Returns the best known chain tip, preferring the volatile suffix when
    /// present and otherwise falling back to the immutable tip.
    pub fn tip(&self) -> Point {
        let volatile_tip = self.volatile.tip();
        if volatile_tip == Point::Origin {
            self.immutable.get_tip()
        } else {
            volatile_tip
        }
    }

    /// Returns recovery-facing metadata derived from the coordinated stores.
    pub fn recovery(&self) -> ChainDbRecovery {
        let tip = self.tip();
        let ledger_snapshot_slot = match tip {
            Point::Origin => self.ledger.latest_snapshot().map(|(slot, _)| slot),
            Point::BlockPoint(slot, _) => self
                .ledger
                .latest_snapshot_before_or_at(slot)
                .map(|(snapshot_slot, _)| snapshot_slot),
        };

        ChainDbRecovery {
            tip,
            ledger_snapshot_slot,
        }
    }

    /// Adds a block to the volatile suffix.
    pub fn add_volatile_block(&mut self, block: Block) -> Result<(), StorageError> {
        self.volatile.add_block(block)
    }

    /// Saves a ledger snapshot at `slot`.
    pub fn save_ledger_snapshot(
        &mut self,
        slot: SlotNo,
        data: Vec<u8>,
    ) -> Result<(), StorageError> {
        self.ledger.save_snapshot(slot, data)
    }

    /// Saves a typed ledger-state checkpoint at `slot` using the ledger
    /// crate's deterministic CBOR encoding.
    pub fn save_ledger_checkpoint(
        &mut self,
        slot: SlotNo,
        checkpoint: &LedgerStateCheckpoint,
    ) -> Result<(), StorageError> {
        self.save_ledger_snapshot(slot, checkpoint.to_cbor_bytes())
    }

    /// Retains only the newest `max_snapshots` typed ledger checkpoints.
    pub fn retain_latest_ledger_checkpoints(
        &mut self,
        max_snapshots: usize,
    ) -> Result<(), StorageError> {
        self.ledger.retain_latest(max_snapshots)
    }

    /// Loads the latest typed ledger-state checkpoint, if present.
    pub fn latest_ledger_checkpoint(
        &self,
    ) -> Result<Option<(SlotNo, LedgerStateCheckpoint)>, StorageError> {
        self.ledger
            .latest_snapshot()
            .map(|(slot, data)| {
                LedgerStateCheckpoint::from_cbor_bytes(data)
                    .map(|checkpoint| (slot, checkpoint))
                    .map_err(|error| StorageError::Serialization(error.to_string()))
            })
            .transpose()
    }

    /// Loads the latest typed ledger-state checkpoint at or before `slot`, if
    /// present.
    pub fn latest_ledger_checkpoint_before_or_at(
        &self,
        slot: SlotNo,
    ) -> Result<Option<(SlotNo, LedgerStateCheckpoint)>, StorageError> {
        self.ledger
            .latest_snapshot_before_or_at(slot)
            .map(|(snapshot_slot, data)| {
                LedgerStateCheckpoint::from_cbor_bytes(data)
                    .map(|checkpoint| (snapshot_slot, checkpoint))
                    .map_err(|error| StorageError::Serialization(error.to_string()))
            })
            .transpose()
    }

    /// Clears all stored ledger checkpoints.
    pub fn clear_ledger_checkpoints(&mut self) -> Result<(), StorageError> {
        self.ledger.truncate_after(None)
    }

    /// Truncates stored ledger checkpoints newer than `point`.
    pub fn truncate_ledger_checkpoints_after_point(
        &mut self,
        point: &Point,
    ) -> Result<(), StorageError> {
        match point {
            Point::Origin => self.ledger.truncate_after(None),
            Point::BlockPoint(slot, _) => self.ledger.truncate_after(Some(*slot)),
        }
    }

    /// Saves a typed ledger checkpoint at `point` and retains only the newest
    /// `max_snapshots` snapshots.
    pub fn persist_ledger_checkpoint(
        &mut self,
        point: &Point,
        checkpoint: &LedgerStateCheckpoint,
        max_snapshots: usize,
    ) -> Result<LedgerCheckpointRetention, StorageError> {
        match point {
            Point::Origin => {
                self.clear_ledger_checkpoints()?;
                Ok(LedgerCheckpointRetention {
                    retained_snapshots: 0,
                    pruned_snapshots: 0,
                })
            }
            Point::BlockPoint(slot, _) => {
                self.save_ledger_checkpoint(*slot, checkpoint)?;
                let after_save = self.ledger.count();
                self.retain_latest_ledger_checkpoints(max_snapshots)?;
                let after_retain = self.ledger.count();
                Ok(LedgerCheckpointRetention {
                    retained_snapshots: after_retain,
                    pruned_snapshots: after_save.saturating_sub(after_retain),
                })
            }
        }
    }

    /// Restores ledger state from the latest typed checkpoint and replays any
    /// remaining immutable and volatile suffix.
    pub fn recover_ledger_state(
        &self,
        base_state: LedgerState,
    ) -> Result<LedgerRecoveryOutcome, StorageError> {
        let best_tip = self.tip();

        // Attempt checkpoint recovery with fallback to progressively older
        // snapshots when the latest checkpoint is corrupt or undecodable.
        let max_slot = match best_tip {
            Point::Origin => None,
            Point::BlockPoint(slot, _) => Some(slot),
        };
        let (mut ledger_state, checkpoint_slot, replay_from_exclusive) =
            self.try_restore_checkpoint(base_state, max_slot)?;

        let immutable_replay_blocks = self.immutable.suffix_after(&replay_from_exclusive)?;
        for block in &immutable_replay_blocks {
            ledger_state
                .apply_block(block)
                .map_err(|error| StorageError::Recovery(error.to_string()))?;
        }

        let replay_anchor = immutable_replay_blocks
            .last()
            .map(point_for_block)
            .unwrap_or(replay_from_exclusive);
        let replay_blocks = volatile_suffix_after(&self.volatile, &replay_anchor)?;
        for block in &replay_blocks {
            ledger_state
                .apply_block(block)
                .map_err(|error| StorageError::Recovery(error.to_string()))?;
        }

        let point = ledger_state.tip;
        if point != best_tip {
            return Err(StorageError::Recovery(format!(
                "recovered ledger tip {point:?} does not match coordinated storage tip {best_tip:?}"
            )));
        }

        Ok(LedgerRecoveryOutcome {
            ledger_state,
            point,
            checkpoint_slot,
            replayed_volatile_blocks: replay_blocks.len(),
        })
    }

    /// Attempts to restore a typed ledger checkpoint, falling back to
    /// progressively older snapshots when the newest one cannot be decoded.
    ///
    /// Returns `(ledger_state, checkpoint_slot, replay_from_exclusive)` on
    /// success, or falls through to the `base_state` when no decodable
    /// checkpoint exists.
    fn try_restore_checkpoint(
        &self,
        base_state: LedgerState,
        max_slot: Option<SlotNo>,
    ) -> Result<(LedgerState, Option<SlotNo>, Point), StorageError> {
        let mut cursor = max_slot;
        loop {
            let raw = match cursor {
                Some(slot) => self.ledger.latest_snapshot_before_or_at(slot),
                None => self.ledger.latest_snapshot(),
            };
            let Some((snapshot_slot, data)) = raw else {
                // No (more) snapshots — fall through to base state.
                let point = base_state.tip;
                return Ok((base_state, None, point));
            };

            match LedgerStateCheckpoint::from_cbor_bytes(data) {
                Ok(checkpoint) => {
                    let state = checkpoint.restore();
                    let point = state.tip;
                    return Ok((state, Some(snapshot_slot), point));
                }
                Err(_err) => {
                    // Snapshot is corrupt; try the next-oldest one.
                    if snapshot_slot.0 == 0 {
                        let point = base_state.tip;
                        return Ok((base_state, None, point));
                    }
                    cursor = Some(SlotNo(snapshot_slot.0 - 1));
                }
            }
        }
    }

    /// Rolls back the volatile suffix and truncates ledger snapshots newer
    /// than the rollback point.
    pub fn rollback_to(&mut self, point: &Point) -> Result<(), StorageError> {
        match point {
            Point::Origin => self.volatile.rollback_to(point),
            Point::BlockPoint(_, hash) => {
                if self.volatile.get_block(hash).is_some() {
                    self.volatile.rollback_to(point);
                } else if self.immutable.get_block(hash).is_some() {
                    // When the rollback target is already immutable, the
                    // volatile suffix must be dropped entirely so the best
                    // known tip realigns with the immutable chain.
                    self.volatile.rollback_to(&Point::Origin);
                } else {
                    return Err(StorageError::PointNotFound);
                }
            }
        }

        match point {
            Point::Origin => self.ledger.truncate_after(None),
            Point::BlockPoint(slot, _) => self.ledger.truncate_after(Some(*slot)),
        }
    }

    /// Promotes the volatile prefix through `point` into immutable storage
    /// and prunes the promoted prefix from the volatile store.
    ///
    /// The promotion is idempotent under partial-completion crashes: each
    /// per-block append is skipped when the immutable store already
    /// contains a block with the same header hash. Without this, a crash
    /// between two `append_block` calls — or between the final append and
    /// `prune_up_to` — would leave overlap between the two stores, and the
    /// next promotion attempt would fail with
    /// [`StorageError::DuplicateBlock`] from the very first overlapping
    /// block, blocking all subsequent sync until manual cleanup.
    ///
    /// The append-then-prune ordering is preserved on purpose: it keeps
    /// every block present in at least one store at all times across the
    /// crash window, so recovery can always reach the chain tip via
    /// immutable + volatile suffix replay even if the process is killed
    /// mid-promotion.
    ///
    /// Returns the total number of volatile blocks that were on the
    /// promotion path (including any skipped because they were already in
    /// immutable).
    ///
    /// Reference: `Ouroboros.Consensus.Storage.ChainDB.Impl` —
    /// `copyToImmutableDB` runs as an idempotent operation on restart so a
    /// previously-interrupted copy resumes cleanly.
    pub fn promote_volatile_prefix(&mut self, point: &Point) -> Result<usize, StorageError> {
        let blocks = self.volatile.prefix_up_to(point)?;
        for block in &blocks {
            if self.immutable.contains_block(&block.header.hash) {
                // Already promoted by a previous (possibly crashed) run.
                continue;
            }
            self.immutable.append_block(block.clone())?;
        }
        self.volatile.prune_up_to(point)?;
        Ok(blocks.len())
    }

    /// Garbage-collects immutable blocks with slots strictly before `slot`.
    ///
    /// This is the coordinated counterpart to `ImmutableStore::trim_before_slot`.
    /// The caller is responsible for choosing a slot that preserves enough
    /// history for ledger replay (typically the oldest retained ledger
    /// checkpoint slot).
    ///
    /// Returns the number of blocks removed.
    pub fn gc_immutable_before_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError> {
        self.immutable.trim_before_slot(slot)
    }

    /// Garbage-collects volatile blocks with slots strictly before `slot`.
    ///
    /// This is the coordinated counterpart to `VolatileStore::garbage_collect`
    /// and corresponds to the upstream `garbageCollect` function invoked
    /// after stable blocks have been promoted to immutable storage.
    ///
    /// Returns the number of blocks removed.
    pub fn gc_volatile_before_slot(&mut self, slot: SlotNo) -> usize {
        self.volatile.garbage_collect(slot)
    }

    /// Borrows the immutable store.
    pub fn immutable(&self) -> &I {
        &self.immutable
    }

    /// Mutably borrows the immutable store.
    pub fn immutable_mut(&mut self) -> &mut I {
        &mut self.immutable
    }

    /// Borrows the volatile store.
    pub fn volatile(&self) -> &V {
        &self.volatile
    }

    /// Mutably borrows the volatile store.
    pub fn volatile_mut(&mut self) -> &mut V {
        &mut self.volatile
    }

    /// Borrows the ledger snapshot store.
    pub fn ledger(&self) -> &L {
        &self.ledger
    }

    /// Mutably borrows the ledger snapshot store.
    pub fn ledger_mut(&mut self) -> &mut L {
        &mut self.ledger
    }

    /// Decomposes the coordinator back into its component stores.
    pub fn into_inner(self) -> (I, V, L) {
        (self.immutable, self.volatile, self.ledger)
    }
}
