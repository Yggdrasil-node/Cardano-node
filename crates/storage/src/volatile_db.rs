#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VolatileBlockStore {
    blocks: Vec<String>,
}

impl VolatileBlockStore {
    pub fn insert(&mut self, block_id: String) {
        self.blocks.push(block_id);
    }

    pub fn rollback(&mut self, len: usize) {
        self.blocks.truncate(len);
    }
}
