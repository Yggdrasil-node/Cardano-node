//! File-backed implementation of [`ImmutableStore`].
//!
//! Each finalized block is stored as a JSON file named by its hex-encoded
//! header hash. An in-memory index tracks insertion order for tip queries.
//! On startup the index is rebuilt by scanning the data directory.
//!
//! Reference: `Ouroboros.Consensus.Storage.ImmutableDB` in the official node.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_ledger::{Block, HeaderHash, Point};

use crate::error::StorageError;
use crate::immutable_db::ImmutableStore;

/// Writes `data` to `path` atomically by writing to a temp file first and
/// then renaming. This prevents partial writes on crash.
fn atomic_write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, data)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// File-backed immutable block store.
///
/// Blocks are persisted as `{hex_hash}.json` files inside `data_dir`.
/// The store is append-only: once written, files are never modified or deleted.
pub struct FileImmutable {
    data_dir: PathBuf,
    /// Ordered list of header hashes matching insertion order.
    chain: Vec<HeaderHash>,
    /// In-memory block cache keyed by header hash.
    index: HashMap<HeaderHash, Block>,
}

impl FileImmutable {
    /// Opens or creates a file-backed immutable store at `data_dir`.
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

impl ImmutableStore for FileImmutable {
    fn append_block(&mut self, block: Block) -> Result<(), StorageError> {
        if self.index.contains_key(&block.header.hash) {
            return Err(StorageError::DuplicateBlock(block.header.hash));
        }

        let path = self.block_path(&block.header.hash);
        let json = serde_json::to_string(&block)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        atomic_write_file(&path, json.as_bytes())?;

        self.chain.push(block.header.hash);
        self.index.insert(block.header.hash, block);
        Ok(())
    }

    fn get_tip(&self) -> Point {
        self.chain.last().map_or(Point::Origin, |hash| {
            let block = &self.index[hash];
            Point::BlockPoint(block.header.slot_no, block.header.hash)
        })
    }

    fn get_block(&self, hash: &HeaderHash) -> Option<&Block> {
        self.index.get(hash)
    }

    fn suffix_after(&self, point: &Point) -> Result<Vec<Block>, StorageError> {
        match point {
            Point::Origin => Ok(self
                .chain
                .iter()
                .filter_map(|hash| self.index.get(hash).cloned())
                .collect()),
            Point::BlockPoint(slot, hash) => {
                if self.chain.is_empty() {
                    return Ok(Vec::new());
                }

                let first_slot = self.index[&self.chain[0]].header.slot_no;
                if *slot < first_slot {
                    return Ok(self
                        .chain
                        .iter()
                        .filter_map(|block_hash| self.index.get(block_hash).cloned())
                        .collect());
                }

                if let Some(pos) = self.chain.iter().position(|block_hash| *block_hash == *hash) {
                    return Ok(self.chain[pos + 1..]
                        .iter()
                        .filter_map(|block_hash| self.index.get(block_hash).cloned())
                        .collect());
                }

                let last_slot = self.index[&self.chain[self.chain.len() - 1]].header.slot_no;
                if *slot > last_slot {
                    return Ok(Vec::new());
                }

                Err(StorageError::PointNotFound)
            }
        }
    }

    fn len(&self) -> usize {
        self.chain.len()
    }
}

/// Encode a byte slice as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
