use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::Result;
use serde_json::json;

use yggdrasil_node::config::{NetworkPreset, NodeConfigFile, default_config};
use yggdrasil_node::tracer::{NodeTracer, trace_fields};
use yggdrasil_node::{
    NodeConfig, ReconnectingSyncServiceOutcome, VerificationConfig, VerifiedSyncServiceConfig,
    run_reconnecting_verified_sync_service_with_tracer,
};
use yggdrasil_consensus::{EpochSize, NonceEvolutionConfig, NonceEvolutionState, SecurityParam};
use yggdrasil_ledger::{Nonce, Point};
use yggdrasil_network::HandshakeVersion;
use yggdrasil_storage::InMemoryVolatile;

/// Yggdrasil — a pure Rust Cardano node.
#[derive(Parser)]
#[command(name = "yggdrasil", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Connect to a peer and sync the chain.
    Run {
        /// Path to a JSON configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
        /// Peer address (host:port). Overrides config file.
        #[arg(long)]
        peer: Option<SocketAddr>,
        /// Network magic. Overrides config file.
        #[arg(long)]
        network_magic: Option<u32>,
        /// Disable header verification.
        #[arg(long)]
        no_verify: bool,
        /// Batch size for sync iterations.
        #[arg(long, default_value = "10")]
        batch_size: usize,
    },
    /// Print the default configuration as JSON.
    DefaultConfig,
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
        Command::Run {
            config,
            network,
            peer,
            network_magic,
            no_verify,
            batch_size,
        } => {
            let file_cfg = match config {
                Some(path) => {
                    let contents = std::fs::read_to_string(&path)?;
                    let parsed: NodeConfigFile = serde_json::from_str(&contents)?;
                    parsed
                }
                None => match network {
                    Some(preset) => preset.to_config(),
                    None => default_config(),
                },
            };

            let peer_addr = peer.unwrap_or(file_cfg.peer_addr);
            let bootstrap_peers = if peer.is_some() {
                Vec::new()
            } else {
                file_cfg.ordered_fallback_peers()
            };
            let magic = network_magic.unwrap_or(file_cfg.network_magic);
            let protocol_versions: Vec<HandshakeVersion> = file_cfg
                .protocol_versions
                .iter()
                .map(|v| HandshakeVersion(*v as u16))
                .collect();

            let node_config = NodeConfig {
                peer_addr,
                network_magic: magic,
                protocol_versions,
            };

            let verification = if no_verify {
                None
            } else {
                Some(VerificationConfig {
                    slots_per_kes_period: file_cfg.slots_per_kes_period,
                    max_kes_evolutions: file_cfg.max_kes_evolutions,
                    verify_body_hash: true,
                })
            };

            let nonce_config = NonceEvolutionConfig {
                epoch_size: EpochSize(file_cfg.epoch_length),
                // stability_window = 3k/f
                stability_window: (3.0 * file_cfg.security_param_k as f64
                    / file_cfg.active_slot_coeff) as u64,
                extra_entropy: Nonce::Neutral,
            };

            let security_param = SecurityParam(file_cfg.security_param_k);

            let sync_config = if let Some(verification) = verification {
                VerifiedSyncServiceConfig {
                    batch_size,
                    verification,
                    nonce_config: Some(nonce_config),
                    security_param: Some(security_param),
                }
            } else {
                VerifiedSyncServiceConfig {
                    batch_size,
                    verification: VerificationConfig {
                        slots_per_kes_period: file_cfg.slots_per_kes_period,
                        max_kes_evolutions: file_cfg.max_kes_evolutions,
                        verify_body_hash: false,
                    },
                    nonce_config: Some(nonce_config),
                    security_param: Some(security_param),
                }
            };

            let rt = tokio::runtime::Runtime::new()?;
            let tracer = NodeTracer::from_config(&file_cfg);
            rt.block_on(run_node(node_config, bootstrap_peers, sync_config, tracer))
        }
    }
}

async fn run_node(
    node_config: NodeConfig,
    bootstrap_peers: Vec<SocketAddr>,
    sync_config: VerifiedSyncServiceConfig,
    tracer: NodeTracer,
) -> Result<()> {
    tracer.trace_runtime(
        "Startup.DiffusionInit",
        "Notice",
        "starting node runtime",
        trace_fields([
            ("primaryPeer", json!(node_config.peer_addr.to_string())),
            ("bootstrapPeerCount", json!(1 + bootstrap_peers.len())),
            ("networkMagic", json!(node_config.network_magic)),
            ("protocolVersions", json!(node_config.protocol_versions.iter().map(|v| v.0).collect::<Vec<_>>())),
        ]),
    );

    let mut store = InMemoryVolatile::default();
    let from_point = Point::Origin;

    let nonce_state = sync_config
        .nonce_config
        .as_ref()
        .map(|_| NonceEvolutionState::new(Nonce::Neutral));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn signal handler for graceful shutdown.
    let signal_tracer = tracer.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        signal_tracer.trace_runtime(
            "Node.Shutdown",
            "Notice",
            "shutdown signal received",
            std::collections::BTreeMap::new(),
        );
        let _ = shutdown_tx.send(());
    });

    let outcome: ReconnectingSyncServiceOutcome = match run_reconnecting_verified_sync_service_with_tracer(
        &node_config,
        &bootstrap_peers,
        &mut store,
        from_point,
        &sync_config,
        nonce_state,
        &tracer,
        async { let _ = shutdown_rx.await; },
    )
    .await {
        Ok(outcome) => outcome,
        Err(err) => {
            tracer.trace_runtime(
                "Node.Sync",
                "Error",
                "node run failed",
                trace_fields([
                    ("error", json!(err.to_string())),
                    ("primaryPeer", json!(node_config.peer_addr.to_string())),
                ]),
            );
            return Err(err.into());
        }
    };

    tracer.trace_runtime(
        "Node.Sync",
        "Notice",
        "sync complete",
        trace_fields([
            ("totalBlocks", json!(outcome.total_blocks)),
            ("totalRollbacks", json!(outcome.total_rollbacks)),
            ("batchesCompleted", json!(outcome.batches_completed)),
            ("stableBlockCount", json!(outcome.stable_block_count)),
            ("reconnectCount", json!(outcome.reconnect_count)),
            ("lastConnectedPeer", json!(outcome.last_connected_peer_addr.map(|addr| addr.to_string()))),
            ("finalPoint", json!(format!("{:?}", outcome.final_point))),
        ]),
    );

    if let Some(ref nonce) = outcome.nonce_state {
        tracer.trace_runtime(
            "Node.Sync",
            "Info",
            "epoch nonce state updated",
            trace_fields([
                ("epoch", json!(nonce.current_epoch.0)),
                ("epochNonce", json!(format!("{:?}", nonce.epoch_nonce))),
            ]),
        );
    }

    if let Some(ref cs) = outcome.chain_state {
        tracer.trace_runtime(
            "Node.Sync",
            "Info",
            "chain state tracked",
            trace_fields([
                ("volatileEntries", json!(cs.volatile_len())),
                ("tip", json!(format!("{:?}", cs.tip()))),
            ]),
        );
    }

    Ok(())
}
