use yggdrasil_ledger::{Block, HeaderHash, Point};

use crate::error::StorageError;

/// Append-only store for immutable (finalized) blocks.
///
/// Once a block crosses the immutability window it is moved from the volatile
/// DB into the immutable DB and can never be rolled back.
///
/// Reference: `Ouroboros.Consensus.Storage.ImmutableDB.API`.
pub trait ImmutableStore {
    /// Appends a finalized block. Returns an error if a block with the same
    /// header hash already exists.
    fn append_block(&mut self, block: Block) -> Result<(), StorageError>;

    /// Returns the tip of the immutable chain as a [`Point`].
    fn get_tip(&self) -> Point;

    /// Retrieves a block by its header hash.
    fn get_block(&self, hash: &HeaderHash) -> Option<&Block>;

    /// Returns the number of stored blocks.
    fn len(&self) -> usize;

    /// Returns `true` when no blocks have been stored.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// In-memory immutable store for tests and interface stabilization.
#[derive(Clone, Debug, Default)]
pub struct InMemoryImmutable {
    blocks: Vec<Block>,
}

impl ImmutableStore for InMemoryImmutable {
    fn append_block(&mut self, block: Block) -> Result<(), StorageError> {
        if self.blocks.iter().any(|b| b.header.hash == block.header.hash) {
            return Err(StorageError::DuplicateBlock(block.header.hash));
        }
        self.blocks.push(block);
        Ok(())
    }

    fn get_tip(&self) -> Point {
        self.blocks.last().map_or(Point::Origin, |b| {
            Point::BlockPoint(b.header.slot_no, b.header.hash)
        })
    }

    fn get_block(&self, hash: &HeaderHash) -> Option<&Block> {
        self.blocks.iter().find(|b| b.header.hash == *hash)
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }
}
