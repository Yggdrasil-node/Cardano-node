#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MempoolEntry {
    pub tx_id: String,
    pub fee: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Mempool {
    entries: Vec<MempoolEntry>,
}

impl Mempool {
    pub fn insert(&mut self, entry: MempoolEntry) {
        self.entries.push(entry);
        self.entries.sort_by(|left, right| right.fee.cmp(&left.fee));
    }

    pub fn pop_best(&mut self) -> Option<MempoolEntry> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }
}
