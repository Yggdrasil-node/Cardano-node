//! `cardano-testnet` output directory discovery for `tx-generator`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Setup/TestnetDiscovery.hs`.
//! Ports `TestnetConfig`, `discoverTestnetConfig`, `discoverNodes`,
//! `parseNodeIndex`, `readNodeDescription`, `mkLocalhostAddr`,
//! `mergeValues`, and `validateFileExists`. The upstream function
//! returns `NixServiceOptions`; this Rust slice returns the merged JSON
//! value until the later `NixServiceOptions` mirror lands.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Location of a `cardano-testnet` output directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestnetConfig {
    /// Output directory path, mirroring upstream `tcDir`.
    pub tc_dir: PathBuf,
}

/// Mirror of upstream `NodeDescription` JSON shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeDescription {
    /// Node alias, for example `node1`.
    pub name: String,
    /// IPv4 address. `cardano-testnet` currently emits localhost nodes.
    pub addr: String,
    /// Node-to-node port.
    pub port: u16,
}

/// Errors from testnet discovery.
#[derive(Debug, thiserror::Error)]
pub enum TestnetDiscoveryError {
    /// Required path is missing.
    #[error("{message}: {path}")]
    MissingPath {
        /// Error message matching upstream prefixes.
        message: String,
        /// Missing path.
        path: PathBuf,
    },
    /// Filesystem operation failed.
    #[error("{context}: {path}: {source}")]
    Io {
        /// Error context.
        context: &'static str,
        /// Path being accessed.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A port file did not contain a valid `PortNumber`.
    #[error("readNodeDescription: invalid port number in: {0}")]
    InvalidPort(PathBuf),
    /// No nodes were discovered.
    #[error("discoverNodes: no nodes found in: {0}")]
    NoNodes(PathBuf),
}

/// Discover testnet connection settings and merge them over user JSON.
pub fn discover_testnet_config(
    config: &TestnetConfig,
    user_config: Value,
) -> Result<Value, TestnetDiscoveryError> {
    if !config.tc_dir.is_dir() {
        return Err(TestnetDiscoveryError::MissingPath {
            message: "discoverTestnetConfig: testnet directory does not exist".to_string(),
            path: config.tc_dir.clone(),
        });
    }

    let target_nodes = discover_nodes(&config.tc_dir)?;
    let socket_path = config.tc_dir.join(default_socket_path(1));
    let sig_key_path = config.tc_dir.join(default_utxo_skey_path(1));
    let config_path = config.tc_dir.join(default_config_file());

    validate_file_exists(&socket_path, "socket")?;
    validate_file_exists(&sig_key_path, "signing key")?;
    validate_file_exists(&config_path, "configuration")?;

    let connection_settings = json!({
        "localNodeSocketPath": path_to_json_string(&socket_path),
        "sigKey": path_to_json_string(&sig_key_path),
        "nodeConfigFile": path_to_json_string(&config_path),
        "targetNodes": target_nodes,
    });

    Ok(merge_values(user_config, connection_settings))
}

/// Discover nodes by scanning for port files in the testnet directory.
pub fn discover_nodes(dir: &Path) -> Result<Vec<NodeDescription>, TestnetDiscoveryError> {
    let node_data_dir = dir.join("node-data");
    if !node_data_dir.is_dir() {
        return Err(TestnetDiscoveryError::MissingPath {
            message: "discoverNodes: node data directory does not exist".to_string(),
            path: node_data_dir,
        });
    }

    let entries = fs::read_dir(&node_data_dir).map_err(|source| TestnetDiscoveryError::Io {
        context: "discoverNodes: failed to list node data directory",
        path: node_data_dir.clone(),
        source,
    })?;
    let mut node_indices = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| parse_node_index(&name))
        .collect::<Vec<_>>();
    node_indices.sort_unstable();

    let nodes = node_indices
        .into_iter()
        .map(|idx| read_node_description(dir, idx))
        .collect::<Result<Vec<_>, _>>()?;

    if nodes.is_empty() {
        return Err(TestnetDiscoveryError::NoNodes(node_data_dir));
    }
    Ok(nodes)
}

