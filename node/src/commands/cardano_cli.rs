//! Path-resolution helpers for the `cardano-cli` subcommand surface.
//!
//! Mirrors the upstream-config + network-magic discovery upstream
//! `cardano-cli` performs implicitly via `Cardano.Api.Environment`
//! and `Cardano.CLI.Environment`. Yggdrasil's `cardano-cli` subcommand
//! is a deliberately small Rust subset; these helpers locate the
//! reference Haskell-share `config.json` / `topology.json` and read
//! the network magic from either the config envelope or the Shelley
//! genesis file it points at.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Environment.hs>

use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use serde_json::json;

use yggdrasil_node::config::NetworkPreset;

use crate::cli::CardanoCliCommand;

/// Resolve the on-disk paths for the upstream Haskell-share `config.json`
/// and `topology.json` that the pure-Rust `cardano-cli` subset reads when
/// it needs to discover protocol/network parameters.
///
/// `upstream_config_root` is the operator-supplied override (typically
/// `--upstream-config-root /tmp/cardano-tooling/share`); when absent we
/// fall back to the vendored `node/configuration/<network>` directory so
/// the subcommand still works without an upstream install.
pub fn resolve_upstream_reference_paths(
    network: NetworkPreset,
    upstream_config_root: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)> {
    let network_dir = match network {
        NetworkPreset::Mainnet => "mainnet",
        NetworkPreset::Preprod => "preprod",
        NetworkPreset::Preview => "preview",
    };

    let mut root =
        upstream_config_root.unwrap_or_else(|| PathBuf::from("/tmp/cardano-tooling/share"));
    if !root.join(network_dir).is_dir() {
        root = PathBuf::from("node/configuration");
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

/// Read the network magic from the upstream Haskell-share `config.json`
/// at `config_path`, falling back through the upstream key precedence:
/// `TestnetMagic` → `NetworkMagic` → the `networkMagic` field of the
/// `ShelleyGenesisFile` it references → the network preset default.
pub fn extract_reference_network_magic(config_path: &Path, network: NetworkPreset) -> u32 {
    // Use the cheap accessor — `to_config()` would re-load topology +
    // peer-snapshot files just to read a single u32.
    let fallback_magic = network.network_magic();

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

/// Run selected cardano-cli operations from the pure Rust CLI implementation.
pub(crate) fn run_cardano_cli_command(
    network: NetworkPreset,
    upstream_config_root: Option<PathBuf>,
    action: CardanoCliCommand,
) -> Result<()> {
    let (config_path, topology_path) =
        resolve_upstream_reference_paths(network, upstream_config_root)?;
    let reference_network_magic = extract_reference_network_magic(&config_path, network);

    match action {
        CardanoCliCommand::Version => {
            // R296: Version output now sources its banner from
            // `yggdrasil_cardano_cli::helper::version_info()` so the
            // pure-Rust subset and any future Phase-F-implemented
            // commands print a consistent version string.
            println!("{}", yggdrasil_cardano_cli::helper::version_info());
            println!("network preset default: {}", network);
            Ok(())
        }
        CardanoCliCommand::ShowUpstreamConfig => {
            let out = json!({
                "network": network.to_string(),
                "config": config_path,
                "topology": topology_path,
                "network_magic": reference_network_magic,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        CardanoCliCommand::QueryTip {
            socket_path: _socket_path,
            network_magic,
        } => {
            let _magic = network_magic.unwrap_or(reference_network_magic);
            #[cfg(unix)]
            {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::commands::query::run_query(
                    _socket_path,
                    _magic,
                    crate::commands::query::QueryCommand::Tip,
                ))
            }
            #[cfg(not(unix))]
            {
                eyre::bail!(
                    "query-tip requires a Unix domain socket; not supported on this platform"
                )
            }
        }
    }
}
