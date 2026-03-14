use yggdrasil_ledger::{Block, CborDecode, CborEncode, LedgerStateCheckpoint, Point, SlotNo};

use crate::{ImmutableStore, LedgerStore, StorageError, VolatileStore};

/// Recovery metadata derived from the coordinated storage state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainDbRecovery {
    /// Best known chain point across immutable and volatile storage.
    pub tip: Point,
    /// Latest ledger snapshot at or before the current best known point.
    pub ledger_snapshot_slot: Option<SlotNo>,
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

    /// Rolls back the volatile suffix and truncates ledger snapshots newer
    /// than the rollback point.
    pub fn rollback_to(&mut self, point: &Point) -> Result<(), StorageError> {
        self.volatile.rollback_to(point);
        match point {
            Point::Origin => self.ledger.truncate_after(None),
            Point::BlockPoint(slot, _) => self.ledger.truncate_after(Some(*slot)),
        }
    }

    /// Promotes the volatile prefix through `point` into immutable storage and
    /// prunes the promoted prefix from the volatile store.
    pub fn promote_volatile_prefix(&mut self, point: &Point) -> Result<usize, StorageError> {
        let blocks = self.volatile.prefix_up_to(point)?;
        for block in &blocks {
            self.immutable.append_block(block.clone())?;
        }
        self.volatile.prune_up_to(point)?;
        Ok(blocks.len())
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