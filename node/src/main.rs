use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use eyre::Result;

use yggdrasil_node::config::{NodeConfigFile, default_config};
use yggdrasil_node::{
    NodeConfig, SyncServiceConfig, SyncServiceOutcome, VerificationConfig, bootstrap,
    run_sync_service,
};
use yggdrasil_ledger::Point;
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
                None => default_config(),
            };

            let peer_addr = peer.unwrap_or(file_cfg.peer_addr);
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

            let sync_config = SyncServiceConfig {
                batch_size,
                keepalive_interval: file_cfg
                    .keepalive_interval_secs
                    .map(Duration::from_secs),
            };

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_node(node_config, sync_config, verification))
        }
    }
}

async fn run_node(
    node_config: NodeConfig,
    sync_config: SyncServiceConfig,
    verification: Option<VerificationConfig>,
) -> Result<()> {
    eprintln!(
        "Yggdrasil connecting to {} (magic {})",
        node_config.peer_addr, node_config.network_magic
    );

    let mut session = bootstrap(&node_config).await?;
    eprintln!(
        "Handshake complete: version {}",
        session.version.0
    );

    let mut store = InMemoryVolatile::default();
    let from_point = Point::Origin;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn signal handler for graceful shutdown.
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\nShutdown signal received");
        let _ = shutdown_tx.send(());
    });

    let _verification = verification;
    let outcome: SyncServiceOutcome = run_sync_service(
        &mut session.chain_sync,
        &mut session.block_fetch,
        &mut store,
        from_point,
        &sync_config,
        async { let _ = shutdown_rx.await; },
    )
    .await?;

    eprintln!(
        "Sync complete: {} blocks, {} rollbacks, {} batches, tip {:?}",
        outcome.total_blocks,
        outcome.total_rollbacks,
        outcome.batches_completed,
        outcome.final_point,
    );

    session.mux.abort();
    Ok(())
}
