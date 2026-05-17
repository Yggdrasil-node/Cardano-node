#![cfg_attr(test, allow(clippy::unwrap_used))]

use clap::Parser;
use eyre::{Result, WrapErr, bail};

use yggdrasil_node_config::default_config;

// `clap` subcommand definitions for the binary surface. Mirrors
// upstream `Cardano.Node.Parsers`.
mod cli;
use cli::Command;

/// Yggdrasil — a pure Rust Cardano node.
#[derive(Parser)]
#[command(name = "yggdrasil", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::DefaultConfig => {
            let cfg = default_config();
            let json = serde_json::to_string_pretty(&cfg)?;
            println!("{json}");
            Ok(())
        }
        Command::CardanoCli {
            network,
            upstream_config_root,
            action,
        } => commands::cardano_cli::run_cardano_cli_command(network, upstream_config_root, action),
        Command::ValidateConfig {
            config,
            network,
            topology,
            database_path,
            port,
            host_addr,
            non_producing_node,
            shelley_kes_key,
            shelley_vrf_key,
            shelley_operational_certificate,
            shelley_operational_certificate_issuer_vkey,
        } => commands::validate_config::run_validate_config_subcommand(
            config,
            network,
            topology,
            database_path,
            port,
            host_addr,
            non_producing_node,
            shelley_kes_key,
            shelley_vrf_key,
            shelley_operational_certificate,
            shelley_operational_certificate_issuer_vkey,
        ),
        Command::Status {
            config,
            network,
            topology,
            database_path,
        } => commands::status::run_status_subcommand(config, network, topology, database_path),
        Command::Run {
            config,
            network,
            topology,
            peer,
            network_magic,
            database_path,
            port,
            host_addr,
            no_verify,
            batch_size,
            checkpoint_interval_slots,
            max_ledger_snapshots,
            checkpoint_trace_max_frequency,
            checkpoint_trace_severity,
            checkpoint_trace_backend,
            metrics_port,
            non_producing_node,
            max_concurrent_block_fetch_peers,
            socket_path,
            shelley_kes_key,
            shelley_vrf_key,
            shelley_operational_certificate,
            shelley_operational_certificate_issuer_vkey,
        } => commands::run::run_subcommand(commands::run::RunCmdArgs {
            config,
            network,
            topology,
            peer,
            network_magic,
            database_path,
            port,
            host_addr,
            no_verify,
            batch_size,
            checkpoint_interval_slots,
            max_ledger_snapshots,
            checkpoint_trace_max_frequency,
            checkpoint_trace_severity,
            checkpoint_trace_backend,
            metrics_port,
            non_producing_node,
            max_concurrent_block_fetch_peers,
            socket_path,
            shelley_kes_key,
            shelley_vrf_key,
            shelley_operational_certificate,
            shelley_operational_certificate_issuer_vkey,
        }),
        #[cfg(unix)]
        Command::Query {
            socket_path,
            network_magic,
            query,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_query(socket_path, network_magic, query))
        }
        #[cfg(unix)]
        Command::TxMempool {
            socket_path,
            network_magic,
            action,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_tx_mempool(socket_path, network_magic, action))
        }
        #[cfg(unix)]
        Command::SubmitTx {
            socket_path,
            network_magic,
            tx_file,
            tx_hex,
        } => {
            let tx_bytes = match (tx_file, tx_hex) {
                (Some(path), _) => std::fs::read(&path)
                    .wrap_err_with(|| format!("failed to read tx file {}", path.display()))?,
                (_, Some(hex)) => decode_tx_hex_arg(&hex)?,
                (None, None) => bail!("one of --tx-file or --tx-hex is required"),
            };
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_submit_tx(socket_path, network_magic, tx_bytes))
        }
    }
}

// Shutdown signal handler moved to `crates/node/yggdrasil-node/src/handlers/shutdown.rs` in R257
// (Phase D-runtime first slice) to mirror upstream
// `Cardano.Node.Handlers.Shutdown`.
mod handlers;
use handlers::shutdown::wait_for_shutdown_signal;

// Prometheus metrics HTTP server (loopback-only). Mirrors upstream
// `Cardano.Node.Tracing.Tracers.Startup` Prometheus endpoint.
// Wave 5 PR 7: metrics_server moved to the yggdrasil-node-tracer crate;
// reach it through the `tracer::metrics_server` re-export.
use metrics_server::serve_metrics;
use yggdrasil_node_tracer::metrics_server;

// Genesis-aware startup helpers. Mirrors upstream
// `Cardano.Node.Run`'s genesis-loading slice +
// `Ouroboros.Consensus.Node.Genesis`.
mod startup;
pub(crate) use startup::best_effort_base_ledger_state;
#[cfg(feature = "forge")]
use startup::forged_header_protocol_version;
use startup::{strict_base_ledger_state, trace_genesis_hashes_verified};

// Config-relative path resolution. Wave 5 PR 7+8 moved this module
// into the standalone `yggdrasil-node-config` crate. The binary
// imports the helpers directly from the new crate — the lib.rs
// `pub use yggdrasil_node_config::path_resolve;` re-export covers
// `yggdrasil_node_config::path_resolve::*` for downstream users, but the
// binary itself is a separate crate root and reaches the helpers
// through the public crate path.
pub(crate) use yggdrasil_node_config::path_resolve::{resolve_config_path, resolve_storage_dir};

// Ledger-derived fallback peer assembly. Mirrors upstream
// `Ouroboros.Network.PeerSelection.LedgerPeers`.
mod ledger_peers;
pub(crate) use ledger_peers::{
    configured_fallback_peers, ledger_peer_snapshot_from_ledger_state, point_slot,
};

// Node runtime entry point. Mirrors upstream `Cardano.Node.Run.run`.
mod run_node;

// `commands` collects the per-subcommand dispatch helpers,
// mirroring upstream `Cardano.CLI.*`.
mod commands;
#[cfg(unix)]
use commands::query::run_query;
#[cfg(unix)]
use commands::submit_tx::{decode_tx_hex_arg, run_submit_tx};
#[cfg(unix)]
use commands::tx_mempool::run_tx_mempool;

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
