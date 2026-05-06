//! Config-relative path resolution.
//!
//! Both helpers translate a path string supplied by the operator
//! through the file config (e.g. `ShelleyGenesisFile = "shelley-genesis.json"`)
//! into an absolute filesystem path: absolute inputs pass through
//! unchanged, relative inputs resolve against the directory holding
//! the config file (when known) or the process working directory.
//!
//! Mirrors upstream `Cardano.Node.Configuration.NodeAddress` /
//! `Cardano.Node.Configuration.POM.fromConfigPath` — the upstream
//! variant uses the `FilePath` newtype + `mkConfigBaseDir`; Yggdrasil
//! works with `&Path` directly.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Configuration/NodeAddress.hs>

use std::path::{Path, PathBuf};

/// Resolve a `storage_dir` config field. Identical semantics to
/// [`resolve_config_path`] but kept as a separate name so call-site
/// intent (`storage_dir` vs. arbitrary `*File` fields) is obvious.
pub(crate) fn resolve_storage_dir(storage_dir: &Path, config_base_dir: Option<&Path>) -> PathBuf {
    if storage_dir.is_absolute() {
        storage_dir.to_path_buf()
    } else if let Some(base_dir) = config_base_dir {
        base_dir.join(storage_dir)
    } else {
        storage_dir.to_path_buf()
    }
}

/// Resolve a generic config-supplied path against the directory
/// holding the config file. Used for `ShelleyGenesisFile`,
/// `AlonzoGenesisFile`, `TopologyFilePath`, KES key paths, peer
/// snapshot paths, the checkpoints file, etc.
pub(crate) fn resolve_config_path(path: &Path, config_base_dir: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base_dir) = config_base_dir {
        base_dir.join(path)
    } else {
        path.to_path_buf()
    }
}
