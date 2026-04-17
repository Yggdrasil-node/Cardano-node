use yggdrasil_ledger::SlotNo;

use crate::error::StorageError;

/// Persistent store for ledger state snapshots.
///
/// Snapshots allow the node to resume from a recent ledger state without
/// replaying the entire chain. Each snapshot is tagged with the slot at
/// which it was taken.
///
/// Reference: snapshot handling in `Ouroboros.Consensus.Storage.LedgerDB`.
pub trait LedgerStore {
    /// Persists a serialized ledger snapshot taken at the given slot.
    fn save_snapshot(&mut self, slot: SlotNo, data: Vec<u8>) -> Result<(), StorageError>;

    /// Returns the most recently stored snapshot (slot + payload), or `None`
    /// if no snapshot has been taken.
    fn latest_snapshot(&self) -> Option<(SlotNo, &[u8])>;

    /// Returns the most recent snapshot at or before `slot`.
    fn latest_snapshot_before_or_at(&self, slot: SlotNo) -> Option<(SlotNo, &[u8])>;

    /// Deletes snapshots newer than `slot`.
    ///
    /// Passing `None` clears all snapshots.
    fn truncate_after(&mut self, slot: Option<SlotNo>) -> Result<(), StorageError>;

    /// Retains only the newest `max_snapshots` snapshots.
    /// Passing `0` clears all snapshots.
    fn retain_latest(&mut self, max_snapshots: usize) -> Result<(), StorageError>;

    /// Returns the total number of stored snapshots.
    fn count(&self) -> usize;
}

/// In-memory ledger snapshot store for tests and interface stabilization.
#[derive(Clone, Debug, Default)]
pub struct InMemoryLedgerStore {
    snapshots: Vec<(SlotNo, Vec<u8>)>,
}

impl LedgerStore for InMemoryLedgerStore {
    fn save_snapshot(&mut self, slot: SlotNo, data: Vec<u8>) -> Result<(), StorageError> {
        if let Some((_, existing)) = self
            .snapshots
            .iter_mut()
            .find(|(snapshot_slot, _)| *snapshot_slot == slot)
        {
            *existing = data;
        } else {
            self.snapshots.push((slot, data));
            self.snapshots
                .sort_by_key(|(snapshot_slot, _)| *snapshot_slot);
        }
        Ok(())
    }

    fn latest_snapshot(&self) -> Option<(SlotNo, &[u8])> {
        self.snapshots.last().map(|(s, d)| (*s, d.as_slice()))
    }

    fn latest_snapshot_before_or_at(&self, slot: SlotNo) -> Option<(SlotNo, &[u8])> {
        self.snapshots
            .iter()
            .rev()
            .find(|(snapshot_slot, _)| *snapshot_slot <= slot)
            .map(|(snapshot_slot, data)| (*snapshot_slot, data.as_slice()))
    }

    fn truncate_after(&mut self, slot: Option<SlotNo>) -> Result<(), StorageError> {
        match slot {
            Some(slot) => self
                .snapshots
                .retain(|(snapshot_slot, _)| *snapshot_slot <= slot),
            None => self.snapshots.clear(),
        }
        Ok(())
    }

    fn retain_latest(&mut self, max_snapshots: usize) -> Result<(), StorageError> {
        if max_snapshots == 0 {
            self.snapshots.clear();
        } else if self.snapshots.len() > max_snapshots {
            let remove_count = self.snapshots.len() - max_snapshots;
            self.snapshots.drain(..remove_count);
        }
        Ok(())
    }

    fn count(&self) -> usize {
        self.snapshots.len()
    }
}
