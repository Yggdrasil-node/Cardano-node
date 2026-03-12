#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LedgerSnapshotStore {
    snapshots: Vec<String>,
}

impl LedgerSnapshotStore {
    pub fn persist(&mut self, snapshot_id: String) {
        self.snapshots.push(snapshot_id);
    }

    pub fn count(&self) -> usize {
        self.snapshots.len()
    }
}
