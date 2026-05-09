//! Typed configuration surface for the `snapshot-converter` binary.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/ouroboros-consensus/ouroboros-consensus-cardano/app/snapshot-converter.hs.
//!
//! Direct ports of the operator-facing data declarations:
//!
//! - [`SnapshotsDirectory`] — `newtype SnapshotsDirectory = SnapshotsDirectory FilePath`
//!   (re-exported from upstream `Ouroboros.Consensus.Storage.LedgerDB.Snapshots`).
//! - [`LsmDatabaseFilePath`] — `newtype LSMDatabaseFilePath = LSMDatabaseFilePath FilePath`.
//! - [`StandaloneFormat`] — `data StandaloneFormat = Mem | LSM` (only `Mem`
//!   is currently exposed via the CLI; `LSM` is reserved for future
//!   parity if upstream extends the standalone surface).
//! - [`SnapshotsDirectoryWithFormat`] — `data SnapshotsDirectoryWithFormat = LSMSnapshot SnapshotsDirectory LSMDatabaseFilePath`
//!   (used by daemon mode to describe the watched directory + its
//!   database backend).
//! - [`SnapshotSpec`] — `data Snapshot' = StandaloneSnapshot' FilePath StandaloneFormat | LSMSnapshot' FilePath LSMDatabaseFilePath`.
//!   Renamed from the prime-suffixed Haskell name because Rust does
//!   not allow apostrophes in identifiers; the upstream `Snapshot`
//!   (without prime) is a *different* type defined in the LedgerDB
//!   module — see [carve-outs](#carve-outs).
//! - [`Config`] — `data Config = DaemonConfig SnapshotsDirectoryWithFormat SnapshotsDirectory | NoDaemonConfig Snapshot' Snapshot'`.
//!
//! ## Carve-outs (NOT ported, by design)
//!
//! - **Upstream's `Ouroboros.Consensus.Cardano.SnapshotConversion.convertSnapshot`**:
//!   the actual mem↔lsm conversion logic operates on upstream's
//!   ledger-DB on-disk format, which Yggdrasil does not currently
//!   implement (Yggdrasil's `LedgerStore` uses a different on-disk
//!   layout under `data_dir/ledger/`). A future round can either
//!   (a) ship a yggdrasil-format ↔ upstream-mem-format converter
//!   (semantic parity, not byte-format parity); or
//!   (b) implement upstream's LSM/mem readers/writers as a separate
//!   "compat-snapshot" crate that snapshot-converter depends on.
//!   Both paths are tracked under `remaining_work` in
//!   `docs/parity-matrix.json`.
//!
//! - **Upstream's `Snapshot` type** (no prime) from
//!   `Ouroboros.Consensus.Storage.LedgerDB.Snapshots` — combines the
//!   directory-with-format and a parsed snapshot name (slot number).
//!   Yggdrasil's parser-side surface stops at [`SnapshotSpec`]; the
//!   slot-number parsing + Snapshot-typed pairing lands when the
//!   conversion logic is ported.
//!
//! - **Upstream's daemon-mode `withManager`/`watchTree` filesystem
//!   watcher** (from `System.FSNotify`): the daemon mode itself is
//!   port-able, but the file-watching backend will need a Rust
//!   equivalent (likely the `notify` crate). Tracked separately.

use std::path::PathBuf;

/// Path to a directory containing one or more ledger snapshots.
///
/// Upstream: `newtype SnapshotsDirectory = SnapshotsDirectory FilePath`
/// (re-exported from `Ouroboros.Consensus.Storage.LedgerDB.Snapshots`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SnapshotsDirectory(pub PathBuf);

impl SnapshotsDirectory {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        SnapshotsDirectory(path.into())
    }

    /// Borrow the underlying path.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for SnapshotsDirectory {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Path to an LSM database file backing an LSM-format snapshot.
