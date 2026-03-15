use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::Result;
use serde_json::json;

use yggdrasil_node::config::{NetworkPreset, NodeConfigFile, TraceNamespaceConfig, default_config};
use yggdrasil_node::tracer::{NodeTracer, trace_fields};
use yggdrasil_node::{
    LedgerCheckpointPolicy, NodeConfig, ResumedSyncServiceOutcome, VerificationConfig,
    ResumeReconnectingVerifiedSyncRequest, VerifiedSyncServiceConfig,
    resume_reconnecting_verified_sync_service_chaindb,
};
use yggdrasil_consensus::{EpochSize, NonceEvolutionConfig, NonceEvolutionState, SecurityParam};
use yggdrasil_ledger::{Era, LedgerState, Nonce};
use yggdrasil_network::HandshakeVersion;
use yggdrasil_storage::{ChainDb, FileImmutable, FileLedgerStore, FileVolatile};

const CHECKPOINT_TRACE_NAMESPACE: &str = "Node.Recovery.Checkpoint";

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
        /// Minimum slot delta between persisted ledger checkpoints.
        #[arg(long)]
        checkpoint_interval_slots: Option<u64>,
        /// Maximum number of persisted ledger checkpoints to retain.
        #[arg(long)]
        max_ledger_snapshots: Option<usize>,
        /// Maximum checkpoint trace events emitted per second. Use `0` to disable rate limiting.
        #[arg(long)]
        checkpoint_trace_max_frequency: Option<f64>,
        /// Severity override for checkpoint trace events, for example `Info` or `Silence`.
        #[arg(long)]
        checkpoint_trace_severity: Option<String>,
        /// Backend override for checkpoint trace events. Repeat the flag to route to multiple backends.
        #[arg(long, action = clap::ArgAction::Append)]
        checkpoint_trace_backend: Vec<String>,
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
            checkpoint_interval_slots,
            max_ledger_snapshots,
            checkpoint_trace_max_frequency,
            checkpoint_trace_severity,
            checkpoint_trace_backend,
        } => {
            let (mut file_cfg, config_base_dir) = match config {
                Some(path) => {
                    let contents = std::fs::read_to_string(&path)?;
                    let parsed: NodeConfigFile = serde_json::from_str(&contents)?;
                    (parsed, path.parent().map(PathBuf::from))
                }
                None => match network {
                    Some(preset) => (preset.to_config(), None),
                    None => (default_config(), None),
                },
            };

            if let Some(max_frequency) = checkpoint_trace_max_frequency {
                checkpoint_trace_config_mut(&mut file_cfg).max_frequency = if max_frequency > 0.0 {
                    Some(max_frequency)
                } else {
                    None
                };
            }

            if let Some(severity) = checkpoint_trace_severity {
                checkpoint_trace_config_mut(&mut file_cfg).severity = Some(severity);
            }

            if !checkpoint_trace_backend.is_empty() {
                checkpoint_trace_config_mut(&mut file_cfg).backends = checkpoint_trace_backend;
            }

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
            let checkpoint_interval_slots = checkpoint_interval_slots
                .unwrap_or(file_cfg.checkpoint_interval_slots);
            let max_ledger_snapshots = max_ledger_snapshots
                .unwrap_or(file_cfg.max_ledger_snapshots);

            let sync_config = if let Some(verification) = verification {
                VerifiedSyncServiceConfig {
                    batch_size,
                    verification,
                    nonce_config: Some(nonce_config),
                    security_param: Some(security_param),
                    checkpoint_policy: LedgerCheckpointPolicy {
                        min_slot_delta: checkpoint_interval_slots,
                        max_snapshots: max_ledger_snapshots,
                    },
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
                    checkpoint_policy: LedgerCheckpointPolicy {
                        min_slot_delta: checkpoint_interval_slots,
                        max_snapshots: max_ledger_snapshots,
                    },
                }
            };

            let rt = tokio::runtime::Runtime::new()?;
            let tracer = NodeTracer::from_config(&file_cfg);
            let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir.as_deref());
            rt.block_on(run_node(node_config, bootstrap_peers, sync_config, tracer, storage_dir))
        }
    }
}

fn resolve_storage_dir(storage_dir: &std::path::Path, config_base_dir: Option<&std::path::Path>) -> PathBuf {
    if storage_dir.is_absolute() {
        storage_dir.to_path_buf()
    } else if let Some(base_dir) = config_base_dir {
        base_dir.join(storage_dir)
    } else {
        storage_dir.to_path_buf()
    }
}

