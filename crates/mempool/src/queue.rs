/// A mempool entry carrying a transaction identifier and its fee for ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MempoolEntry {
    pub tx_id: String,
    pub fee: u64,
}

/// A minimal fee-ordered mempool.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Mempool {
    entries: Vec<MempoolEntry>,
}

impl Mempool {
    /// Inserts an entry and keeps the queue ordered by descending fee.
    pub fn insert(&mut self, entry: MempoolEntry) {
        self.entries.push(entry);
        self.entries.sort_by(|left, right| right.fee.cmp(&left.fee));
    }

    /// Removes and returns the currently best-fee entry, if any.
    pub fn pop_best(&mut self) -> Option<MempoolEntry> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }
}