///
/// Upstream: `newtype LSMDatabaseFilePath = LSMDatabaseFilePath FilePath`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LsmDatabaseFilePath(pub PathBuf);

impl LsmDatabaseFilePath {
    /// Construct from any path-like value.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        LsmDatabaseFilePath(path.into())
    }

    /// Borrow the underlying path.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for LsmDatabaseFilePath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

/// Standalone-snapshot format discriminator.
///
/// Upstream: `data StandaloneFormat = Mem | LSM`. Only [`Self::Mem`]
/// is currently exposed via the CLI parser; [`Self::Lsm`] is reserved
/// for future parity if upstream extends the standalone surface (the
/// LSM case currently always pairs with a separate [`LsmDatabaseFilePath`]
/// via [`SnapshotSpec::Lsm`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum StandaloneFormat {
    /// In-memory snapshot format (operator-facing default).
    #[default]
    Mem,
    /// LSM-database snapshot format (reserved; not currently CLI-reachable).
    Lsm,
}

/// A snapshots directory paired with the database backend it contains.
///
/// Upstream: `data SnapshotsDirectoryWithFormat = LSMSnapshot SnapshotsDirectory LSMDatabaseFilePath`.
/// Currently only the LSM case exists upstream; the variant naming is
/// preserved for future parity if upstream adds an `MemSnapshot` arm.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SnapshotsDirectoryWithFormat {
    /// An LSM-format snapshots directory + its database file.
    LsmSnapshot {
        /// Directory containing the snapshot's metadata + content.
        directory: SnapshotsDirectory,
        /// Path to the LSM database backing the snapshot.
        database: LsmDatabaseFilePath,
    },
}

/// A specific snapshot (path + format) parsed from CLI flags.
///
/// Upstream: `data Snapshot' = StandaloneSnapshot' FilePath StandaloneFormat | LSMSnapshot' FilePath LSMDatabaseFilePath`.
/// Renamed from `Snapshot'` (with prime) because Rust does not allow
/// apostrophes in identifiers; see the module-level docstring for
/// the upstream-`Snapshot`-vs-`Snapshot'` distinction.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SnapshotSpec {
    /// A standalone-format snapshot at the given path.
    Standalone {
        /// Snapshot directory (named after the slot number it captures;
        /// e.g. `100_xyz`).
        path: PathBuf,
        /// In-memory or LSM format (currently only `Mem` is CLI-reachable).
        format: StandaloneFormat,
    },
    /// An LSM-format snapshot at the given path with a separate database.
    Lsm {
        /// Snapshot directory.
        path: PathBuf,
        /// Path to the LSM database backing the snapshot.
        database: LsmDatabaseFilePath,
    },
}

impl SnapshotSpec {
    /// Construct a standalone-format snapshot spec.
    pub fn standalone(path: impl Into<PathBuf>, format: StandaloneFormat) -> Self {
        SnapshotSpec::Standalone {
            path: path.into(),
            format,
        }
    }

    /// Construct an LSM-format snapshot spec.
    pub fn lsm(path: impl Into<PathBuf>, database: LsmDatabaseFilePath) -> Self {
        SnapshotSpec::Lsm {
            path: path.into(),
            database,
        }
    }

    /// Borrow the underlying snapshot path (independent of format).
    pub fn path(&self) -> &std::path::Path {
        match self {
            SnapshotSpec::Standalone { path, .. } | SnapshotSpec::Lsm { path, .. } => path,
        }
    }
}

