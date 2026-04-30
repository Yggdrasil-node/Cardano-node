//! Sidecar persistence for opaque, slot-independent state files.
//!
//! The primary use today is the OpCert counter map (`PraosState.csCounters`
//! in upstream `Ouroboros.Consensus.Protocol.Praos`), whose monotonicity
//! invariant (per-pool sequence number `n` must satisfy `stored ≤ n ≤ stored
//! + 1`) only holds across restarts if the high-water marks are persisted.
//! Without that, every restart resets the counters to zero, and a malicious
//! peer can replay an old block whose OpCert sequence number is below the
//! true on-chain value.
//!
//! This module exposes a tiny atomic-write/read helper pair plus the
//! standard `ocert_counters.cbor` filename used by [`save_ocert_counters`]
//! and [`load_ocert_counters`]. The bytes themselves are opaque to storage:
//! the consensus crate owns the CBOR codec for `OcertCounters`. Keeping the
//! layout opaque mirrors the way `LedgerStore` stores ledger snapshots as
//! raw bytes while `LedgerStateCheckpoint` owns the typed encoding.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::StorageError;

/// Filename used for the OpCert counter sidecar inside the storage dir.
pub const OCERT_COUNTERS_FILENAME: &str = "ocert_counters.cbor";

/// Filename used for the Praos nonce-evolution sidecar (R197).
pub const NONCE_STATE_FILENAME: &str = "nonce_state.cbor";

fn sidecar_path(dir: &Path) -> PathBuf {
    dir.join(OCERT_COUNTERS_FILENAME)
}

fn nonce_sidecar_path(dir: &Path) -> PathBuf {
    dir.join(NONCE_STATE_FILENAME)
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

/// Atomically writes the encoded OpCert counter sidecar to `<dir>/ocert_counters.cbor`.
///
/// Creates `dir` if it does not exist. The write is atomic: bytes are first
/// written to `ocert_counters.tmp` (with `sync_all()`) and then renamed into
/// place, after which the parent directory is synced so the rename is
/// durable. This matches the same atomic-write contract used by
/// `FileLedgerStore::save_snapshot`, and mirrors the upstream `LedgerDB`
/// snapshot-write durability discipline in `Ouroboros.Consensus.Storage.LedgerDB`.
pub fn save_ocert_counters(dir: &Path, encoded: &[u8]) -> Result<(), StorageError> {
    fs::create_dir_all(dir)?;
    atomic_write_file(&sidecar_path(dir), encoded)?;
    Ok(())
}

/// Loads the OpCert counter sidecar from `<dir>/ocert_counters.cbor`.
///
/// Returns `Ok(None)` when the file does not exist (fresh node, never
/// persisted). Returns `Ok(Some(bytes))` when the file is present and
/// readable; the caller (consensus crate) is responsible for CBOR-decoding.
/// Other I/O errors (corrupted disk, permission denied) propagate as
/// [`StorageError::Io`].
pub fn load_ocert_counters(dir: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let path = sidecar_path(dir);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// R197 — atomically writes the encoded `NonceEvolutionState`
/// sidecar to `<dir>/nonce_state.cbor`.  Same atomic-write
/// contract as [`save_ocert_counters`].
pub fn save_nonce_state(dir: &Path, encoded: &[u8]) -> Result<(), StorageError> {
    fs::create_dir_all(dir)?;
    atomic_write_file(&nonce_sidecar_path(dir), encoded)?;
    Ok(())
}

/// R197 — loads the `NonceEvolutionState` sidecar from
/// `<dir>/nonce_state.cbor`.  Returns `Ok(None)` when the file
/// does not exist (fresh node, never persisted).
pub fn load_nonce_state(dir: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let path = nonce_sidecar_path(dir);
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_returns_none_when_sidecar_missing() {
        let dir = TempDir::new().unwrap();
        assert!(load_ocert_counters(dir.path()).unwrap().is_none());
    }

    #[test]
    fn save_then_load_round_trips_bytes() {
        let dir = TempDir::new().unwrap();
        let payload = vec![0xa1, 0x58, 28, 0x42, 0x42, 0x42, 0x07];
        save_ocert_counters(dir.path(), &payload).unwrap();
        let loaded = load_ocert_counters(dir.path()).unwrap();
        assert_eq!(loaded.as_deref(), Some(payload.as_slice()));
    }

    #[test]
    fn save_overwrites_existing_sidecar() {
        let dir = TempDir::new().unwrap();
        save_ocert_counters(dir.path(), b"old contents").unwrap();
        save_ocert_counters(dir.path(), b"new contents").unwrap();
        let loaded = load_ocert_counters(dir.path()).unwrap();
        assert_eq!(loaded.as_deref(), Some(b"new contents".as_slice()));
    }

    #[test]
    fn save_creates_storage_dir_if_missing() {
        let parent = TempDir::new().unwrap();
        let nested = parent.path().join("does/not/exist/yet");
        save_ocert_counters(&nested, b"hello").unwrap();
        let loaded = load_ocert_counters(&nested).unwrap();
        assert_eq!(loaded.as_deref(), Some(b"hello".as_slice()));
    }
}
