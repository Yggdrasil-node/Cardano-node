use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::Result;

use yggdrasil_node::config::{NetworkPreset, NodeConfigFile, default_config};
use yggdrasil_node::{
    NodeConfig, VerificationConfig, VerifiedSyncServiceConfig, VerifiedSyncServiceOutcome,
    bootstrap_with_fallbacks, run_verified_sync_service,
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
                file_cfg.bootstrap_peers.clone()
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
            rt.block_on(run_node(node_config, bootstrap_peers, sync_config))
        }
    }
}

async fn run_node(
    node_config: NodeConfig,
    bootstrap_peers: Vec<SocketAddr>,
    sync_config: VerifiedSyncServiceConfig,
) -> Result<()> {
    eprintln!(
        "Yggdrasil connecting to {} bootstrap peer(s) (primary {}, magic {})",
        1 + bootstrap_peers.len(),
        node_config.peer_addr,
        node_config.network_magic
    );

    let mut session = bootstrap_with_fallbacks(&node_config, &bootstrap_peers).await?;
    eprintln!(
        "Handshake complete with {}: version {}",
        session.connected_peer_addr,
        session.version.0
    );

    let mut store = InMemoryVolatile::default();
    let from_point = Point::Origin;

    let nonce_state = sync_config
        .nonce_config
        .as_ref()
        .map(|_| NonceEvolutionState::new(Nonce::Neutral));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn signal handler for graceful shutdown.
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\nShutdown signal received");
        let _ = shutdown_tx.send(());
    });

    let outcome: VerifiedSyncServiceOutcome = run_verified_sync_service(
        &mut session.chain_sync,
        &mut session.block_fetch,
        &mut store,
        from_point,
        &sync_config,
        nonce_state,
        async { let _ = shutdown_rx.await; },
    )
    .await?;

    eprintln!(
        "Sync complete: {} blocks, {} rollbacks, {} batches, {} stable, tip {:?}",
        outcome.total_blocks,
        outcome.total_rollbacks,
        outcome.batches_completed,
        outcome.stable_block_count,
        outcome.final_point,
    );

    if let Some(ref nonce) = outcome.nonce_state {
        eprintln!(
            "Epoch nonce: {:?} (epoch {})",
            nonce.epoch_nonce,
            nonce.current_epoch.0,
        );
    }

    if let Some(ref cs) = outcome.chain_state {
        eprintln!(
            "Chain state: {} volatile entries, tip {:?}",
            cs.volatile_len(),
            cs.tip(),
        );
    }

    session.mux.abort();
    Ok(())
}
