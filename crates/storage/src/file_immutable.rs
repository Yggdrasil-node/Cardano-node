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

use yggdrasil_ledger::{Block, HeaderHash, Point, SlotNo};

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
/// The store is append-only: once written, files are never modified or deleted
/// except via [`ImmutableStore::trim_before_slot`] garbage collection.
pub struct FileImmutable {
    data_dir: PathBuf,
    /// Ordered list of header hashes matching insertion order.
    chain: Vec<HeaderHash>,
    /// In-memory block cache keyed by header hash.
    index: HashMap<HeaderHash, Block>,
    /// Number of corrupted or unreadable files skipped during open.
    skipped_on_open: usize,
}

impl FileImmutable {
    /// Opens or creates a file-backed immutable store at `data_dir`.
    ///
    /// If the directory already exists its contents are scanned and the
    /// chain order is recovered from block slot numbers.
    ///
    /// Corrupted or unreadable block files are silently skipped so that an
    /// incomplete prior shutdown does not prevent the node from restarting.
    /// The number of skipped files is available via
    /// [`FileImmutable::skipped_on_open`].
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        let mut blocks = Vec::new();
        let mut skipped: usize = 0;
        for entry in fs::read_dir(&data_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                match fs::read_to_string(&path) {
                    Ok(contents) => match serde_json::from_str::<Block>(&contents) {
                        Ok(block) => blocks.push(block),
                        Err(_) => {
                            // Corrupted block file — skip rather than fail.
                            skipped += 1;
                        }
                    },
                    Err(_) => {
                        // Unreadable file — skip.
                        skipped += 1;
                    }
                }
            }
            // Also skip leftover .tmp files from atomic writes.
        }

        // Sort by slot to recover insertion order.
        blocks.sort_by_key(|b| b.header.slot_no);

        let chain: Vec<HeaderHash> = blocks.iter().map(|b| b.header.hash).collect();
        let index: HashMap<HeaderHash, Block> = blocks
            .into_iter()
            .map(|b| (b.header.hash, b))
            .collect();

        Ok(Self { data_dir, chain, index, skipped_on_open: skipped })
    }

    /// Returns the number of block files that were skipped during
    /// [`FileImmutable::open`] due to corruption or read errors.
    pub fn skipped_on_open(&self) -> usize {
        self.skipped_on_open
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

    fn get_block_by_slot(&self, slot: SlotNo) -> Option<&Block> {
        // The chain vec is sorted by slot, so we can binary search.
        self.chain
            .binary_search_by_key(&slot, |hash| self.index[hash].header.slot_no)
            .ok()
            .and_then(|idx| self.index.get(&self.chain[idx]))
    }

    fn trim_before_slot(&mut self, slot: SlotNo) -> Result<usize, StorageError> {
        let to_remove: Vec<HeaderHash> = self
            .chain
            .iter()
            .filter(|hash| self.index[hash].header.slot_no < slot)
            .copied()
            .collect();
        let removed = to_remove.len();
        for hash in &to_remove {
            let path = self.block_path(hash);
            if path.exists() {
                fs::remove_file(&path)?;
            }
            self.index.remove(hash);
        }
        self.chain.retain(|hash| !to_remove.contains(hash));
        Ok(removed)
    }
}

/// Encode a byte slice as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}
