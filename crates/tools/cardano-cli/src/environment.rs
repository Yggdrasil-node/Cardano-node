//! Environment / config-path helpers.
//!
//! Mirrors upstream `Cardano.CLI.Environment` — discovery of
//! `CARDANO_NODE_SOCKET_PATH`, `CARDANO_NODE_NETWORK_ID`, and the
//! upstream-config root directory the cardano-cli reads when invoked
//! without explicit flags.
//!
//! R297 migrates `resolve_upstream_reference_paths` and
//! `extract_reference_network_magic` from `node/src/commands/cardano_cli.rs`
//! into this module. Both helpers were authored in the node binary
//! before the new crate landed; the migration keeps cross-crate
//! decoupling clean by taking `network_dir: &str` and `fallback_magic:
//! u32` as parameters instead of importing `yggdrasil_node_config::
//! NetworkPreset` (which would invert the dependency direction).
//!
//! ## Naming parity
//!
//! **Strict mirror:** `cardano-cli/cardano-cli/src/Cardano/CLI/Environment.hs`.

use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use serde_json::json;

/// Resolve the upstream Haskell-share root directory used by the
/// cardano-cli binary. The fallback chain (R289 stub):
/// 1. Operator-supplied `--upstream-config-root` flag.
/// 2. `CARDANO_NODE_UPSTREAM_CONFIG_ROOT` env var.
/// 3. `/tmp/cardano-tooling/share` default (operator convention).
/// 4. Vendored `node/configuration/<network>/` as last-resort.
///
/// Mirrors upstream `getEnvCli*` resolution from
/// `Cardano.CLI.Environment`.
pub fn resolve_upstream_config_root(override_path: Option<PathBuf>) -> PathBuf {
    if let Some(path) = override_path {
        return path;
    }
    if let Ok(env_path) = std::env::var("CARDANO_NODE_UPSTREAM_CONFIG_ROOT") {
        if !env_path.is_empty() {
            return PathBuf::from(env_path);
        }
    }
    PathBuf::from("/tmp/cardano-tooling/share")
}

/// Resolve the node socket path from the operator flag or
/// `CARDANO_NODE_SOCKET_PATH` env var.
///
/// Mirrors upstream `getEnvSocketPath` from `Cardano.CLI.Environment`.
pub fn resolve_socket_path(override_path: Option<PathBuf>) -> Option<PathBuf> {
    if override_path.is_some() {
        return override_path;
    }
    std::env::var("CARDANO_NODE_SOCKET_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Resolve the on-disk paths for the upstream Haskell-share `config.json`
/// and `topology.json`.
///
/// `network_dir` is the per-network sub-directory name (`"mainnet"`,
/// `"preprod"`, `"preview"`); `upstream_config_root` is the operator-
/// supplied override (typically `--upstream-config-root /tmp/cardano-tooling/share`).
/// When the override is absent we fall back to `/tmp/cardano-tooling/share`,
/// then to the vendored `crates/node/yggdrasil-node/configuration/<network_dir>` directory so
/// the subcommand still works without an upstream install.
///
/// Migrated from `node/src/commands/cardano_cli.rs` in R297. Takes
/// `network_dir: &str` rather than `yggdrasil_node_config::NetworkPreset`
/// to keep the new crate independent of the node binary's config types.
pub fn resolve_upstream_reference_paths(
    network_dir: &str,
    upstream_config_root: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)> {
    let mut root =
        upstream_config_root.unwrap_or_else(|| PathBuf::from("/tmp/cardano-tooling/share"));
    if !root.join(network_dir).is_dir() {
        root = PathBuf::from("crates/node/yggdrasil-node/configuration");
    }

    let config_path = root.join(network_dir).join("config.json");
    let topology_path = root.join(network_dir).join("topology.json");

    if !config_path.is_file() {
        bail!(
            "upstream reference config not found: {}",
            config_path.display()
        );
    }

    Ok((config_path, topology_path))
}

/// Read the network magic from the upstream Haskell-share `config.json`,
/// falling back through the upstream key precedence:
/// `TestnetMagic` -> `NetworkMagic` -> the `networkMagic` field of the
/// `ShelleyGenesisFile` it references -> `fallback_magic`.
///
/// Migrated from `node/src/commands/cardano_cli.rs` in R297. Takes
/// `fallback_magic: u32` rather than deriving it from a NetworkPreset.
pub fn extract_reference_network_magic(config_path: &Path, fallback_magic: u32) -> u32 {
    let config_json = std::fs::read(config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());

    if let Some(magic) = config_json
        .as_ref()
        .and_then(|v| v.get("TestnetMagic"))
        .and_then(|v| v.as_u64())
    {
        return magic as u32;
    }

    if let Some(magic) = config_json
        .as_ref()
        .and_then(|v| v.get("NetworkMagic"))
        .and_then(|v| v.as_u64())
    {
        return magic as u32;
    }

    let genesis_path = config_json
        .as_ref()
        .and_then(|v| v.get("ShelleyGenesisFile"))
        .and_then(|v| v.as_str())
        .map(|name| {
            config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(name)
        });

    if let Some(path) = genesis_path {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(magic) = v.get("networkMagic").and_then(|n| n.as_u64()) {
                    return magic as u32;
                }
            }
        }
    }

    fallback_magic
}

/// Print the resolved upstream-config snapshot as pretty JSON to
/// stdout (the body of the `cardano-cli show-upstream-config`
/// subcommand).
///
/// Migrated from `node/src/commands/cardano_cli.rs` in R297. The node
/// binary's existing handler now calls into this function.
pub fn run_show_upstream_config(
    network_name: &str,
    config_path: &Path,
    topology_path: &Path,
    network_magic: u32,
) -> Result<()> {
    let out = json!({
        "network": network_name,
        "config": config_path,
        "topology": topology_path,
        "network_magic": network_magic,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
