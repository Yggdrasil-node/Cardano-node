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
        self.snapshots.push((slot, data));
        Ok(())
    }

    fn latest_snapshot(&self) -> Option<(SlotNo, &[u8])> {
        self.snapshots.last().map(|(s, d)| (*s, d.as_slice()))
    }

    fn count(&self) -> usize {
        self.snapshots.len()
    }
}
