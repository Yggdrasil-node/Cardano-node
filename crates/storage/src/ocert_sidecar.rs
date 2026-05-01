//! Sidecar persistence for opaque consensus-adjacent state files.
//!
//! The canonical nonce/OpCert path is the slot-indexed ChainDepState history
//! under `chain_dep_state/<slot-hex>.cbor`. The node crate owns the typed
//! bundle; storage owns only atomic byte persistence, lookup, truncation, and
//! retention. Keeping the layout opaque mirrors the way `LedgerStore` stores
//! ledger snapshots as raw bytes while `LedgerStateCheckpoint` owns the typed
//! encoding.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::StorageError;
use yggdrasil_ledger::{Point, SlotNo};

/// Filename used for the active stake-snapshot rotation sidecar (R203).
pub const STAKE_SNAPSHOTS_FILENAME: &str = "stake_snapshots.cbor";

/// Directory used for slot-indexed ChainDepState sidecar snapshots.
pub const CHAIN_DEP_STATE_DIR: &str = "chain_dep_state";

fn stake_snapshots_sidecar_path(dir: &Path) -> PathBuf {
    dir.join(STAKE_SNAPSHOTS_FILENAME)
}

fn chain_dep_state_dir(dir: &Path) -> PathBuf {
    dir.join(CHAIN_DEP_STATE_DIR)
}

fn chain_dep_state_snapshot_path(dir: &Path, slot: SlotNo) -> PathBuf {
    chain_dep_state_dir(dir).join(format!("{:016x}.cbor", slot.0))
}

fn parse_chain_dep_state_slot(path: &Path) -> Option<SlotNo> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("cbor") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.len() != 16 {
        return None;
    }
    u64::from_str_radix(stem, 16).ok().map(SlotNo)
}

fn sync_dir(dir: Option<&Path>) -> std::io::Result<()> {
    if let Some(d) = dir {
        let f = fs::File::open(d)?;
        f.sync_all()?;
    }
    Ok(())
}

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

/// R203 — atomically writes the encoded `StakeSnapshots`
/// sidecar to `<dir>/stake_snapshots.cbor`. Same atomic-write contract as
/// the slot-indexed ChainDepState snapshots.
pub fn save_stake_snapshots(dir: &Path, encoded: &[u8]) -> Result<(), StorageError> {
    fs::create_dir_all(dir)?;
    atomic_write_file(&stake_snapshots_sidecar_path(dir), encoded)?;
    Ok(())
}

/// R203 — loads the `StakeSnapshots` sidecar from
/// `<dir>/stake_snapshots.cbor`.  Returns `Ok(None)` when the
/// file does not exist (fresh node, pre-epoch-boundary, or
/// path without stake-snapshot tracking).
pub fn load_stake_snapshots(dir: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let path = stake_snapshots_sidecar_path(dir);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Atomically writes an opaque ChainDepState snapshot under
/// `<dir>/chain_dep_state/<slot-hex>.cbor`.
///
/// The storage crate does not decode these bytes. The node crate owns the
/// typed bundle (`Point`, nonce state, OpCert counters) while storage owns the
/// slot-indexed persistence and retention mechanics. `Point::Origin` has no
/// slot-indexed filename and is rejected; callers should truncate the
/// directory when rolling back to origin.
///
/// Reference: upstream `LedgerDB` snapshots are keyed by the slot in their
/// tip point and restored/replayed by `openDB`; this helper mirrors that
/// durability pattern for the consensus-side state that Yggdrasil stores next
/// to ledger checkpoints.
pub fn save_chain_dep_state_snapshot(
    dir: &Path,
    point: &Point,
    encoded: &[u8],
) -> Result<(), StorageError> {
    let slot = match point {
        Point::Origin => return Err(StorageError::PointNotFound),
        Point::BlockPoint(slot, _) => *slot,
    };
    let snapshot_dir = chain_dep_state_dir(dir);
    fs::create_dir_all(&snapshot_dir)?;
    atomic_write_file(&chain_dep_state_snapshot_path(dir, slot), encoded)?;
    Ok(())
}

/// Loads the newest opaque ChainDepState snapshot with a slot less than or
/// equal to `slot`.
///
/// Returns `Ok(None)` when the sidecar directory is absent or contains no
/// matching snapshot. Invalid filenames are ignored so future migrations can
/// leave non-snapshot files in the directory without breaking startup.
pub fn load_latest_chain_dep_state_snapshot_before_or_at(
    dir: &Path,
    slot: SlotNo,
) -> Result<Option<(SlotNo, Vec<u8>)>, StorageError> {
    let snapshot_dir = chain_dep_state_dir(dir);
    let entries = match fs::read_dir(&snapshot_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let mut best: Option<(SlotNo, PathBuf)> = None;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(candidate_slot) = parse_chain_dep_state_slot(&path) else {
            continue;
        };
        if candidate_slot.0 > slot.0 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_slot, _)| candidate_slot.0 > best_slot.0)
        {
            best = Some((candidate_slot, path));
        }
    }

    match best {
        Some((snapshot_slot, path)) => Ok(Some((snapshot_slot, fs::read(path)?))),
        None => Ok(None),
    }
}

