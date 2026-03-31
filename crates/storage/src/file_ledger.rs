//! File-backed implementation of [`LedgerStore`].
//!
//! Each snapshot is stored as a file named `snapshot_{slot}.dat` inside the
//! data directory. The most recently saved snapshot is tracked in memory.
//!
//! Reference: `Ouroboros.Consensus.Storage.LedgerDB` in the official node.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use yggdrasil_ledger::SlotNo;

use crate::error::StorageError;
use crate::ledger_db::LedgerStore;

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

/// File-backed ledger snapshot store.
///
/// Snapshots are persisted as `snapshot_{slot}.dat` files inside `data_dir`.
pub struct FileLedgerStore {
    data_dir: PathBuf,
    /// Path to the write-ahead dirty sentinel file.
    dirty_path: PathBuf,
    /// Ordered (slot, data) pairs loaded at startup.
    snapshots: Vec<(SlotNo, Vec<u8>)>,
}

impl FileLedgerStore {
    /// Opens or creates a file-backed ledger store at `data_dir`.
    ///
    /// Existing snapshot files are scanned and loaded, sorted by slot number.
    /// Unreadable files are silently skipped.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        let dirty_path = data_dir.join("dirty.flag");
        let had_dirty = dirty_path.exists();
        if had_dirty {
            eprintln!(
                "[storage] LedgerStore: dirty sentinel found at {:?}; \
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

        let mut snapshots = Vec::new();
        for entry in fs::read_dir(&data_dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(slot_str) = name.strip_prefix("snapshot_").and_then(|s| s.strip_suffix(".dat")) {
                    if let Ok(slot) = slot_str.parse::<u64>() {
                        match fs::read(&path) {
                            Ok(data) => snapshots.push((SlotNo(slot), data)),
                            Err(_) => {
                                // Skip unreadable snapshot files.
                            }
                        }
                    }
                }
            }
        }

        snapshots.sort_by_key(|(slot, _)| *slot);

        // Stale dirty sentinel has been recovered; clear it so subsequent
        // opens do not produce spurious warnings.
        if had_dirty {
            let _ = fs::remove_file(&dirty_path);
        }

        Ok(Self { data_dir, dirty_path, snapshots })
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

    fn snapshot_path(&self, slot: SlotNo) -> PathBuf {
        self.data_dir.join(format!("snapshot_{}.dat", slot.0))
    }
}

impl LedgerStore for FileLedgerStore {
    fn save_snapshot(&mut self, slot: SlotNo, data: Vec<u8>) -> Result<(), StorageError> {
        let path = self.snapshot_path(slot);
        self.mark_dirty()?;
        atomic_write_file(&path, &data)?;
        if let Some((_, existing)) = self
            .snapshots
            .iter_mut()
            .find(|(snapshot_slot, _)| *snapshot_slot == slot)
        {
            *existing = data;
        } else {
            self.snapshots.push((slot, data));
            self.snapshots.sort_by_key(|(snapshot_slot, _)| *snapshot_slot);
        }
        self.mark_clean()?;
        Ok(())
    }

    fn latest_snapshot(&self) -> Option<(SlotNo, &[u8])> {
        self.snapshots.last().map(|(s, d)| (*s, d.as_slice()))
    }

    fn latest_snapshot_before_or_at(&self, slot: SlotNo) -> Option<(SlotNo, &[u8])> {
        self.snapshots
            .iter()
            .rev()
            .find(|(snapshot_slot, _)| *snapshot_slot <= slot)
            .map(|(snapshot_slot, data)| (*snapshot_slot, data.as_slice()))
    }

    fn truncate_after(&mut self, slot: Option<SlotNo>) -> Result<(), StorageError> {
        self.mark_dirty()?;
        let retained: Vec<(SlotNo, Vec<u8>)> = self
            .snapshots
            .iter()
            .filter(|(snapshot_slot, _)| slot.is_some_and(|limit| *snapshot_slot <= limit))
            .cloned()
            .collect();

        for (snapshot_slot, _) in &self.snapshots {
            if slot.is_none_or(|limit| *snapshot_slot > limit) {
                let path = self.snapshot_path(*snapshot_slot);
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }

        self.snapshots = if slot.is_some() { retained } else { Vec::new() };
        self.mark_clean()?;
        Ok(())
    }

    fn retain_latest(&mut self, max_snapshots: usize) -> Result<(), StorageError> {
        if max_snapshots == 0 {
            return self.truncate_after(None);
        }

        if self.snapshots.len() <= max_snapshots {
            return Ok(());
        }

        let remove_count = self.snapshots.len() - max_snapshots;
        let removed: Vec<SlotNo> = self
            .snapshots
            .iter()
            .take(remove_count)
            .map(|(slot, _)| *slot)
            .collect();

        self.mark_dirty()?;
        for slot in &removed {
            let path = self.snapshot_path(*slot);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }

        self.snapshots.drain(..remove_count);
        self.mark_clean()?;
        Ok(())
    }

    fn count(&self) -> usize {
        self.snapshots.len()
    }
}
