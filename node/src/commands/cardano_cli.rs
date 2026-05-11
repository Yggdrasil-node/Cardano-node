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
use yggdrasil_node::config::NetworkPreset;

use crate::cli::CardanoCliCommand;

/// Map a `NetworkPreset` enum to its on-disk sub-directory name. The
/// `yggdrasil-cardano-cli` crate accepts the directory name as a `&str`
/// to avoid importing `yggdrasil_node::config::NetworkPreset` (which
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
        } => {
            // QueryTip remains in the node binary because it depends on
            // the binary's tokio runtime + `commands::query::run_query`
            // helper. R298+ will migrate it once the trait-based
            // abstraction for the LSQ socket client lands.
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
