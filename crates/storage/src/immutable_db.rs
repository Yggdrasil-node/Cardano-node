#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImmutableBlockStore {
    blocks: Vec<String>,
}

impl ImmutableBlockStore {
    pub fn append(&mut self, block_id: String) {
        self.blocks.push(block_id);
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}
