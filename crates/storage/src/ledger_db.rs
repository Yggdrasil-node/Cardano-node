/// A minimal store for ledger snapshot identifiers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LedgerSnapshotStore {
    snapshots: Vec<String>,
}

impl LedgerSnapshotStore {
    /// Persists a snapshot identifier.
    pub fn persist(&mut self, snapshot_id: String) {
        self.snapshots.push(snapshot_id);
    }

    /// Returns the number of known snapshots.
    pub fn count(&self) -> usize {
        self.snapshots.len()
    }
}
