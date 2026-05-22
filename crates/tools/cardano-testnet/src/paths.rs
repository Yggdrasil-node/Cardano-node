//! Shared cardano-testnet output-directory path conventions.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of upstream
//! `cardano-node/src/Cardano/Node/Testnet/Paths.hs`. That module
//! (`Cardano.Node.Testnet.Paths`) is a `cardano-node` module shared
//! by `cardano-testnet` (the producer) and consumers of generated
//! testnet configurations; yggdrasil places it in the cardano-testnet
//! crate — its primary consumer — as pure path conventions. The
//! `<n>` node index is `i32`, matching `types::NodeId`.

use std::path::PathBuf;

/// The name of a testnet node — `node<n>`.
///
/// Mirror of upstream `defaultNodeName n = "node" <> show n`.
pub fn default_node_name(n: i32) -> String {
    format!("node{n}")
}

/// Relative path to a node's data directory — `node-data/node<n>`.
///
/// Mirror of upstream `defaultNodeDataDir`.
pub fn default_node_data_dir(n: i32) -> PathBuf {
    PathBuf::from("node-data").join(default_node_name(n))
}

/// Relative path to a UTxO key directory — `utxo-keys/utxo<n>`.
///
/// Mirror of upstream `defaultUtxoKeyDir`.
pub fn default_utxo_key_dir(n: i32) -> PathBuf {
    PathBuf::from("utxo-keys").join(format!("utxo{n}"))
}

/// Relative path to a UTxO signing key — `<utxo-key-dir>/utxo.skey`.
///
/// Mirror of upstream `defaultUtxoSKeyPath`.
pub fn default_utxo_skey_path(n: i32) -> PathBuf {
    default_utxo_key_dir(n).join("utxo.skey")
}

/// Relative path to a UTxO verification key —
/// `<utxo-key-dir>/utxo.vkey`.
///
/// Mirror of upstream `defaultUtxoVKeyPath`.
pub fn default_utxo_vkey_path(n: i32) -> PathBuf {
    default_utxo_key_dir(n).join("utxo.vkey")
}

/// Relative path to a UTxO address file — `<utxo-key-dir>/utxo.addr`.
///
/// Mirror of upstream `defaultUtxoAddrPath`.
pub fn default_utxo_addr_path(n: i32) -> PathBuf {
    default_utxo_key_dir(n).join("utxo.addr")
}

/// The socket-directory name. Mirror of upstream `defaultSocketDir`.
pub const DEFAULT_SOCKET_DIR: &str = "socket";

/// The socket file name. Mirror of upstream `defaultSocketName`.
pub const DEFAULT_SOCKET_NAME: &str = "sock";

/// Relative path to a node's socket — `socket/node<n>/sock`.
///
/// Mirror of upstream `defaultSocketPath`.
pub fn default_socket_path(n: i32) -> PathBuf {
    PathBuf::from(DEFAULT_SOCKET_DIR)
        .join(default_node_name(n))
        .join(DEFAULT_SOCKET_NAME)
}

/// The main node-configuration file name. Mirror of upstream
/// `defaultConfigFile`.
pub const DEFAULT_CONFIG_FILE: &str = "configuration.yaml";

/// Relative path to a node's port file — `<node-data-dir>/port`.
///
/// Mirror of upstream `defaultPortFile`.
pub fn default_port_file(n: i32) -> PathBuf {
    default_node_data_dir(n).join("port")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_name_and_data_dir() {
        assert_eq!(default_node_name(0), "node0");
        assert_eq!(default_node_name(3), "node3");
        assert_eq!(default_node_data_dir(2), PathBuf::from("node-data/node2"));
    }

    #[test]
    fn utxo_key_paths() {
        assert_eq!(default_utxo_key_dir(1), PathBuf::from("utxo-keys/utxo1"));
        assert_eq!(
            default_utxo_skey_path(1),
            PathBuf::from("utxo-keys/utxo1/utxo.skey")
        );
        assert_eq!(
            default_utxo_vkey_path(1),
            PathBuf::from("utxo-keys/utxo1/utxo.vkey")
        );
        assert_eq!(
            default_utxo_addr_path(1),
            PathBuf::from("utxo-keys/utxo1/utxo.addr")
        );
    }

    #[test]
    fn socket_path_is_dir_node_name() {
        assert_eq!(DEFAULT_SOCKET_DIR, "socket");
        assert_eq!(DEFAULT_SOCKET_NAME, "sock");
        assert_eq!(default_socket_path(4), PathBuf::from("socket/node4/sock"));
    }

    #[test]
    fn config_file_and_port_file() {
        assert_eq!(DEFAULT_CONFIG_FILE, "configuration.yaml");
        assert_eq!(default_port_file(5), PathBuf::from("node-data/node5/port"));
    }
}
