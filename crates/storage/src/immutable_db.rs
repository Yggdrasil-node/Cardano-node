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

    /// Returns clones of all immutable blocks strictly after `point`.
    ///
    /// Passing [`Point::Origin`] returns the full immutable chain. If `point`
    /// precedes the first immutable block, the full chain is also returned.
    /// Returns an error when `point` falls within the covered immutable range
    /// but does not correspond to a known immutable block.
    fn suffix_after(&self, point: &Point) -> Result<Vec<Block>, StorageError>;

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

    fn suffix_after(&self, point: &Point) -> Result<Vec<Block>, StorageError> {
        match point {
            Point::Origin => Ok(self.blocks.clone()),
            Point::BlockPoint(slot, hash) => {
                if self.blocks.is_empty() {
                    return Ok(Vec::new());
                }

                if *slot < self.blocks[0].header.slot_no {
                    return Ok(self.blocks.clone());
                }

                if let Some(pos) = self.blocks.iter().position(|block| block.header.hash == *hash) {
                    return Ok(self.blocks[pos + 1..].to_vec());
                }

                if *slot > self.blocks[self.blocks.len() - 1].header.slot_no {
                    return Ok(Vec::new());
                }

                Err(StorageError::PointNotFound)
            }
        }
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }
}