/// Top-level operator-supplied configuration for `snapshot-converter`.
///
/// Upstream:
/// ```haskell
/// data Config
///   = DaemonConfig SnapshotsDirectoryWithFormat SnapshotsDirectory
///   | NoDaemonConfig Snapshot' Snapshot'
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Config {
    /// Run as a daemon: watch the input directory and write converted
    /// snapshots to the output directory as new ones arrive.
    Daemon {
        /// Directory + database to watch.
        watch: SnapshotsDirectoryWithFormat,
        /// Output directory for converted snapshots (Mem-format only).
        output: SnapshotsDirectory,
    },
    /// Run once: convert the supplied input snapshot to the supplied
    /// output snapshot and exit.
    Oneshot {
        /// Input snapshot spec.
        input: SnapshotSpec,
        /// Output snapshot spec.
        output: SnapshotSpec,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_directory_round_trip() {
        let dir = SnapshotsDirectory::new("/var/lib/cardano/snapshots");
        assert_eq!(dir.as_path().to_str(), Some("/var/lib/cardano/snapshots"));
    }

    #[test]
    fn lsm_database_file_path_round_trip() {
        let path = LsmDatabaseFilePath::new("/var/lib/cardano/lsm.db");
        assert_eq!(path.as_path().to_str(), Some("/var/lib/cardano/lsm.db"));
    }

    #[test]
    fn standalone_format_default_is_mem() {
        assert_eq!(StandaloneFormat::default(), StandaloneFormat::Mem);
    }

    #[test]
    fn snapshots_directory_with_format_lsm_round_trip() {
        let dir = SnapshotsDirectoryWithFormat::LsmSnapshot {
            directory: SnapshotsDirectory::new("/snapshots"),
            database: LsmDatabaseFilePath::new("/lsm.db"),
        };
        match dir {
            SnapshotsDirectoryWithFormat::LsmSnapshot {
                directory,
                database,
            } => {
                assert_eq!(directory.as_path().to_str(), Some("/snapshots"));
                assert_eq!(database.as_path().to_str(), Some("/lsm.db"));
            }
        }
    }

    #[test]
    fn snapshot_spec_standalone_constructor() {
        let spec = SnapshotSpec::standalone("/snapshots/100_xyz", StandaloneFormat::Mem);
        match spec {
            SnapshotSpec::Standalone { path, format } => {
                assert_eq!(path.to_str(), Some("/snapshots/100_xyz"));
                assert_eq!(format, StandaloneFormat::Mem);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn snapshot_spec_lsm_constructor() {
        let spec = SnapshotSpec::lsm("/snapshots/100_xyz", LsmDatabaseFilePath::new("/lsm.db"));
        match spec {
            SnapshotSpec::Lsm { path, database } => {
                assert_eq!(path.to_str(), Some("/snapshots/100_xyz"));
                assert_eq!(database.as_path().to_str(), Some("/lsm.db"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn snapshot_spec_path_accessor_works_for_both_variants() {
        let s = SnapshotSpec::standalone("/a", StandaloneFormat::Mem);
        let l = SnapshotSpec::lsm("/b", LsmDatabaseFilePath::new("/db"));
        assert_eq!(s.path().to_str(), Some("/a"));
        assert_eq!(l.path().to_str(), Some("/b"));
    }

    #[test]
    fn config_daemon_round_trip() {
        let cfg = Config::Daemon {
            watch: SnapshotsDirectoryWithFormat::LsmSnapshot {
                directory: SnapshotsDirectory::new("/in"),
                database: LsmDatabaseFilePath::new("/in.db"),
            },
            output: SnapshotsDirectory::new("/out"),
        };
        match cfg {
            Config::Daemon {
                watch:
                    SnapshotsDirectoryWithFormat::LsmSnapshot {
                        directory: ref _d, ..
                    },
                output: ref _o,
            } => {}
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn config_oneshot_round_trip() {
        let cfg = Config::Oneshot {
            input: SnapshotSpec::standalone("/in", StandaloneFormat::Mem),
            output: SnapshotSpec::lsm("/out", LsmDatabaseFilePath::new("/out.db")),
        };
        match cfg {
            Config::Oneshot { input, output } => {
                assert_eq!(input.path().to_str(), Some("/in"));
                assert_eq!(output.path().to_str(), Some("/out"));
            }
            _ => panic!("wrong variant"),
        }
    }
}
