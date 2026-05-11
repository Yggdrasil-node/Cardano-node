//! Append-only store for immutable (finalized) blocks.
//!
//! ## Naming parity
//!
//! **Strict mirror:** Ouroboros/Consensus/Storage/ImmutableDB.hs.
//! Filename matches upstream basename; the module is
//! the canonical 1:1 mirror of upstream's `Ouroboros/Consensus/Storage/ImmutableDB.hs`.

use yggdrasil_ledger::{Block, HeaderHash, Point, SlotNo};

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

    /// Returns `true` when a block with the given header hash is already
    /// stored. The default implementation delegates to [`Self::get_block`];
    /// backends with an internal hash index (e.g. `FileImmutable`) should
    /// override for `O(1)` lookups.
    ///
    /// Used by [`crate::ChainDb::promote_volatile_prefix`] to make the
    /// volatile-to-immutable promotion idempotent under partial-completion
    /// crashes: if a previous run crashed between `append_block` calls or
    /// between the final append and `prune_up_to`, the volatile store still
    /// contains blocks that were already appended to the immutable store,
    /// so the next promotion attempt would otherwise fail with
    /// [`StorageError::DuplicateBlock`].
    ///
    /// Reference: upstream `Ouroboros.Consensus.Storage.ChainDB.Impl`
    /// recovers from a partial copy-to-immutable by treating already-copied
    /// blocks as a no-op rather than restarting from scratch.
    fn contains_block(&self, hash: &HeaderHash) -> bool {
        self.get_block(hash).is_some()
    }

    /// Returns clones of all immutable blocks strictly after `point`.
    ///
    /// Passing [`Point::Origin`] returns the full immutable chain. If `point`
    /// precedes the first immutable block, the full chain is also returned.
    /// Returns an error when `point` falls within the covered immutable range
    /// but does not correspond to a known immutable block.
    fn suffix_after(&self, point: &Point) -> Result<Vec<Block>, StorageError>;

    /// Streaming counterpart to [`Self::suffix_after`] — returns an
    /// iterator that yields cloned [`Block`] values one at a time
    /// rather than materializing the full chain in a `Vec<Block>`.
    ///
    /// The default implementation calls [`Self::suffix_after`] and
    /// returns the resulting `Vec`'s `into_iter()`, so existing
    /// callers see the same per-block sequence + same error semantics
    /// for point-not-found. Storage backends with internal layouts
    /// that can yield blocks lazily (e.g. a future on-disk
    /// `FileImmutable` revision that streams CBOR records from
    /// chunked log files) should override this method to avoid the
    /// intermediate `Vec`.
    ///
    /// Used by `db-analyser`'s `analysis::runner::run_analysis` to
    /// process multi-terabyte forensic chains without materializing
    /// the full chain in memory.
    ///
    /// Reference: Upstream `Ouroboros.Consensus.Storage.ImmutableDB`
    /// exposes a streaming-iterator API
    /// (`Ouroboros.Consensus.Storage.ImmutableDB.API.Iterator`); the
    /// Rust port matches the lazy-yield contract.
    fn iter_after<'a>(
        &'a self,
        point: &Point,
    ) -> Result<Box<dyn Iterator<Item = Block> + 'a>, StorageError> {
        let blocks = self.suffix_after(point)?;
        Ok(Box::new(blocks.into_iter()))
    }

    /// Returns the number of stored blocks.
    fn len(&self) -> usize;

    /// Returns `true` when no blocks have been stored.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieves a block by its slot number.
    ///
    /// Returns the first block found at the given slot, or `None` if no
    /// immutable block occupies that slot. The default implementation
    /// performs a linear scan; backends with a slot index should override
    /// for O(1) / O(log n) performance.
    ///
    /// Reference: The Haskell `ImmutableDB` maintains a slot-indexed
    /// on-disk structure for O(1) slot lookups.
    fn get_block_by_slot(&self, _slot: SlotNo) -> Option<&Block> {
        None
    }

    /// Removes all immutable blocks with slots strictly before `slot`.
    ///
    /// Returns the number of blocks removed. Blocks at `slot` or later are
    /// retained. This is the immutable-store analogue of
    /// `VolatileStore::prune_up_to`.
    ///
    /// Reference: `Ouroboros.Consensus.Storage.ImmutableDB` GC semantics —
    /// the official node periodically garbage-collects immutable chunks that
    /// are no longer needed for chain recovery or ledger replay.
    fn trim_before_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError>;

    /// Removes all immutable blocks with slots strictly **after** `slot`.
    ///
    /// Returns the number of blocks removed. Blocks at `slot` or earlier are
    /// retained. This is the inverse of [`Self::trim_before_slot`] and is
    /// the storage primitive used by the `db-truncater` operator tool to
    /// rewind a ChainDB to an earlier point.
    ///
    /// Reference: `Ouroboros.Consensus.Storage.ImmutableDB.Tools.DBTruncater`
    /// — upstream's `db-truncater` rewinds a ChainDB by truncating the
    /// immutable-DB to a specified slot, dropping all blocks beyond that
    /// point. The operation is destructive and irreversible.
    fn trim_after_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError>;
}

