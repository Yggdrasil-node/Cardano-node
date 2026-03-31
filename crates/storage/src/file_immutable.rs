//! File-backed implementation of [`ImmutableStore`].
//!
//! Each finalized block is stored as a CBOR file named by its hex-encoded
//! header hash. An in-memory index tracks insertion order for tip queries.
//! On startup the index is rebuilt by scanning the data directory.
//!
//! Reference: `Ouroboros.Consensus.Storage.ImmutableDB` in the official node.

use std::collections::{HashMap, hash_map::Entry};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use yggdrasil_ledger::{Block, HeaderHash, Point, SlotNo};

use crate::error::StorageError;
use crate::immutable_db::ImmutableStore;

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

/// File-backed immutable block store.
///
/// Blocks are persisted as `{hex_hash}.cbor` files inside `data_dir`.
/// The store is append-only: once written, files are never modified or deleted
/// except via [`ImmutableStore::trim_before_slot`] garbage collection.
pub struct FileImmutable {
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

        let dirty_path = data_dir.join("dirty.flag");
        let had_dirty = dirty_path.exists();
        if had_dirty {
            // A dirty sentinel left from a previous run indicates that the
            // node did not shut down cleanly.  The scan below will recover
            // whatever complete block files are present; corrupted or
            // partially-written files are silently skipped.
            eprintln!(
                "[storage] ImmutableStore: dirty sentinel found at {:?}; \
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
                            // Corrupted block file — skip rather than fail.
                            skipped += 1;
                        }
                    },
                    Err(_) => {
                        // Unreadable file — skip.
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
            // Also skip leftover .tmp files from atomic writes.
        }

        // Sort by slot to recover insertion order.
        let mut blocks: Vec<Block> = blocks_by_hash
            .into_values()
            .map(|(block, _)| block)
            .collect();
        blocks.sort_by_key(|b| b.header.slot_no);

        let chain: Vec<HeaderHash> = blocks.iter().map(|b| b.header.hash).collect();
        let index: HashMap<HeaderHash, Block> = blocks
            .into_iter()
            .map(|b| (b.header.hash, b))
            .collect();

        // Stale dirty sentinel has been recovered; clear it so subsequent
        // opens do not produce spurious warnings.
        if had_dirty {
            let _ = fs::remove_file(&dirty_path);
            if skipped > 0 {
                eprintln!("  -> skipped {skipped} unreadable block file(s) during recovery");
            }
        }

        Ok(Self { data_dir, dirty_path, chain, index, skipped_on_open: skipped })
    }

    /// Returns the number of block files that were skipped during
    /// [`FileImmutable::open`] due to corruption or read errors.
    pub fn skipped_on_open(&self) -> usize {
        self.skipped_on_open
    }

    fn mark_dirty(&self) -> std::io::Result<()> {
        // Sentinel is written before any mutation begins.  If the process
        // crashes between mark_dirty and mark_clean the sentinel survives,
        // and the next open() will log a warning before recovering normally.
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
}

impl ImmutableStore for FileImmutable {
    fn append_block(&mut self, block: Block) -> Result<(), StorageError> {
        if self.index.contains_key(&block.header.hash) {
            return Err(StorageError::DuplicateBlock(block.header.hash));
        }

        self.mark_dirty()?;
        let path = self.block_path(&block.header.hash);
        let cbor = serde_cbor::to_vec(&block)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        atomic_write_file(&path, &cbor)?;

        self.chain.push(block.header.hash);
        self.index.insert(block.header.hash, block);
        self.mark_clean()?;
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
        self.mark_dirty()?;
        let to_remove: Vec<HeaderHash> = self
            .chain
            .iter()
            .filter(|hash| self.index[hash].header.slot_no < slot)
            .copied()
            .collect();
        let removed = to_remove.len();
        for hash in &to_remove {
            let cbor_path = self.block_path(hash);
            if cbor_path.exists() {
                fs::remove_file(&cbor_path)?;
            }
            let json_path = self.legacy_json_block_path(hash);
            if json_path.exists() {
                fs::remove_file(&json_path)?;
            }
            self.index.remove(hash);
        }
        self.chain.retain(|hash| !to_remove.contains(hash));
        self.mark_clean()?;
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
