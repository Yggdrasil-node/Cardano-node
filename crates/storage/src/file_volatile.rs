//! File-backed implementation of [`VolatileStore`].
//!
//! Each block in the volatile window is stored as a CBOR file named by its
//! hex-encoded header hash. Rollback deletes files for blocks beyond the
//! rollback point. An in-memory ordered chain vector tracks current state.
//!
//! Reference: `Ouroboros.Consensus.Storage.VolatileDB` in the official node.

use std::collections::{HashMap, hash_map::Entry};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use yggdrasil_ledger::{Block, HeaderHash, Point, SlotNo};

use crate::error::StorageError;
use crate::volatile_db::VolatileStore;

/// Writes `data` to `path` atomically by writing to a temp file first and
/// then renaming. This prevents partial writes on crash.
///
/// `sync_all()` is called on the temp file before rename to ensure data
/// reaches durable storage, and the parent directory is synced after rename
/// so the directory entry is durable.
fn atomic_write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    sync_dir(path.parent())?;
    Ok(())
}

/// Syncs the parent directory to make rename/unlink metadata durable.
fn sync_dir(dir: Option<&Path>) -> std::io::Result<()> {
    if let Some(d) = dir {
        let f = fs::File::open(d)?;
        f.sync_all()?;
    }
    Ok(())
}

/// File-backed volatile block store with rollback support.
///
/// Blocks are persisted as `{hex_hash}.cbor` files inside `data_dir`.
/// Rollback removes files for discarded blocks. Corrupted files are
/// silently skipped on open so that an incomplete shutdown does not
/// prevent the node from restarting.
pub struct FileVolatile {
    data_dir: PathBuf,
    /// Path to the write-ahead dirty sentinel file.
    dirty_path: PathBuf,
    /// Ordered list of header hashes matching insertion order.
    chain: Vec<HeaderHash>,
    /// In-memory block cache keyed by header hash.
    index: HashMap<HeaderHash, Block>,
    /// Number of corrupted or unreadable files skipped during open.
    skipped_on_open: usize,
}

