/// A minimal rollback-aware store representing the volatile block database.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VolatileBlockStore {
    blocks: Vec<String>,
}

impl VolatileBlockStore {
    /// Inserts a block identifier into the volatile suffix.
    pub fn insert(&mut self, block_id: String) {
        self.blocks.push(block_id);
    }

    /// Rolls back the volatile suffix to the provided retained length.
    pub fn rollback(&mut self, len: usize) {
        self.blocks.truncate(len);
    }
}
