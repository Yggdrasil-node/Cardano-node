/// A minimal append-only store representing the immutable block database.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImmutableBlockStore {
    blocks: Vec<String>,
}

impl ImmutableBlockStore {
    /// Appends a block identifier to the immutable sequence.
    pub fn append(&mut self, block_id: String) {
        self.blocks.push(block_id);
    }

    /// Returns the number of persisted immutable block identifiers.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Returns whether the immutable store is currently empty.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}
