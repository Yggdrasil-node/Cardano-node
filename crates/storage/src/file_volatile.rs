//! File-backed implementation of [`VolatileStore`].
//!
//! Each block in the volatile window is stored as a JSON file named by its
//! hex-encoded header hash. Rollback deletes files for blocks beyond the
//! rollback point. An in-memory ordered chain vector tracks current state.
//!
//! Reference: `Ouroboros.Consensus.Storage.VolatileDB` in the official node.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_ledger::{Block, HeaderHash, Point};

use crate::error::StorageError;
use crate::volatile_db::VolatileStore;

/// File-backed volatile block store with rollback support.
///
/// Blocks are persisted as `{hex_hash}.json` files inside `data_dir`.
/// Rollback removes files for discarded blocks.
pub struct FileVolatile {
    data_dir: PathBuf,
    /// Ordered list of header hashes matching insertion order.
    chain: Vec<HeaderHash>,
    /// In-memory block cache keyed by header hash.
    index: HashMap<HeaderHash, Block>,
}

impl FileVolatile {
    /// Opens or creates a file-backed volatile store at `data_dir`.
    ///
    /// If the directory already exists its contents are scanned and the
    /// chain order is recovered from block slot numbers.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        let mut blocks = Vec::new();
        for entry in fs::read_dir(&data_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let contents = fs::read_to_string(&path)?;
                let block: Block = serde_json::from_str(&contents)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                blocks.push(block);
            }
        }

        // Sort by slot to recover insertion order.
        blocks.sort_by_key(|b| b.header.slot_no);

        let chain: Vec<HeaderHash> = blocks.iter().map(|b| b.header.hash).collect();
        let index: HashMap<HeaderHash, Block> = blocks
            .into_iter()
            .map(|b| (b.header.hash, b))
            .collect();

        Ok(Self { data_dir, chain, index })
    }

    fn block_path(&self, hash: &HeaderHash) -> PathBuf {
        self.data_dir.join(format!("{}.json", hex_encode(&hash.0)))
    }
}

impl VolatileStore for FileVolatile {
    fn add_block(&mut self, block: Block) -> Result<(), StorageError> {
        if self.index.contains_key(&block.header.hash) {
            return Err(StorageError::DuplicateBlock(block.header.hash));
        }

        let path = self.block_path(&block.header.hash);
        let json = serde_json::to_string(&block)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        fs::write(&path, json)?;

        self.chain.push(block.header.hash);
        self.index.insert(block.header.hash, block);
        Ok(())
    }

    fn get_block(&self, hash: &HeaderHash) -> Option<&Block> {
        self.index.get(hash)
    }

    fn prefix_up_to(&self, point: &Point) -> Result<Vec<Block>, StorageError> {
        match point {
            Point::Origin => Ok(Vec::new()),
            Point::BlockPoint(_, hash) => {
                let pos = self
                    .chain
                    .iter()
                    .position(|h| h == hash)
                    .ok_or(StorageError::PointNotFound)?;
                self.chain[..=pos]
                    .iter()
                    .map(|prefix_hash| {
                        self.index
                            .get(prefix_hash)
                            .cloned()
                            .ok_or(StorageError::PointNotFound)
                    })
                    .collect()
            }
        }
    }

    fn prune_up_to(&mut self, point: &Point) -> Result<(), StorageError> {
        match point {
            Point::Origin => Ok(()),
            Point::BlockPoint(_, hash) => {
                let prune_count = self
                    .chain
                    .iter()
                    .position(|h| h == hash)
                    .map(|pos| pos + 1)
                    .ok_or(StorageError::PointNotFound)?;

                let removed: Vec<HeaderHash> = self.chain.drain(..prune_count).collect();
                for removed_hash in removed {
                    let path = self.block_path(&removed_hash);
                    let _ = fs::remove_file(path);
                    self.index.remove(&removed_hash);
                }
                Ok(())
            }
        }
    }

    fn rollback_to(&mut self, point: &Point) {
        match point {
            Point::Origin => {
                for hash in &self.chain {
                    let path = self.block_path(hash);
                    let _ = fs::remove_file(path);
                }
                self.chain.clear();
                self.index.clear();
            }
            Point::BlockPoint(_, hash) => {
                if let Some(pos) = self.chain.iter().position(|h| h == hash) {
                    let removed: Vec<HeaderHash> = self.chain.drain((pos + 1)..).collect();
                    for h in &removed {
                        let path = self.block_path(h);
                        let _ = fs::remove_file(path);
                        self.index.remove(h);
                    }
                }
            }
        }
    }

    fn tip(&self) -> Point {
        self.chain.last().map_or(Point::Origin, |hash| {
            let block = &self.index[hash];
            Point::BlockPoint(block.header.slot_no, block.header.hash)
        })
    }
}

/// Encode a byte slice as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
