use yggdrasil_ledger::{Block, HeaderHash, Point, SlotNo};

use crate::error::StorageError;

/// Rollback-aware store for blocks that have not yet been finalized.
///
/// Blocks added to the volatile store may be rolled back if the chain
/// selection switches to a competing fork. Only after the immutability
/// window passes should a block be moved to the [`super::ImmutableStore`].
///
/// Reference: `Ouroboros.Consensus.Storage.VolatileDB.API`.
pub trait VolatileStore {
    /// Adds a block to the volatile suffix. Returns an error if a block with
    /// the same header hash already exists.
    fn add_block(&mut self, block: Block) -> Result<(), StorageError>;

    /// Retrieves a block by its header hash.
    fn get_block(&self, hash: &HeaderHash) -> Option<&Block>;

    /// Returns clones of all blocks from the volatile prefix up to and
    /// including `point`.
    ///
    /// Returns an error if `point` is not present in the current volatile
    /// chain.
    fn prefix_up_to(&self, point: &Point) -> Result<Vec<Block>, StorageError>;

    /// Prunes all volatile blocks up to and including `point`.
    ///
    /// Passing [`Point::Origin`] is a no-op. Returns an error if `point` is
    /// not present in the current volatile chain.
    fn prune_up_to(&mut self, point: &Point) -> Result<(), StorageError>;

    /// Rolls the volatile suffix back so that the given point becomes the new
    /// tip. All blocks after that point are discarded. Passing
    /// [`Point::Origin`] clears the store entirely.
    fn rollback_to(&mut self, point: &Point);

    /// Returns the tip of the volatile chain as a [`Point`].
    fn tip(&self) -> Point;

    /// Returns clones of all blocks *after* `point` in the volatile chain.
    ///
    /// This is used during rollback to capture the blocks that will be
    /// discarded so their transactions can be re-admitted to the mempool.
    ///
    /// The default implementation uses [`prefix_up_to`] to find the split
    /// point and returns the remaining suffix. Returns an empty vec for
    /// `Origin` (the entire chain would be returned by the rollback path
    /// instead) or if the point is not found.
    fn suffix_after(&self, point: &Point) -> Vec<Block>;

    /// Removes all blocks with a slot number strictly less than `slot`.
    ///
    /// Returns the number of blocks removed. This is the volatile-store
    /// counterpart of `ImmutableStore::trim_before_slot` and corresponds to
    /// the upstream `garbageCollect` function.
    ///
    /// Reference: `Ouroboros.Consensus.Storage.VolatileDB.API.garbageCollect`
    /// — removes blocks older than the most recent immutable block so they
    /// are not stored indefinitely.
    fn garbage_collect(&mut self, slot: SlotNo) -> usize;

    /// Returns the number of blocks currently stored.
    fn block_count(&self) -> usize;
}

/// In-memory volatile store for tests and interface stabilization.
#[derive(Clone, Debug, Default)]
pub struct InMemoryVolatile {
    blocks: Vec<Block>,
}

impl VolatileStore for InMemoryVolatile {
    fn add_block(&mut self, block: Block) -> Result<(), StorageError> {
        if self
            .blocks
            .iter()
            .any(|b| b.header.hash == block.header.hash)
        {
            return Err(StorageError::DuplicateBlock(block.header.hash));
        }
        self.blocks.push(block);
        Ok(())
    }

    fn get_block(&self, hash: &HeaderHash) -> Option<&Block> {
        self.blocks.iter().find(|b| b.header.hash == *hash)
    }

    fn prefix_up_to(&self, point: &Point) -> Result<Vec<Block>, StorageError> {
        match point {
            Point::Origin => Ok(Vec::new()),
            Point::BlockPoint(_, hash) => {
                let pos = self
                    .blocks
                    .iter()
                    .position(|b| b.header.hash == *hash)
                    .ok_or(StorageError::PointNotFound)?;
                Ok(self.blocks[..=pos].to_vec())
            }
        }
    }

    fn prune_up_to(&mut self, point: &Point) -> Result<(), StorageError> {
        match point {
            Point::Origin => Ok(()),
            Point::BlockPoint(_, hash) => {
                let prune_count = self
                    .blocks
                    .iter()
                    .position(|b| b.header.hash == *hash)
                    .map(|pos| pos + 1)
                    .ok_or(StorageError::PointNotFound)?;
                self.blocks.drain(..prune_count);
                Ok(())
            }
        }
    }

    fn rollback_to(&mut self, point: &Point) {
        match point {
            Point::Origin => self.blocks.clear(),
            Point::BlockPoint(_, hash) => {
                if let Some(pos) = self.blocks.iter().position(|b| b.header.hash == *hash) {
                    self.blocks.truncate(pos + 1);
                }
            }
        }
    }

    fn tip(&self) -> Point {
        self.blocks.last().map_or(Point::Origin, |b| {
            Point::BlockPoint(b.header.slot_no, b.header.hash)
        })
    }

    fn suffix_after(&self, point: &Point) -> Vec<Block> {
        match point {
            Point::Origin => self.blocks.clone(),
            Point::BlockPoint(_, hash) => {
                match self.blocks.iter().position(|b| b.header.hash == *hash) {
                    Some(pos) if pos + 1 < self.blocks.len() => self.blocks[(pos + 1)..].to_vec(),
                    _ => Vec::new(),
                }
            }
        }
    }

    fn garbage_collect(&mut self, slot: SlotNo) -> usize {
        let before = self.blocks.len();
        self.blocks.retain(|b| b.header.slot_no >= slot);
        before - self.blocks.len()
    }

    fn block_count(&self) -> usize {
        self.blocks.len()
    }
}