/// In-memory immutable store for tests and interface stabilization.
#[derive(Clone, Debug, Default)]
pub struct InMemoryImmutable {
    blocks: Vec<Block>,
}

impl InMemoryImmutable {
    /// Resolve the `point` argument of [`ImmutableStore::suffix_after`] /
    /// [`ImmutableStore::iter_after`] into the index of the first block
    /// to yield (`blocks[start..]` is the suffix).
    ///
    /// Returns `blocks.len()` when the chain is fully consumed (suffix
    /// is empty), `0` when the chain should be yielded in full, or an
    /// error when the point falls within the covered range but does
    /// not correspond to a known block (`PointNotFound`).
    fn resolve_suffix_start(&self, point: &Point) -> Result<usize, StorageError> {
        match point {
            Point::Origin => Ok(0),
            Point::BlockPoint(slot, hash) => {
                if self.blocks.is_empty() {
                    return Ok(0);
                }
                if *slot < self.blocks[0].header.slot_no {
                    return Ok(0);
                }
                if let Some(pos) = self
                    .blocks
                    .iter()
                    .position(|block| block.header.hash == *hash)
                {
                    return Ok(pos + 1);
                }
                if *slot > self.blocks[self.blocks.len() - 1].header.slot_no {
                    return Ok(self.blocks.len());
                }
                Err(StorageError::PointNotFound)
            }
        }
    }
}

impl ImmutableStore for InMemoryImmutable {
    fn append_block(&mut self, block: Block) -> Result<(), StorageError> {
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

    fn get_tip(&self) -> Point {
        self.blocks.last().map_or(Point::Origin, |b| {
            Point::BlockPoint(b.header.slot_no, b.header.hash)
        })
    }

    fn get_block(&self, hash: &HeaderHash) -> Option<&Block> {
        self.blocks.iter().find(|b| b.header.hash == *hash)
    }

    fn contains_block(&self, hash: &HeaderHash) -> bool {
        self.blocks.iter().any(|b| b.header.hash == *hash)
    }

    fn suffix_after(&self, point: &Point) -> Result<Vec<Block>, StorageError> {
        let start = self.resolve_suffix_start(point)?;
        Ok(self.blocks[start..].to_vec())
    }

    fn iter_after<'a>(
        &'a self,
        point: &Point,
    ) -> Result<Box<dyn Iterator<Item = Block> + 'a>, StorageError> {
        let start = self.resolve_suffix_start(point)?;
        Ok(Box::new(self.blocks[start..].iter().cloned()))
    }

    fn len(&self) -> usize {
        self.blocks.len()
    }

    fn get_block_by_slot(&self, slot: SlotNo) -> Option<&Block> {
        self.blocks.iter().find(|b| b.header.slot_no == slot)
    }

    fn trim_before_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError> {
        let before = self.blocks.len();
        self.blocks.retain(|b| b.header.slot_no >= slot);
        Ok(before - self.blocks.len())
    }

    fn trim_after_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError> {
        let before = self.blocks.len();
        self.blocks.retain(|b| b.header.slot_no <= slot);
        Ok(before - self.blocks.len())
    }
}