impl FileVolatile {
    /// Opens or creates a file-backed volatile store at `data_dir`.
    ///
    /// If the directory already exists its contents are scanned and the
    /// chain order is recovered from block slot numbers. Corrupted or
    /// unreadable block files are skipped.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        let dirty_path = data_dir.join("dirty.flag");
        let had_dirty = dirty_path.exists();
        if had_dirty {
            eprintln!(
                "[storage] VolatileStore: dirty sentinel found at {:?}; \
                 recovering from unclean shutdown",
                dirty_path
            );
            // Remove leftover .tmp files from incomplete atomic writes.
            let mut tmp_removed = 0usize;
            if let Ok(entries) = fs::read_dir(&data_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("tmp")
                        && fs::remove_file(&p).is_ok()
                    {
                        tmp_removed += 1;
                    }
                }
            }
            if tmp_removed > 0 {
                eprintln!("  -> removed {tmp_removed} incomplete .tmp file(s)");
            }
        }

        let mut blocks_by_hash: HashMap<HeaderHash, (Block, bool)> = HashMap::new();
        let mut skipped: usize = 0;
        for entry in fs::read_dir(&data_dir)? {
            let entry = entry?;
            let path = entry.path();
            match path.extension().and_then(|e| e.to_str()) {
                Some("cbor") => match fs::read(&path) {
                    Ok(bytes) => match serde_cbor::from_slice::<Block>(&bytes) {
                        Ok(block) => {
                            match blocks_by_hash.entry(block.header.hash) {
                                Entry::Vacant(vacant) => {
                                    vacant.insert((block, true));
                                }
                                Entry::Occupied(mut occupied) => {
                                    // Prefer CBOR over legacy JSON when both exist.
                                    if !occupied.get().1 {
                                        occupied.insert((block, true));
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            skipped += 1;
                        }
                    },
                    Err(_) => {
                        skipped += 1;
                    }
                },
                Some("json") => {
                    // Backward-compatible read path for legacy JSON block files.
                    match fs::read_to_string(&path) {
                        Ok(contents) => match serde_json::from_str::<Block>(&contents) {
                            Ok(block) => {
                                match blocks_by_hash.entry(block.header.hash) {
                                    Entry::Vacant(vacant) => {
                                        vacant.insert((block, false));
                                    }
                                    Entry::Occupied(_) => {
                                        // Duplicate representation for the same hash.
                                        // Keep the existing one (CBOR preferred).
                                    }
                                }
                            }
                            Err(_) => {
                                skipped += 1;
                            }
                        },
                        Err(_) => {
                            skipped += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        // Sort by slot to recover insertion order.
        let mut blocks: Vec<Block> = blocks_by_hash
            .into_values()
            .map(|(block, _)| block)
            .collect();
        blocks.sort_by_key(|b| b.header.slot_no);

        let chain: Vec<HeaderHash> = blocks.iter().map(|b| b.header.hash).collect();
        let index: HashMap<HeaderHash, Block> =
            blocks.into_iter().map(|b| (b.header.hash, b)).collect();

        // Stale dirty sentinel has been recovered; clear it so subsequent
        // opens do not produce spurious warnings.
        if had_dirty {
            let _ = fs::remove_file(&dirty_path);
            if skipped > 0 {
                eprintln!("  -> skipped {skipped} unreadable block file(s) during recovery");
            }
        }

        Ok(Self {
            data_dir,
            dirty_path,
            chain,
            index,
            skipped_on_open: skipped,
        })
    }

    /// Returns the number of block files that were skipped during open
    /// due to corruption or read errors.
    pub fn skipped_on_open(&self) -> usize {
        self.skipped_on_open
    }

    fn mark_dirty(&self) -> std::io::Result<()> {
        {
            let f = fs::File::create(&self.dirty_path)?;
            f.sync_all()?;
        }
        sync_dir(self.dirty_path.parent())?;
        Ok(())
    }

    fn mark_clean(&self) -> std::io::Result<()> {
        let _ = fs::remove_file(&self.dirty_path);
        Ok(())
    }

    fn block_path(&self, hash: &HeaderHash) -> PathBuf {
        self.data_dir.join(format!("{}.cbor", hex_encode(&hash.0)))
    }

    fn legacy_json_block_path(&self, hash: &HeaderHash) -> PathBuf {
        self.data_dir.join(format!("{}.json", hex_encode(&hash.0)))
    }

    /// Removes orphaned block files from disk that are not tracked by the
    /// in-memory index.
    ///
    /// After a crash during rollback, files for discarded blocks may remain
    /// on disk even though they are no longer part of the volatile chain.
    /// This method scans the data directory and deletes any `.cbor` or
    /// `.json` block files whose decoded hash is not present in the current
    /// index, as well as stale `.tmp` files left by interrupted atomic
    /// writes.
    ///
    /// Returns the number of orphaned files removed.
    ///
    /// Reference: upstream VolatileDB recovery re-validates on open; this
    /// method provides an explicit post-open cleanup path.
    pub fn compact(&self) -> Result<usize, StorageError> {
        let mut removed = 0usize;
        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("cbor") | Some("json") => {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        match hex_decode_hash(stem) {
                            Some(hash) if !self.index.contains_key(&hash) => {
                                let _ = fs::remove_file(&path);
                                removed += 1;
                            }
                            None => {
                                // Not a valid block file — remove.
                                let _ = fs::remove_file(&path);
                                removed += 1;
                            }
                            _ => {}
                        }
                    }
                }
                Some("tmp") => {
                    // Stale temp file from an interrupted atomic write.
                    let _ = fs::remove_file(&path);
                    removed += 1;
                }
                _ => {}
            }
        }
        Ok(removed)
    }
}

impl VolatileStore for FileVolatile {
    fn add_block(&mut self, block: Block) -> Result<(), StorageError> {
        if self.index.contains_key(&block.header.hash) {
            return Err(StorageError::DuplicateBlock(block.header.hash));
        }

        self.mark_dirty()?;
        let path = self.block_path(&block.header.hash);
        let cbor =
            serde_cbor::to_vec(&block).map_err(|e| StorageError::Serialization(e.to_string()))?;
        atomic_write_file(&path, &cbor)?;

        self.chain.push(block.header.hash);
        self.index.insert(block.header.hash, block);
        self.mark_clean()?;
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
                self.mark_dirty()?;
                let prune_count = self
                    .chain
                    .iter()
                    .position(|h| h == hash)
                    .map(|pos| pos + 1)
                    .ok_or(StorageError::PointNotFound)?;

                let removed: Vec<HeaderHash> = self.chain.drain(..prune_count).collect();
                for removed_hash in removed {
                    let cbor_path = self.block_path(&removed_hash);
                    let json_path = self.legacy_json_block_path(&removed_hash);
                    let _ = fs::remove_file(cbor_path);
                    let _ = fs::remove_file(json_path);
                    self.index.remove(&removed_hash);
                }
                self.mark_clean()?;
                Ok(())
            }
        }
    }

    fn rollback_to(&mut self, point: &Point) {
        match point {
            Point::Origin => {
                let _ = self.mark_dirty();
                for hash in &self.chain {
                    let cbor_path = self.block_path(hash);
                    let json_path = self.legacy_json_block_path(hash);
                    let _ = fs::remove_file(cbor_path);
                    let _ = fs::remove_file(json_path);
                }
                self.chain.clear();
                self.index.clear();
                let _ = self.mark_clean();
            }
            Point::BlockPoint(_, hash) => {
                if let Some(pos) = self.chain.iter().position(|h| h == hash) {
                    let _ = self.mark_dirty();
                    let removed: Vec<HeaderHash> = self.chain.drain((pos + 1)..).collect();
                    for h in &removed {
                        let cbor_path = self.block_path(h);
                        let json_path = self.legacy_json_block_path(h);
                        let _ = fs::remove_file(cbor_path);
                        let _ = fs::remove_file(json_path);
                        self.index.remove(h);
                    }
                    let _ = self.mark_clean();
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

    fn suffix_after(&self, point: &Point) -> Vec<Block> {
        let start = match point {
            Point::Origin => 0,
            Point::BlockPoint(_, hash) => match self.chain.iter().position(|h| h == hash) {
                Some(pos) => pos + 1,
                None => return Vec::new(),
            },
        };
        if start >= self.chain.len() {
            return Vec::new();
        }
        self.chain[start..]
            .iter()
            .filter_map(|h| self.index.get(h).cloned())
            .collect()
    }

    fn garbage_collect(&mut self, slot: SlotNo) -> usize {
        // Collect hashes of blocks with slot < the given threshold.
        let to_remove: Vec<HeaderHash> = self
            .chain
            .iter()
            .filter_map(|h| {
                let block = self.index.get(h)?;
                if block.header.slot_no < slot {
                    Some(*h)
                } else {
                    None
                }
            })
            .collect();

        if to_remove.is_empty() {
            return 0;
        }

        let _ = self.mark_dirty();
        let count = to_remove.len();
        for hash in &to_remove {
            let cbor_path = self.block_path(hash);
            let json_path = self.legacy_json_block_path(hash);
            let _ = fs::remove_file(cbor_path);
            let _ = fs::remove_file(json_path);
            self.index.remove(hash);
        }

        let remove_set: std::collections::HashSet<HeaderHash> = to_remove.into_iter().collect();
        self.chain.retain(|h| !remove_set.contains(h));
        let _ = self.mark_clean();
        count
    }

    fn block_count(&self) -> usize {
        self.chain.len()
    }
}

/// Encode a byte slice as lowercase hex.
fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Decode a 64-character hex string into a 32-byte `HeaderHash`.
fn hex_decode_hash(hex: &str) -> Option<HeaderHash> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(HeaderHash(bytes))
}
