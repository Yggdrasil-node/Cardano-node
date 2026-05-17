//! yggdrasil-test-fixtures — shared test scaffolding.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis crate that
//! consolidates fixtures and helpers used across `crates/node/yggdrasil-node/tests/`,
//! `crates/node/runtime/src/tests.rs`, and the per-crate `tests/` trees.
//! Upstream `cardano-node` distributes equivalent helpers across
//! `Test.ThreadNet.*`, `Test.Util.*`, and per-package `test/` trees;
//! Yggdrasil pulls the Rust-side equivalents into one crate so a
//! Wave 5 sub-crate's tests don't have to re-implement `tmp_chaindb`
//! or `make_test_peer` from scratch.
//!
//! **Wave 2 status:** scaffold only. The crate ships the API shape;
//! Wave 5 sub-crate extraction PRs migrate their tests to depend on
//! this crate as a `dev-dependency` and delete the local copies.
//! The helpers below are deliberately minimal — anything heavier
//! (CBOR fixture loaders, mock peers, network-simulation harness)
//! gets added when the consuming test actually needs it.

#![cfg_attr(test, allow(clippy::unwrap_used))]

use core::sync::atomic::{AtomicU64, Ordering};
use std::path::{Path, PathBuf};

/// A temporary directory that is cleaned up on `Drop`. Use this
/// instead of `tempfile` (not in workspace deps) so the crate stays
/// std-only.
///
/// Path layout: `$TMPDIR/yggdrasil-test-<pid>-<counter>`.
/// The counter avoids collisions when several tests in the same
/// process create dirs back-to-back.
pub struct TmpDir {
    path: PathBuf,
}

impl TmpDir {
    /// Create a new uniquely-named directory under `$TMPDIR`.
    pub fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "yggdrasil-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst),
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    /// Borrow the on-disk path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        // Best-effort: a leaked test dir is not worth panicking in `Drop` over.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a temporary directory laid out for a fresh `ChainDb`
/// (`immutable/`, `volatile/`, `ledger/`). Returns the owning
/// `TmpDir`; let it drop to clean up.
///
/// Wave 5 PR 9 (`yggdrasil-node-sync` extraction) and Wave 5 PR 10
/// (mempool / block-producer / plutus-eval extraction) consumers
/// can call this from their `dev-dependencies` rather than each
/// re-implementing the layout.
pub fn tmp_chaindb() -> std::io::Result<TmpDir> {
    let dir = TmpDir::new()?;
    for sub in ["immutable", "volatile", "ledger"] {
        std::fs::create_dir_all(dir.path().join(sub))?;
    }
    Ok(dir)
}

/// Construct a stable bogus peer address suitable for tests that
/// need to populate routing / topology fixtures without actually
/// binding a socket. Returns `127.0.0.1:30000+index` so addresses
/// are distinct per call and never collide with the standard NtN
/// (3001) / NtC (12798) / preview-producer (19002) ports.
pub fn make_test_peer(index: u8) -> std::net::SocketAddr {
    std::net::SocketAddr::from(([127, 0, 0, 1], 30_000_u16 + u16::from(index)))
}

/// Assert byte-for-byte equality between two slices, with a
/// diagnostic hex dump on failure. Wave 5–6 byte-equivalence
/// parity tests use this as the single assertion site so the
/// failure diagnostic is uniform across crates.
#[track_caller]
pub fn assert_bytes_eq(label: &str, actual: &[u8], expected: &[u8]) {
    if actual != expected {
        panic!(
            "byte-equivalence failure ({label}):\n  actual   ({}B): {}\n  expected ({}B): {}",
            actual.len(),
            hex_dump(actual),
            expected.len(),
            hex_dump(expected),
        );
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    use core::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
            let _ = write!(acc, "{:02x}", b);
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmpdir_creates_and_drops() {
        let p = {
            let d = TmpDir::new().unwrap();
            let p = d.path().to_path_buf();
            assert!(p.exists());
            p
        };
        assert!(!p.exists());
    }

    #[test]
    fn tmp_chaindb_layout() {
        let d = tmp_chaindb().unwrap();
        assert!(d.path().join("immutable").is_dir());
        assert!(d.path().join("volatile").is_dir());
        assert!(d.path().join("ledger").is_dir());
    }

    #[test]
    fn test_peer_addresses_distinct() {
        let a = make_test_peer(0);
        let b = make_test_peer(1);
        assert_ne!(a.port(), b.port());
        assert_eq!(a.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn assert_bytes_eq_passes_for_equal() {
        assert_bytes_eq("equal", b"\xab\xcd", b"\xab\xcd");
    }

    #[test]
    #[should_panic(expected = "byte-equivalence failure")]
    fn assert_bytes_eq_panics_on_diff() {
        assert_bytes_eq("diff", b"\xab\xcd", b"\xab\x00");
    }
}
