//! File-backed implementation of [`LedgerStore`].
//!
//! Each snapshot is stored as a file named `snapshot_{slot}.dat` inside the
//! data directory. The most recently saved snapshot is tracked in memory.
//!
//! Reference: `Ouroboros.Consensus.Storage.LedgerDB` in the official node.

use std::fs;
use std::path::{Path, PathBuf};

use yggdrasil_ledger::SlotNo;

use crate::error::StorageError;
use crate::ledger_db::LedgerStore;

/// Writes `data` to `path` atomically by writing to a temp file first and
/// then renaming. This prevents partial writes on crash.
fn atomic_write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, data)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// File-backed ledger snapshot store.
///
/// Snapshots are persisted as `snapshot_{slot}.dat` files inside `data_dir`.
pub struct FileLedgerStore {
    data_dir: PathBuf,
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

        Ok(Self { data_dir, snapshots })
    }

    fn snapshot_path(&self, slot: SlotNo) -> PathBuf {
        self.data_dir.join(format!("snapshot_{}.dat", slot.0))
    }
}

impl LedgerStore for FileLedgerStore {
    fn save_snapshot(&mut self, slot: SlotNo, data: Vec<u8>) -> Result<(), StorageError> {
        let path = self.snapshot_path(slot);
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

        for slot in &removed {
            let path = self.snapshot_path(*slot);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }

        self.snapshots.drain(..remove_count);
        Ok(())
    }

    fn count(&self) -> usize {
        self.snapshots.len()
    }
}