/// Deletes ChainDepState snapshots newer than `point`.
///
/// `Point::Origin` removes every slot-indexed snapshot. For block points, any
/// snapshot with `slot > point.slot` is deleted. The parent directory is
/// synced after deletion so rollback-time truncation is durable.
pub fn truncate_chain_dep_state_snapshots_after(
    dir: &Path,
    point: &Point,
) -> Result<(), StorageError> {
    let snapshot_dir = chain_dep_state_dir(dir);
    let entries = match fs::read_dir(&snapshot_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let keep_after = match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    };
    let mut removed = false;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(candidate_slot) = parse_chain_dep_state_slot(&path) else {
            continue;
        };
        let should_remove = keep_after.is_none_or(|max_slot| candidate_slot.0 > max_slot);
        if should_remove {
            fs::remove_file(path)?;
            removed = true;
        }
    }
    if removed {
        sync_dir(Some(&snapshot_dir))?;
    }
    Ok(())
}

/// Retains only the newest `max_snapshots` ChainDepState snapshots.
///
/// This mirrors ledger-checkpoint retention so the sidecar history does not
/// grow without bound. Non-snapshot files are ignored.
pub fn retain_latest_chain_dep_state_snapshots(
    dir: &Path,
    max_snapshots: usize,
) -> Result<(), StorageError> {
    let snapshot_dir = chain_dep_state_dir(dir);
    let entries = match fs::read_dir(&snapshot_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    let mut snapshots = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if let Some(slot) = parse_chain_dep_state_slot(&path) {
            snapshots.push((slot, path));
        }
    }
    snapshots.sort_by_key(|(slot, _)| std::cmp::Reverse(slot.0));

    let mut removed = false;
    for (_, path) in snapshots.into_iter().skip(max_snapshots) {
        fs::remove_file(path)?;
        removed = true;
    }
    if removed {
        sync_dir(Some(&snapshot_dir))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn chain_dep_state_load_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        assert!(
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(42))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn chain_dep_state_save_and_load_uses_latest_slot_at_or_before_target() {
        let dir = TempDir::new().unwrap();
        let p10 = Point::BlockPoint(SlotNo(10), yggdrasil_ledger::HeaderHash([0x10; 32]));
        let p20 = Point::BlockPoint(SlotNo(20), yggdrasil_ledger::HeaderHash([0x20; 32]));
        let p30 = Point::BlockPoint(SlotNo(30), yggdrasil_ledger::HeaderHash([0x30; 32]));
        save_chain_dep_state_snapshot(dir.path(), &p10, b"slot10").unwrap();
        save_chain_dep_state_snapshot(dir.path(), &p30, b"slot30").unwrap();
        save_chain_dep_state_snapshot(dir.path(), &p20, b"slot20").unwrap();

        let (slot, bytes) =
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(25))
                .unwrap()
                .expect("snapshot before slot 25");
        assert_eq!(slot, SlotNo(20));
        assert_eq!(bytes, b"slot20");

        let (slot, bytes) =
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(30))
                .unwrap()
                .expect("snapshot at slot 30");
        assert_eq!(slot, SlotNo(30));
        assert_eq!(bytes, b"slot30");
    }

    #[test]
    fn chain_dep_state_truncate_after_point_removes_newer_snapshots() {
        let dir = TempDir::new().unwrap();
        for slot in [10, 20, 30] {
            let point =
                Point::BlockPoint(SlotNo(slot), yggdrasil_ledger::HeaderHash([slot as u8; 32]));
            save_chain_dep_state_snapshot(dir.path(), &point, &[slot as u8]).unwrap();
        }

        let rollback = Point::BlockPoint(SlotNo(20), yggdrasil_ledger::HeaderHash([0x20; 32]));
        truncate_chain_dep_state_snapshots_after(dir.path(), &rollback).unwrap();

        let (slot, bytes) =
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(99))
                .unwrap()
                .expect("latest after truncate");
        assert_eq!(slot, SlotNo(20));
        assert_eq!(bytes, vec![20]);

        truncate_chain_dep_state_snapshots_after(dir.path(), &Point::Origin).unwrap();
        assert!(
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(99))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn chain_dep_state_retain_latest_keeps_newest_n() {
        let dir = TempDir::new().unwrap();
        for slot in [10, 20, 30, 40] {
            let point =
                Point::BlockPoint(SlotNo(slot), yggdrasil_ledger::HeaderHash([slot as u8; 32]));
            save_chain_dep_state_snapshot(dir.path(), &point, &[slot as u8]).unwrap();
        }

        retain_latest_chain_dep_state_snapshots(dir.path(), 2).unwrap();
        let (slot, bytes) =
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(99))
                .unwrap()
                .expect("latest retained");
        assert_eq!(slot, SlotNo(40));
        assert_eq!(bytes, vec![40]);
        let (slot, bytes) =
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(35))
                .unwrap()
                .expect("second latest retained");
        assert_eq!(slot, SlotNo(30));
        assert_eq!(bytes, vec![30]);
        assert!(
            load_latest_chain_dep_state_snapshot_before_or_at(dir.path(), SlotNo(25))
                .unwrap()
                .is_none()
        );
    }
}
