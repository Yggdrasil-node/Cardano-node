//! Node-binary entry point for the `cardano-cli` subcommand surface.
//!
//! Thin dispatcher that routes the parsed `CardanoCliCommand` to the
//! `yggdrasil-cardano-cli` crate's runners. Network-preset resolution
//! (`NetworkPreset` enum -> network_dir string + fallback magic) lives
//! here so the new crate stays independent of the node binary's
//! config types.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-cli/blob/master/cardano-cli/src/Cardano/CLI/Environment.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side dispatcher for the `yggdrasil-node cardano-cli <subcommand>` integration. Wraps `yggdrasil_cardano_cli::*` library calls; the actual subcommand runtime logic lives in `crates/tools/cardano-cli/` (R447 relocated). No upstream parallel — upstream `cardano-cli` is a separate binary, not a node-binary subcommand.

use std::path::PathBuf;

use eyre::Result;

use yggdrasil_cardano_cli::environment;
use yggdrasil_node_config::NetworkPreset;

use crate::cli::CardanoCliCommand;

/// Map a `NetworkPreset` enum to its on-disk sub-directory name. The
/// `yggdrasil-cardano-cli` crate accepts the directory name as a `&str`
/// to avoid importing `yggdrasil_node_config::NetworkPreset` (which
/// would invert the dependency direction).
fn network_dir(network: NetworkPreset) -> &'static str {
    match network {
        NetworkPreset::Mainnet => "mainnet",
        NetworkPreset::Preprod => "preprod",
        NetworkPreset::Preview => "preview",
    }
}

/// Run selected cardano-cli operations from the pure Rust CLI implementation.
pub(crate) fn run_cardano_cli_command(
    network: NetworkPreset,
    upstream_config_root: Option<PathBuf>,
    action: CardanoCliCommand,
) -> Result<()> {
    let dir = network_dir(network);
    let (config_path, topology_path) =
        environment::resolve_upstream_reference_paths(dir, upstream_config_root)?;
    let reference_network_magic =
        environment::extract_reference_network_magic(&config_path, network.network_magic());

    match action {
        CardanoCliCommand::Version => {
            // R296: Version output sources its banner from
            // `yggdrasil_cardano_cli::helper::version_info()` so the
            // pure-Rust subset and any future Phase-F-implemented
            // commands print a consistent version string.
            println!("{}", yggdrasil_cardano_cli::helper::version_info());
            println!("network preset default: {}", network);
            Ok(())
        }
        CardanoCliCommand::ShowUpstreamConfig => {
            // R297: ShowUpstreamConfig migrated into
            // yggdrasil-cardano-cli::environment::run_show_upstream_config.
            environment::run_show_upstream_config(
                &network.to_string(),
                &config_path,
                &topology_path,
                reference_network_magic,
            )
        }
        CardanoCliCommand::QueryTip {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::Tip,
        ),
        CardanoCliCommand::QueryUtxo {
            socket_path: _socket_path,
            network_magic,
            address,
            tx_in,
        } => {
            let _magic = network_magic.unwrap_or(reference_network_magic);
            let query = match (address, tx_in) {
                (Some(addr), None) => {
                    crate::commands::query::QueryCommand::UtxoByAddress { address: addr }
                }
                (None, Some(tx)) => {
                    // Upstream `cardano-cli` accepts `--tx-in TX#INDEX`
                    // as a single token. Split here so the downstream
                    // `UtxoByTxIn` query gets the structured pair.
                    let (tx_id, index_str) = tx.split_once('#').ok_or_else(|| {
                        eyre::eyre!(
                            "--tx-in expects TX_HASH#INDEX (e.g. 0123ab…#0); got {tx:?}"
                        )
                    })?;
                    let index: u16 = index_str.parse().map_err(|e| {
                        eyre::eyre!(
                            "--tx-in index {index_str:?} is not a valid u16: {e}"
                        )
                    })?;
                    crate::commands::query::QueryCommand::UtxoByTxIn {
                        tx_id: tx_id.to_string(),
                        index,
                    }
                }
                (None, None) => eyre::bail!(
                    "query-utxo requires either --address or --tx-in; pass one of them"
                ),
                (Some(_), Some(_)) => unreachable!(
                    "clap's conflicts_with = ... pair prevents both flags being set"
                ),
            };
            run_query_via_binary_runtime(_socket_path, _magic, query)
        }
        CardanoCliCommand::QueryProtocolParameters {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::ProtocolParams,
        ),
        CardanoCliCommand::QueryStakePools {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::StakePools,
        ),
        CardanoCliCommand::QueryStakeDistribution {
            socket_path: _socket_path,
            network_magic,
        } => run_query_via_binary_runtime(
            _socket_path,
            network_magic.unwrap_or(reference_network_magic),
            crate::commands::query::QueryCommand::StakeDistribution,
        ),
    }
}

/// Shared dispatch helper: build the binary's `tokio::runtime::Runtime`
/// and drive `crate::commands::query::run_query` to completion.
///
/// Used by every `cardano-cli query-*` variant. Centralised here so
/// the Unix-only `cfg` gate + runtime construction logic lives in one
/// place instead of being duplicated across every match arm. The
/// `_socket_path` underscored prefix is preserved from the upstream
/// expansion of the QueryTip arm to keep the non-Unix `cfg` branch
/// non-warning.
fn run_query_via_binary_runtime(
    socket_path: PathBuf,
    network_magic: u32,
    query: crate::commands::query::QueryCommand,
) -> Result<()> {
    let _socket_path = socket_path;
    let _magic = network_magic;
    let _query = query;
    #[cfg(unix)]
    {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(crate::commands::query::run_query(
            _socket_path,
            _magic,
            _query,
        ))
    }
    #[cfg(not(unix))]
    {
        eyre::bail!(
            "cardano-cli query subcommands require a Unix domain socket; \
             not supported on this platform"
        )
    }
}