/// Parse a node index from a directory name like `node1`, `node2`, etc.
pub fn parse_node_index(name: &str) -> Option<usize> {
    name.strip_prefix("node").and_then(|tail| {
        tail.chars()
            .skip_while(|ch| !ch.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()
    })
}

/// Read a node description from its port file.
pub fn read_node_description(
    dir: &Path,
    idx: usize,
) -> Result<NodeDescription, TestnetDiscoveryError> {
    let port_path = dir.join(default_port_file(idx));
    validate_file_exists(&port_path, "port file")?;
    let port_str = fs::read_to_string(&port_path).map_err(|source| TestnetDiscoveryError::Io {
        context: "readNodeDescription: failed to read port file",
        path: port_path.clone(),
        source,
    })?;
    let port = port_str
        .trim()
        .parse::<u16>()
        .map_err(|_| TestnetDiscoveryError::InvalidPort(port_path))?;

    Ok(NodeDescription {
        name: default_node_name(idx),
        addr: "127.0.0.1".to_string(),
        port,
    })
}

/// Deep merge two JSON values. Objects merge recursively; otherwise override wins.
pub fn merge_values(base: Value, override_value: Value) -> Value {
    match (base, override_value) {
        (Value::Object(base), Value::Object(override_map)) => {
            Value::Object(merge_maps(base, override_map))
        }
        (_, override_value) => override_value,
    }
}

fn merge_maps(
    mut base: Map<String, Value>,
    override_map: Map<String, Value>,
) -> Map<String, Value> {
    for (key, override_value) in override_map {
        let value = match base.remove(&key) {
            Some(base_value) => merge_values(base_value, override_value),
            None => override_value,
        };
        base.insert(key, value);
    }
    base
}

/// Validate that a path exists (file, socket, etc.).
pub fn validate_file_exists(
    path: &Path,
    description: &'static str,
) -> Result<(), TestnetDiscoveryError> {
    if path.exists() {
        Ok(())
    } else {
        Err(TestnetDiscoveryError::MissingPath {
            message: format!("validateFileExists: required {description} file not found"),
            path: path.to_path_buf(),
        })
    }
}

/// Shared path convention: `nodeN`.
pub fn default_node_name(n: usize) -> String {
    format!("node{n}")
}

/// Shared path convention: `node-data/nodeN`.
pub fn default_node_data_dir(n: usize) -> PathBuf {
    PathBuf::from("node-data").join(default_node_name(n))
}

/// Shared path convention: `utxo-keys/utxoN/utxo.skey`.
pub fn default_utxo_skey_path(n: usize) -> PathBuf {
    PathBuf::from("utxo-keys")
        .join(format!("utxo{n}"))
        .join("utxo.skey")
}

/// Shared path convention: `socket/nodeN/sock`.
pub fn default_socket_path(n: usize) -> PathBuf {
    PathBuf::from("socket")
        .join(default_node_name(n))
        .join("sock")
}

/// Shared path convention: `configuration.yaml`.
pub fn default_config_file() -> PathBuf {
    PathBuf::from("configuration.yaml")
}

/// Shared path convention: `node-data/nodeN/port`.
pub fn default_port_file(n: usize) -> PathBuf {
    default_node_data_dir(n).join("port")
}

fn path_to_json_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "yggdrasil-tx-generator-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn setup_node_dir(root: &Path, idx: usize, port: u16) {
        let port_path = root.join(default_port_file(idx));
        fs::create_dir_all(port_path.parent().expect("port has parent")).expect("create node dir");
        fs::write(port_path, port.to_string()).expect("write port");
    }

    fn setup_mock_testnet(root: &Path) {
        for (idx, port) in [(1, 30_001), (3, 30_003), (2, 30_002)] {
            setup_node_dir(root, idx, port);
        }
        let socket_path = root.join(default_socket_path(1));
        fs::create_dir_all(socket_path.parent().expect("socket has parent")).expect("socket dir");
        fs::write(socket_path, "").expect("socket marker");

        let sig_key_path = root.join(default_utxo_skey_path(1));
        fs::create_dir_all(sig_key_path.parent().expect("sig key has parent")).expect("key dir");
        fs::write(sig_key_path, "{}").expect("sig key");

        fs::write(root.join(default_config_file()), "Protocol: Cardano\n").expect("config");
    }

    #[test]
    fn path_conventions_match_cardano_testnet_paths() {
        assert_eq!(default_node_name(2), "node2");
        assert_eq!(default_node_data_dir(2), PathBuf::from("node-data/node2"));
        assert_eq!(
            default_utxo_skey_path(1),
            PathBuf::from("utxo-keys/utxo1/utxo.skey")
        );
        assert_eq!(default_socket_path(1), PathBuf::from("socket/node1/sock"));
        assert_eq!(default_config_file(), PathBuf::from("configuration.yaml"));
        assert_eq!(default_port_file(3), PathBuf::from("node-data/node3/port"));
    }

    #[test]
    fn parse_node_index_matches_upstream_prefix_rule() {
        assert_eq!(parse_node_index("node1"), Some(1));
        assert_eq!(parse_node_index("node001"), Some(1));
        assert_eq!(parse_node_index("node-alpha2"), Some(2));
        assert_eq!(parse_node_index("relay1"), None);
        assert_eq!(parse_node_index("node"), None);
    }

    #[test]
    fn discover_nodes_sorts_by_node_index_and_reads_ports() {
        let temp = TempDir::new("discover-nodes");
        setup_mock_testnet(temp.path());

        let nodes = discover_nodes(temp.path()).expect("discovers nodes");
        assert_eq!(
            nodes,
            vec![
                NodeDescription {
                    name: "node1".to_string(),
                    addr: "127.0.0.1".to_string(),
                    port: 30_001,
                },
                NodeDescription {
                    name: "node2".to_string(),
                    addr: "127.0.0.1".to_string(),
                    port: 30_002,
                },
                NodeDescription {
                    name: "node3".to_string(),
                    addr: "127.0.0.1".to_string(),
                    port: 30_003,
                },
            ]
        );
    }

    #[test]
    fn discover_testnet_config_overrides_connection_settings() {
        let temp = TempDir::new("discover-config");
        setup_mock_testnet(temp.path());
        let user_config = json!({
            "debugMode": false,
            "nested": { "keep": true, "override": "user" },
            "localNodeSocketPath": "user-socket",
            "sigKey": "user-key",
            "nodeConfigFile": "user-config",
            "targetNodes": [{ "name": "remote", "addr": "10.0.0.1", "port": 1 }],
        });

        let merged = discover_testnet_config(
            &TestnetConfig {
                tc_dir: temp.path().to_path_buf(),
            },
            user_config,
        )
        .expect("discovers config");

        assert_eq!(merged["debugMode"], json!(false));
        assert_eq!(
            merged["nested"],
            json!({ "keep": true, "override": "user" })
        );
        assert!(
            merged["localNodeSocketPath"]
                .as_str()
                .expect("socket string")
                .ends_with("socket\\node1\\sock")
                || merged["localNodeSocketPath"]
                    .as_str()
                    .expect("socket string")
                    .ends_with("socket/node1/sock")
        );
        assert!(
            merged["sigKey"]
                .as_str()
                .expect("sig key string")
                .ends_with("utxo-keys\\utxo1\\utxo.skey")
                || merged["sigKey"]
                    .as_str()
                    .expect("sig key string")
                    .ends_with("utxo-keys/utxo1/utxo.skey")
        );
        assert!(
            merged["nodeConfigFile"]
                .as_str()
                .expect("config string")
                .ends_with("configuration.yaml")
        );
        assert_eq!(
            merged["targetNodes"],
            json!([
                { "name": "node1", "addr": "127.0.0.1", "port": 30001 },
                { "name": "node2", "addr": "127.0.0.1", "port": 30002 },
                { "name": "node3", "addr": "127.0.0.1", "port": 30003 },
            ])
        );
    }

    #[test]
    fn merge_values_deep_merges_objects_and_override_wins() {
        let base = json!({
            "a": 1,
            "nested": { "left": true, "same": "base" },
            "scalar": { "will": "drop" }
        });
        let override_value = json!({
            "b": 2,
            "nested": { "right": true, "same": "override" },
            "scalar": 9
        });

        assert_eq!(
            merge_values(base, override_value),
            json!({
                "a": 1,
                "b": 2,
                "nested": { "left": true, "right": true, "same": "override" },
                "scalar": 9
            })
        );
    }

    #[test]
    fn missing_testnet_directory_is_reported() {
        let temp = TempDir::new("missing-dir");
        let missing = temp.path().join("absent");
        let err = discover_testnet_config(&TestnetConfig { tc_dir: missing }, json!({}))
            .expect_err("missing dir fails");
        assert!(err.to_string().contains("testnet directory does not exist"));
    }

    #[test]
    fn invalid_port_is_reported() {
        let temp = TempDir::new("invalid-port");
        let port_path = temp.path().join(default_port_file(1));
        fs::create_dir_all(port_path.parent().expect("port has parent")).expect("node dir");
        fs::write(&port_path, "not-a-port").expect("write port");

        let err = read_node_description(temp.path(), 1).expect_err("invalid port fails");
        assert!(err.to_string().contains("invalid port number"));
    }
}
