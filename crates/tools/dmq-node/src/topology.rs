//! DMQ topology-file configuration.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/Configuration/Topology.hs.
//!
//! Upstream `readTopologyFile` parses a `NetworkTopology NoExtraConfig
//! NoExtraFlags` — the standard `ouroboros-network` topology with the
//! "no extra" instantiation. yggdrasil reuses `crates/network`'s
//! concrete [`TopologyConfig`]; the `NoExtraConfig` / `NoExtraFlags`
//! type parameters carry no data and are dropped.

use std::path::Path;

use yggdrasil_network::TopologyConfig;

/// An error reading or parsing a DMQ topology file.
///
/// Mirror of upstream `readTopologyFile`'s `Either Text` failure
/// modes — an I/O error or a JSON decode error.
#[derive(Debug, thiserror::Error)]
pub enum TopologyError {
    /// The topology file could not be read.
    #[error("DMQ.Topology.readTopologyFile: {0}")]
    Io(#[from] std::io::Error),
    /// The topology file was not a valid topology JSON document.
    #[error("topology parsing error:\n{0}")]
    Parse(String),
}

/// Read the network [`TopologyConfig`] from a DMQ topology file.
///
/// Mirror of upstream `readTopologyFile` — reads the file and decodes
/// its JSON; an I/O or decode failure is a [`TopologyError`].
pub fn read_topology_file(path: &Path) -> Result<TopologyConfig, TopologyError> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|err| TopologyError::Parse(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("dmq-topology-{}-{}.json", std::process::id(), tag))
    }

    #[test]
    fn read_topology_file_missing_returns_io_error() {
        let err = read_topology_file(&temp_path("missing")).expect_err("missing file");
        assert!(matches!(err, TopologyError::Io(_)), "got: {err:?}");
    }

    #[test]
    fn read_topology_file_parses_a_valid_topology() {
        let path = temp_path("valid");
        std::fs::write(&path, br#"{"localRoots":[],"publicRoots":[]}"#).unwrap();
        let topology = read_topology_file(&path).expect("valid topology");
        assert!(topology.local_roots.is_empty());
        assert!(topology.public_roots.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_topology_file_rejects_malformed_json() {
        let path = temp_path("malformed");
        std::fs::write(&path, b"not json at all").unwrap();
        let err = read_topology_file(&path).expect_err("malformed json");
        assert!(matches!(err, TopologyError::Parse(_)), "got: {err:?}");
        let _ = std::fs::remove_file(&path);
    }
}