fn checkpoint_trace_config_mut(file_cfg: &mut NodeConfigFile) -> &mut TraceNamespaceConfig {
    file_cfg
        .trace_options
        .entry(CHECKPOINT_TRACE_NAMESPACE.to_owned())
    .or_default()
}

async fn run_node(
    node_config: NodeConfig,
    bootstrap_peers: Vec<SocketAddr>,
    sync_config: VerifiedSyncServiceConfig,
    tracer: NodeTracer,
    storage_dir: PathBuf,
) -> Result<()> {
    tracer.trace_runtime(
        "Startup.DiffusionInit",
        "Notice",
        "starting node runtime",
        trace_fields([
            ("primaryPeer", json!(node_config.peer_addr.to_string())),
            ("bootstrapPeerCount", json!(1 + bootstrap_peers.len())),
            ("networkMagic", json!(node_config.network_magic)),
            ("storageDir", json!(storage_dir.display().to_string())),
            ("protocolVersions", json!(node_config.protocol_versions.iter().map(|v| v.0).collect::<Vec<_>>())),
        ]),
    );

    let mut chain_db = ChainDb::new(
        FileImmutable::open(storage_dir.join("immutable"))?,
        FileVolatile::open(storage_dir.join("volatile"))?,
        FileLedgerStore::open(storage_dir.join("ledger"))?,
    );

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

    let outcome: ResumedSyncServiceOutcome = match resume_reconnecting_verified_sync_service_chaindb(
        &mut chain_db,
        ResumeReconnectingVerifiedSyncRequest {
            node_config: &node_config,
            fallback_peer_addrs: &bootstrap_peers,
            base_ledger_state: LedgerState::new(Era::Byron),
            config: &sync_config,
            nonce_state,
        },
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
            ("checkpointSlot", json!(outcome.recovery.checkpoint_slot.map(|slot| slot.0))),
            ("replayedVolatileBlocks", json!(outcome.recovery.replayed_volatile_blocks)),
            ("recoveredPoint", json!(format!("{:?}", outcome.recovery.point))),
            ("totalBlocks", json!(outcome.sync.total_blocks)),
            ("totalRollbacks", json!(outcome.sync.total_rollbacks)),
            ("batchesCompleted", json!(outcome.sync.batches_completed)),
            ("stableBlockCount", json!(outcome.sync.stable_block_count)),
            ("reconnectCount", json!(outcome.sync.reconnect_count)),
            ("lastConnectedPeer", json!(outcome.sync.last_connected_peer_addr.map(|addr| addr.to_string()))),
            ("finalPoint", json!(format!("{:?}", outcome.sync.final_point))),
        ]),
    );

    if let Some(ref nonce) = outcome.sync.nonce_state {
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

    if let Some(ref cs) = outcome.sync.chain_state {
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

#[cfg(test)]
mod tests {
    use super::{
        CHECKPOINT_TRACE_NAMESPACE, checkpoint_trace_config_mut,
    };
    use yggdrasil_node::config::default_config;

    #[test]
    fn checkpoint_trace_override_creates_namespace_when_missing() {
        let mut cfg = default_config();
        cfg.trace_options.remove(CHECKPOINT_TRACE_NAMESPACE);

        checkpoint_trace_config_mut(&mut cfg).severity = Some("Info".to_owned());

        assert_eq!(
            cfg.trace_options
                .get(CHECKPOINT_TRACE_NAMESPACE)
                .expect("checkpoint namespace")
                .severity
                .as_deref(),
            Some("Info")
        );
    }

    #[test]
    fn checkpoint_trace_override_can_disable_rate_limit() {
        let mut cfg = default_config();

        checkpoint_trace_config_mut(&mut cfg).max_frequency = None;

        assert_eq!(
            cfg.trace_options
                .get(CHECKPOINT_TRACE_NAMESPACE)
                .expect("checkpoint namespace")
                .max_frequency,
            None
        );
    }

    #[test]
    fn checkpoint_trace_override_updates_severity_and_backends() {
        let mut cfg = default_config();
        let override_cfg = checkpoint_trace_config_mut(&mut cfg);
        override_cfg.severity = Some("Silence".to_owned());
        override_cfg.backends = vec![
            "Stdout MachineFormat".to_owned(),
            "Forwarder".to_owned(),
        ];

        let checkpoint_cfg = cfg
            .trace_options
            .get(CHECKPOINT_TRACE_NAMESPACE)
            .expect("checkpoint namespace");
        assert_eq!(checkpoint_cfg.severity.as_deref(), Some("Silence"));
        assert_eq!(
            checkpoint_cfg.backends,
            vec![
                "Stdout MachineFormat".to_owned(),
                "Forwarder".to_owned(),
            ]
        );
    }
}
