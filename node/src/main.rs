use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use clap::{Parser, Subcommand};
use eyre::{Result, WrapErr, bail};
use serde::Serialize;
use serde_json::json;

use yggdrasil_node::config::{
    NetworkPreset, NodeConfigFile, TraceNamespaceConfig, default_config,
    load_peer_snapshot_file,
};
use yggdrasil_node::tracer::{NodeMetrics, NodeTracer, trace_fields};
use yggdrasil_node::{
    BlockProvider, ChainProvider,
    LedgerCheckpointPolicy, NodeConfig, ResumedSyncServiceOutcome,
    RuntimeGovernorConfig, VerificationConfig,
    ResumeReconnectingVerifiedSyncRequest, VerifiedSyncServiceConfig,
    SharedChainDb, SharedPeerSharingProvider, SharedTxSubmissionConsumer,
    recover_ledger_state_chaindb,
    run_governor_loop,
    resume_reconnecting_verified_sync_service_shared_chaindb,
    seed_peer_registry,
    run_inbound_accept_loop,
};
use yggdrasil_consensus::{EpochSize, NonceEvolutionConfig, NonceEvolutionState, SecurityParam};
use yggdrasil_ledger::{Era, GenesisDelegationState, LedgerState, Nonce, Point, PoolRelayAccessPoint, StakeCredential};
use yggdrasil_mempool::SharedMempool;
use yggdrasil_network::{
    GovernorState, GovernorTargets,
    HandshakeVersion, LedgerPeerSnapshot, LedgerStateJudgement, PeerAccessPoint,
    PeerListener, resolve_peer_access_points,
};
use yggdrasil_storage::{ChainDb, FileImmutable, FileLedgerStore, FileVolatile, ImmutableStore, LedgerStore, VolatileStore};

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
        /// Port for Prometheus metrics HTTP endpoint. Disabled when not set.
        #[arg(long)]
        metrics_port: Option<u16>,
    },
    /// Validate config, snapshot inputs, and any existing on-disk storage state.
    ValidateConfig {
        /// Path to a JSON configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
    },
    /// Inspect on-disk storage and report current sync status.
    Status {
        /// Path to a JSON configuration file.
        #[arg(long, short)]
        config: Option<PathBuf>,
        /// Network preset (mainnet, preprod, preview). Overridden by --config.
        #[arg(long, value_parser = clap::value_parser!(NetworkPreset))]
        network: Option<NetworkPreset>,
    },
    /// Print the default configuration as JSON.
    DefaultConfig,
}

#[derive(Serialize)]
struct ConfigValidationReport {
    primary_peer: String,
    network_magic: u32,
    protocol_versions: Vec<u32>,
    storage_dir: String,
    configured_fallback_peer_count: usize,
    resolved_startup_peer_count: usize,
    use_ledger_peers: String,
    checkpoint_interval_slots: u64,
    max_ledger_snapshots: usize,
    peer_snapshot: PeerSnapshotValidationReport,
    storage: StorageValidationReport,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct PeerSnapshotValidationReport {
    status: &'static str,
    path: Option<String>,
    slot: Option<u64>,
    ledger_peer_count: usize,
    big_ledger_peer_count: usize,
    error: Option<String>,
}

#[derive(Serialize)]
struct StorageValidationReport {
    status: &'static str,
    tip: String,
    recovered_point: Option<String>,
    checkpoint_slot: Option<u64>,
    replayed_volatile_blocks: Option<usize>,
    ledger_peer_count: usize,
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
        Command::ValidateConfig { config, network } => {
            let (file_cfg, config_base_dir) = load_effective_config(config, network)?;
            let report = validate_config_report(&file_cfg, config_base_dir.as_deref())?;
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
            Ok(())
        }
        Command::Status { config, network } => {
            let (file_cfg, config_base_dir) = load_effective_config(config, network)?;
            let report = status_report(&file_cfg, config_base_dir.as_deref())?;
            let json = serde_json::to_string_pretty(&report)?;
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
            metrics_port,
        } => {
            let (mut file_cfg, config_base_dir) = load_effective_config(config, network)?;

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

            let magic = network_magic.unwrap_or(file_cfg.network_magic);
            let protocol_versions: Vec<HandshakeVersion> = file_cfg
                .protocol_versions
                .iter()
                .map(|v| HandshakeVersion(*v as u16))
                .collect();
            let plutus_cost_model = file_cfg
                .load_plutus_cost_model(config_base_dir.as_deref())
                .wrap_err("failed to load genesis Plutus cost model")?;

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
                    plutus_cost_model: plutus_cost_model.clone(),
                    verify_vrf: false,
                    active_slot_coeff: None,
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
                    plutus_cost_model: plutus_cost_model.clone(),
                    verify_vrf: false,
                    active_slot_coeff: None,
                }
            };

            let tracer = NodeTracer::from_config(&file_cfg);
            let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir.as_deref());
            let base_ledger_state = strict_base_ledger_state(&file_cfg, config_base_dir.as_deref())?;
            let chain_db = ChainDb::new(
                FileImmutable::open(storage_dir.join("immutable"))?,
                FileVolatile::open(storage_dir.join("volatile"))?,
                FileLedgerStore::open(storage_dir.join("ledger"))?,
            );

            let peer_addr = peer.unwrap_or(file_cfg.peer_addr);
            let recovery = recover_ledger_state_chaindb(&chain_db, base_ledger_state.clone());
            let latest_slot = recovery
                .as_ref()
                .ok()
                .and_then(|recovery| point_slot(&recovery.point))
                .or_else(|| point_slot(&chain_db.recovery().tip));
            let ledger_state_judgement = if recovery.is_ok() {
                LedgerStateJudgement::YoungEnough
            } else {
                LedgerStateJudgement::Unavailable
            };
            let ledger_snapshot = recovery
                .as_ref()
                .map(|recovery| ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state))
                .unwrap_or_default();
            let peer_snapshot_path = file_cfg
                .peer_snapshot_file
                .as_deref()
                .map(|path| resolve_config_path(std::path::Path::new(path), config_base_dir.as_deref()));

            if let Err(err) = &recovery {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to recover ledger state for startup ledger peers",
                    trace_fields([
                        ("latestSlot", json!(latest_slot)),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }

            let bootstrap_peers = if peer.is_some() {
                Vec::new()
            } else {
                configured_fallback_peers(
                    &file_cfg,
                    config_base_dir.as_deref(),
                    &ledger_snapshot,
                    latest_slot,
                    ledger_state_judgement,
                    &tracer,
                )
            };

            let node_config = NodeConfig {
                peer_addr,
                network_magic: magic,
                protocol_versions,
            };

            let governor_config = RuntimeGovernorConfig::new(
                std::time::Duration::from_secs(file_cfg.governor_tick_interval_secs),
                file_cfg.keepalive_interval_secs.map(std::time::Duration::from_secs),
                GovernorTargets {
                    target_known: file_cfg.governor_target_known,
                    target_established: file_cfg.governor_target_established,
                    target_active: file_cfg.governor_target_active,
                },
            );

            let mut topology_config = file_cfg.topology_config();
            if let Some(peer_snapshot_path) = &peer_snapshot_path {
                topology_config.peer_snapshot_file = Some(peer_snapshot_path.display().to_string());
            }

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_node(RunNodeRequest {
                node_config,
                bootstrap_peers,
                sync_config,
                governor_config,
                topology_config,
                tracer,
                storage_dir,
                chain_db,
                inbound_listen_addr: file_cfg.inbound_listen_addr,
                use_ledger_peers: Some(file_cfg.use_ledger_peers_policy()),
                peer_snapshot_path,
                metrics_port,
                base_ledger_state,
            }))
        }
    }
}

fn strict_base_ledger_state(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<LedgerState> {
    let mut state = LedgerState::new(Era::Byron);
    state.set_expected_network_id(file_cfg.expected_network_id());
    if let Some(bootstrap) = file_cfg
        .load_shelley_genesis_bootstrap(config_base_dir)
        .wrap_err("failed to load Shelley genesis bootstrap")?
    {
        state.configure_pending_shelley_genesis_utxo(bootstrap.initial_funds);
        state.configure_pending_shelley_genesis_stake(
            bootstrap
                .staking
                .into_iter()
                .map(|(credential, pool)| (StakeCredential::AddrKeyHash(credential), pool))
                .collect(),
        );
        state.configure_pending_shelley_genesis_delegs(
            bootstrap
                .gen_delegs
                .into_iter()
                .map(|(genesis_hash, parsed)| {
                    (
                        genesis_hash,
                        GenesisDelegationState {
                            delegate: parsed.delegate,
                            vrf: parsed.vrf,
                        },
                    )
                })
                .collect(),
        );
    }
    if let Some(params) = file_cfg
        .load_genesis_protocol_params(config_base_dir)
        .wrap_err("failed to load genesis protocol parameters")?
    {
        state.set_protocol_params(params);
    }
    if let Some(enact) = file_cfg
        .load_genesis_enact_state(config_base_dir)
        .wrap_err("failed to load genesis enact state")?
    {
        *state.enact_state_mut() = enact;
    }
    Ok(state)
}

fn best_effort_base_ledger_state(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> LedgerState {
    strict_base_ledger_state(file_cfg, config_base_dir)
        .unwrap_or_else(|_| LedgerState::new(Era::Byron))
}

fn load_effective_config(
    config: Option<PathBuf>,
    network: Option<NetworkPreset>,
) -> Result<(NodeConfigFile, Option<PathBuf>)> {
    match config {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)
                .wrap_err_with(|| format!("failed to read config file {}", path.display()))?;
            let parsed: NodeConfigFile = serde_json::from_str(&contents)
                .wrap_err_with(|| format!("failed to parse config file {}", path.display()))?;
            Ok((parsed, path.parent().map(PathBuf::from)))
        }
        None => Ok(match network {
            Some(preset) => (preset.to_config(), Some(preset_config_base_dir(preset))),
            None => (default_config(), None),
        }),
    }
}

fn preset_config_base_dir(preset: NetworkPreset) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("configuration")
        .join(preset.to_string())
}

fn validate_config_report(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<ConfigValidationReport> {
    if file_cfg.protocol_versions.is_empty() {
        bail!("node config must include at least one protocol version");
    }

    if !(file_cfg.active_slot_coeff.is_finite()
        && file_cfg.active_slot_coeff > 0.0
        && file_cfg.active_slot_coeff <= 1.0)
    {
        bail!(
            "active_slot_coeff must be finite and within (0, 1], got {}",
            file_cfg.active_slot_coeff
        );
    }

    let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
    let immutable_dir = storage_dir.join("immutable");
    let volatile_dir = storage_dir.join("volatile");
    let ledger_dir = storage_dir.join("ledger");

    let mut warnings = Vec::new();
    if file_cfg.checkpoint_interval_slots == 0 {
        warnings.push(
            "checkpoint_interval_slots is 0; checkpoint persistence cadence is effectively unbounded"
                .to_owned(),
        );
    }
    if file_cfg.max_ledger_snapshots == 0 {
        warnings.push(
            "max_ledger_snapshots is 0; persisted ledger checkpoints will be pruned immediately"
                .to_owned(),
        );
    }
    if !(file_cfg.turn_on_logging && file_cfg.use_trace_dispatcher) {
        warnings.push("runtime tracing is disabled for local operator output".to_owned());
    }
    if !file_cfg.turn_on_log_metrics {
        warnings.push("trace metrics production is disabled".to_owned());
    }
    let peer_snapshot = if let Some(peer_snapshot_file) = file_cfg.peer_snapshot_file.as_deref() {
        let peer_snapshot_path =
            resolve_config_path(std::path::Path::new(peer_snapshot_file), config_base_dir);
        match load_peer_snapshot_file(&peer_snapshot_path) {
            Ok(loaded) => PeerSnapshotValidationReport {
                status: "loaded",
                path: Some(peer_snapshot_path.display().to_string()),
                slot: loaded.slot,
                ledger_peer_count: loaded.snapshot.ledger_peers.len(),
                big_ledger_peer_count: loaded.snapshot.big_ledger_peers.len(),
                error: None,
            },
            Err(err) => {
                warnings.push(format!(
                    "configured peer snapshot file could not be loaded: {}",
                    err
                ));
                PeerSnapshotValidationReport {
                    status: "unavailable",
                    path: Some(peer_snapshot_path.display().to_string()),
                    slot: None,
                    ledger_peer_count: 0,
                    big_ledger_peer_count: 0,
                    error: Some(err.to_string()),
                }
            }
        }
    } else {
        PeerSnapshotValidationReport {
            status: "disabled",
            path: None,
            slot: None,
            ledger_peer_count: 0,
            big_ledger_peer_count: 0,
            error: None,
        }
    };

    let (storage, latest_slot, ledger_state_judgement, ledger_snapshot) = if immutable_dir.exists()
        || volatile_dir.exists()
        || ledger_dir.exists()
    {
        let base_ledger_state = best_effort_base_ledger_state(file_cfg, config_base_dir);
        let chain_db = ChainDb::new(
            FileImmutable::open(&immutable_dir)
                .wrap_err_with(|| format!("failed to open immutable store {}", immutable_dir.display()))?,
            FileVolatile::open(&volatile_dir)
                .wrap_err_with(|| format!("failed to open volatile store {}", volatile_dir.display()))?,
            FileLedgerStore::open(&ledger_dir)
                .wrap_err_with(|| format!("failed to open ledger store {}", ledger_dir.display()))?,
        );
        let tip = chain_db.recovery().tip;
        let recovery = recover_ledger_state_chaindb(&chain_db, base_ledger_state)
            .wrap_err_with(|| {
                format!(
                    "failed to recover ledger state from storage directory {}",
                    storage_dir.display()
                )
            })?;
        let latest_slot = point_slot(&recovery.point).or_else(|| point_slot(&tip));
        let ledger_snapshot = ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state);
        (
            StorageValidationReport {
                status: "initialized",
                tip: format!("{:?}", tip),
                recovered_point: Some(format!("{:?}", recovery.point)),
                checkpoint_slot: recovery.checkpoint_slot.map(|slot| slot.0),
                replayed_volatile_blocks: Some(recovery.replayed_volatile_blocks),
                ledger_peer_count: ledger_snapshot.ledger_peers.len(),
            },
            latest_slot,
            LedgerStateJudgement::YoungEnough,
            ledger_snapshot,
        )
    } else {
        warnings.push(
            "storage directories are not initialized; a deployment preflight cannot validate restart recovery yet"
                .to_owned(),
        );
        (
            StorageValidationReport {
                status: "not-initialized",
                tip: format!("{:?}", Point::Origin),
                recovered_point: None,
                checkpoint_slot: None,
                replayed_volatile_blocks: None,
                ledger_peer_count: 0,
            },
            None,
            LedgerStateJudgement::Unavailable,
            LedgerPeerSnapshot::default(),
        )
    };

    let fallback_peers = configured_fallback_peers(
        file_cfg,
        config_base_dir,
        &ledger_snapshot,
        latest_slot,
        ledger_state_judgement,
        &NodeTracer::disabled(),
    );

    Ok(ConfigValidationReport {
        primary_peer: file_cfg.peer_addr.to_string(),
        network_magic: file_cfg.network_magic,
        protocol_versions: file_cfg.protocol_versions.clone(),
        storage_dir: storage_dir.display().to_string(),
        configured_fallback_peer_count: file_cfg.ordered_fallback_peers().len(),
        resolved_startup_peer_count: 1 + fallback_peers.len(),
        use_ledger_peers: format!("{:?}", file_cfg.use_ledger_peers_policy()),
        checkpoint_interval_slots: file_cfg.checkpoint_interval_slots,
        max_ledger_snapshots: file_cfg.max_ledger_snapshots,
        peer_snapshot,
        storage,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// status subcommand
// ---------------------------------------------------------------------------

/// On-disk node status report produced by the `status` subcommand.
#[derive(Serialize)]
struct StatusReport {
    network_magic: u32,
    storage_dir: String,
    storage_initialized: bool,
    chain_tip: String,
    chain_tip_slot: Option<u64>,
    chain_tip_hash: Option<String>,
    immutable_tip: String,
    immutable_block_count: usize,
    volatile_tip: String,
    volatile_block_count: usize,
    ledger_checkpoint_slot: Option<u64>,
    ledger_checkpoint_count: usize,
    replayed_volatile_blocks: Option<usize>,
    recovered_ledger_point: Option<String>,
}

fn status_report(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
) -> Result<StatusReport> {
    let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir);
    let immutable_dir = storage_dir.join("immutable");
    let volatile_dir = storage_dir.join("volatile");
    let ledger_dir = storage_dir.join("ledger");

    if !(immutable_dir.exists() || volatile_dir.exists() || ledger_dir.exists()) {
        return Ok(StatusReport {
            network_magic: file_cfg.network_magic,
            storage_dir: storage_dir.display().to_string(),
            storage_initialized: false,
            chain_tip: format!("{:?}", Point::Origin),
            chain_tip_slot: None,
            chain_tip_hash: None,
            immutable_tip: format!("{:?}", Point::Origin),
            immutable_block_count: 0,
            volatile_tip: format!("{:?}", Point::Origin),
            volatile_block_count: 0,
            ledger_checkpoint_slot: None,
            ledger_checkpoint_count: 0,
            replayed_volatile_blocks: None,
            recovered_ledger_point: None,
        });
    }

    let chain_db = ChainDb::new(
        FileImmutable::open(immutable_dir)
            .wrap_err("failed to open immutable store")?,
        FileVolatile::open(volatile_dir)
            .wrap_err("failed to open volatile store")?,
        FileLedgerStore::open(ledger_dir)
            .wrap_err("failed to open ledger store")?,
    );

    let chain_tip = chain_db.tip();
    let immutable_tip = chain_db.immutable().get_tip();
    let volatile_tip = chain_db.volatile().tip();
    let immutable_block_count = chain_db.immutable().len();

    // Count volatile blocks by walking the prefix up to the volatile tip.
    let volatile_block_count: usize = if volatile_tip != Point::Origin {
        chain_db
            .volatile()
            .prefix_up_to(&volatile_tip)
            .map(|blocks| blocks.len())
            .unwrap_or(0)
    } else {
        0
    };

    let ledger_checkpoint_count = LedgerStore::count(chain_db.ledger());
    let recovery = recover_ledger_state_chaindb(
        &chain_db,
        best_effort_base_ledger_state(file_cfg, config_base_dir),
    );

    let (chain_tip_slot, chain_tip_hash) = match &chain_tip {
        Point::Origin => (None, None),
        Point::BlockPoint(slot, hash) => (Some(slot.0), Some(format!("{hash:?}"))),
    };

    Ok(StatusReport {
        network_magic: file_cfg.network_magic,
        storage_dir: storage_dir.display().to_string(),
        storage_initialized: true,
        chain_tip: format!("{chain_tip:?}"),
        chain_tip_slot,
        chain_tip_hash,
        immutable_tip: format!("{immutable_tip:?}"),
        immutable_block_count,
        volatile_tip: format!("{volatile_tip:?}"),
        volatile_block_count,
        ledger_checkpoint_slot: recovery
            .as_ref()
            .ok()
            .and_then(|r| r.checkpoint_slot.map(|s| s.0)),
        ledger_checkpoint_count,
        replayed_volatile_blocks: recovery.as_ref().ok().map(|r| r.replayed_volatile_blocks),
        recovered_ledger_point: recovery
            .as_ref()
            .ok()
            .map(|r| format!("{:?}", r.point)),
    })
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

fn resolve_config_path(path: &std::path::Path, config_base_dir: Option<&std::path::Path>) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base_dir) = config_base_dir {
        base_dir.join(path)
    } else {
        path.to_path_buf()
    }
}

fn point_slot(point: &Point) -> Option<u64> {
    match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    }
}

fn extend_unique_peers(target: &mut Vec<SocketAddr>, peers: impl IntoIterator<Item = SocketAddr>) {
    for peer in peers {
        if !target.contains(&peer) {
            target.push(peer);
        }
    }
}

fn extend_unique_ledger_peers(
    target: &mut Vec<SocketAddr>,
    access_points: impl IntoIterator<Item = PoolRelayAccessPoint>,
) {
    for access_point in access_points {
        let peer_access_point = PeerAccessPoint {
            address: access_point.address,
            port: access_point.port,
        };
        extend_unique_peers(target, resolve_peer_access_points(&peer_access_point));
    }
}

fn merge_ledger_peer_snapshots(
    ledger_snapshot: &LedgerPeerSnapshot,
    snapshot_file: Option<LedgerPeerSnapshot>,
) -> LedgerPeerSnapshot {
    let mut merged_ledger_peers = ledger_snapshot.ledger_peers.clone();
    let mut merged_big_ledger_peers = ledger_snapshot.big_ledger_peers.clone();

    if let Some(snapshot_file) = snapshot_file {
        extend_unique_peers(&mut merged_ledger_peers, snapshot_file.ledger_peers);
        extend_unique_peers(
            &mut merged_big_ledger_peers,
            snapshot_file.big_ledger_peers,
        );
    }

    LedgerPeerSnapshot::new(merged_ledger_peers, merged_big_ledger_peers)
}

fn ledger_peer_snapshot_from_ledger_state(ledger_state: &LedgerState) -> LedgerPeerSnapshot {
    let mut ledger_peers = Vec::new();
    extend_unique_ledger_peers(&mut ledger_peers, ledger_state.pool_state().relay_access_points());
    LedgerPeerSnapshot::new(ledger_peers, Vec::new())
}

fn configured_fallback_peers(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
    ledger_snapshot: &LedgerPeerSnapshot,
    latest_slot: Option<u64>,
    ledger_state_judgement: LedgerStateJudgement,
    tracer: &NodeTracer,
) -> Vec<SocketAddr> {
    let mut fallback_peers = file_cfg.ordered_fallback_peers();

    let mut snapshot_slot = None;
    let mut snapshot_available = file_cfg.peer_snapshot_file.is_none();
    let mut snapshot_path = None;
    let mut snapshot_file = None;

    if let Some(peer_snapshot_file) = file_cfg.peer_snapshot_file.as_deref() {
        let peer_snapshot_path = resolve_config_path(std::path::Path::new(peer_snapshot_file), config_base_dir);
        snapshot_path = Some(peer_snapshot_path.clone());

        match load_peer_snapshot_file(&peer_snapshot_path) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                snapshot_file = Some(loaded_snapshot.snapshot);
            }
            Err(err) => {
                let freshness = file_cfg.peer_snapshot_freshness(None, latest_slot, false);
                let (decision, _) = file_cfg.eligible_ledger_fallback_peers(
                    ledger_snapshot,
                    latest_slot,
                    ledger_state_judgement,
                    freshness,
                );

                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to load peer snapshot fallbacks",
                    trace_fields([
                        ("decision", json!(format!("{decision:?}"))),
                        ("latestSlot", json!(latest_slot)),
                        ("snapshotPath", json!(peer_snapshot_path.display().to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    let combined_snapshot = merge_ledger_peer_snapshots(ledger_snapshot, snapshot_file);
    let freshness = file_cfg.peer_snapshot_freshness(snapshot_slot, latest_slot, snapshot_available);
    let (decision, eligible_peers) = file_cfg.eligible_ledger_fallback_peers(
        &combined_snapshot,
        latest_slot,
        ledger_state_judgement,
        freshness,
    );
    let snapshot_peer_count = eligible_peers.len();
    extend_unique_peers(&mut fallback_peers, eligible_peers);

    tracer.trace_runtime(
        "Net.PeerSelection",
        "Info",
        "evaluated ledger-derived startup fallbacks",
        trace_fields([
            ("decision", json!(format!("{decision:?}"))),
            ("latestSlot", json!(latest_slot)),
            ("snapshotSlot", json!(snapshot_slot)),
            (
                "snapshotPath",
                json!(snapshot_path.map(|path| path.display().to_string())),
            ),
            ("ledgerPeerCount", json!(combined_snapshot.ledger_peers.len())),
            ("bigLedgerPeerCount", json!(combined_snapshot.big_ledger_peers.len())),
            ("eligiblePeerCount", json!(snapshot_peer_count)),
        ]),
    );

    fallback_peers
}

fn checkpoint_trace_config_mut(file_cfg: &mut NodeConfigFile) -> &mut TraceNamespaceConfig {
    file_cfg
        .trace_options
        .entry(CHECKPOINT_TRACE_NAMESPACE.to_owned())
    .or_default()
}

async fn run_node(
    request: RunNodeRequest,
) -> Result<()> {
    let RunNodeRequest {
        node_config,
        bootstrap_peers,
        sync_config,
        governor_config,
        topology_config,
        tracer,
        storage_dir,
        chain_db,
        inbound_listen_addr,
        use_ledger_peers,
        peer_snapshot_path,
        metrics_port,
        base_ledger_state,
    } = request;

    let chain_db = Arc::new(RwLock::new(chain_db));
    let peer_registry = Arc::new(RwLock::new(seed_peer_registry(
        node_config.peer_addr,
        &topology_config,
    )));

    let metrics = std::sync::Arc::new(NodeMetrics::new());

    // Optionally spawn the Prometheus metrics HTTP endpoint.
    if let Some(port) = metrics_port {
        let metrics_ref = std::sync::Arc::clone(&metrics);
        tokio::spawn(async move {
            if let Err(err) = serve_metrics(port, metrics_ref).await {
                eprintln!("metrics server error: {err}");
            }
        });
    }

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

    let nonce_state = sync_config
        .nonce_config
        .as_ref()
        .map(|_| NonceEvolutionState::new(Nonce::Neutral));

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn signal handler for graceful shutdown.
    let signal_tracer = tracer.clone();
    let signal_shutdown_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        signal_tracer.trace_runtime(
            "Node.Shutdown",
            "Notice",
            "shutdown signal received",
            std::collections::BTreeMap::new(),
        );
        let _ = signal_shutdown_tx.send(true);
    });

    // Shared mempool for governor TTL purge and inbound TxSubmission admission.
    let shared_mempool = SharedMempool::default();

    let governor_task = {
        let mut governor_shutdown = shutdown_rx.clone();
        let governor_node_config = node_config.clone();
        let governor_chain_db = Arc::clone(&chain_db);
        let governor_registry = Arc::clone(&peer_registry);
        let governor_tracer = tracer.clone();
        let governor_topology = topology_config.clone();
        let governor_base_ledger_state = base_ledger_state.clone();
        let governor_mempool = shared_mempool.clone();
        tokio::spawn(async move {
            let shutdown = async move {
                if *governor_shutdown.borrow() {
                    return;
                }
                while governor_shutdown.changed().await.is_ok() {
                    if *governor_shutdown.borrow() {
                        break;
                    }
                }
            };

            run_governor_loop(
                governor_node_config,
                governor_chain_db,
                governor_registry,
                GovernorState::default(),
                governor_config,
                governor_topology,
                governor_base_ledger_state,
                Some(governor_mempool),
                governor_tracer,
                shutdown,
            ).await;
        })
    };

    let inbound_task = if let Some(listen_addr) = inbound_listen_addr {
        let listener = PeerListener::bind(
            listen_addr,
            node_config.network_magic,
            node_config.protocol_versions.clone(),
        )
        .await?;
        let bound_addr = listener.local_addr().unwrap_or(listen_addr);
        tracer.trace_runtime(
            "Net.Inbound",
            "Notice",
            "inbound listener bound",
            trace_fields([("listenAddr", json!(bound_addr.to_string()))]),
        );

        let shared_provider = Arc::new(SharedChainDb::from_arc(Arc::clone(&chain_db)));
        let block_provider: Arc<dyn BlockProvider> = shared_provider.clone();
        let chain_provider: Arc<dyn ChainProvider> = shared_provider;
        let tx_submission_consumer = Arc::new(SharedTxSubmissionConsumer::new(
            Arc::clone(&chain_db),
            shared_mempool.clone(),
        ));
        let peer_sharing = Arc::new(SharedPeerSharingProvider::new(
            Arc::clone(&peer_registry),
        ));
        let mut inbound_shutdown = shutdown_rx.clone();
        let inbound_tracer = tracer.clone();

        Some(tokio::spawn(async move {
            let shutdown = async move {
                if *inbound_shutdown.borrow() {
                    return;
                }
                while inbound_shutdown.changed().await.is_ok() {
                    if *inbound_shutdown.borrow() {
                        break;
                    }
                }
            };

            if let Err(err) = run_inbound_accept_loop(
                &listener,
                Some(block_provider),
                Some(chain_provider),
                Some(tx_submission_consumer),
                Some(peer_sharing),
                shutdown,
            )
            .await
            {
                inbound_tracer.trace_runtime(
                    "Net.Inbound",
                    "Error",
                    "inbound listener stopped with error",
                    trace_fields([("error", json!(err.to_string()))]),
                );
            }
        }))
    } else {
        None
    };

    let request = ResumeReconnectingVerifiedSyncRequest::new(
        &node_config,
        &bootstrap_peers,
        base_ledger_state,
        &sync_config,
    )
    .with_nonce_state(nonce_state)
    .with_use_ledger_peers(use_ledger_peers)
    .with_peer_snapshot_path(peer_snapshot_path)
    .with_metrics(Some(&metrics));

    let mut sync_shutdown = shutdown_rx.clone();
    let outcome: ResumedSyncServiceOutcome = match resume_reconnecting_verified_sync_service_shared_chaindb(
        &chain_db,
        request,
        async move {
            if *sync_shutdown.borrow() {
                return;
            }
            while sync_shutdown.changed().await.is_ok() {
                if *sync_shutdown.borrow() {
                    break;
                }
            }
        },
    )
    .await {
        Ok(outcome) => outcome,
        Err(err) => {
            let _ = shutdown_tx.send(true);
            let _ = governor_task.await;
            if let Some(handle) = inbound_task {
                let _ = handle.await;
            }
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

    let _ = shutdown_tx.send(true);
    let _ = governor_task.await;
    if let Some(handle) = inbound_task {
        let _ = handle.await;
    }

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

struct RunNodeRequest {
    node_config: NodeConfig,
    bootstrap_peers: Vec<SocketAddr>,
    sync_config: VerifiedSyncServiceConfig,
    governor_config: RuntimeGovernorConfig,
    topology_config: yggdrasil_network::TopologyConfig,
    tracer: NodeTracer,
    storage_dir: PathBuf,
    chain_db: ChainDb<FileImmutable, FileVolatile, FileLedgerStore>,
    inbound_listen_addr: Option<SocketAddr>,
    use_ledger_peers: Option<yggdrasil_network::UseLedgerPeers>,
    peer_snapshot_path: Option<PathBuf>,
    metrics_port: Option<u16>,
    /// Genesis-seeded base ledger state used for recovery and fresh sync.
    base_ledger_state: LedgerState,
}

// ---------------------------------------------------------------------------
// Prometheus metrics HTTP endpoint
// ---------------------------------------------------------------------------

/// Lightweight HTTP handler that responds with Prometheus exposition text on
/// `GET /metrics`, a JSON snapshot on `GET /metrics/json`, and a simple health
/// check on `GET /health`.
///
/// Uses raw tokio TCP — no HTTP framework dependency required.
async fn serve_metrics(
    port: u16,
    metrics: std::sync::Arc<NodeMetrics>,
) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    loop {
        let (mut stream, _addr) = listener.accept().await?;
        let metrics = std::sync::Arc::clone(&metrics);
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let request = String::from_utf8_lossy(&buf[..n]);

            let (status, content_type, body) = if request.starts_with("GET /health") {
                let snap = metrics.snapshot();
                let body = serde_json::json!({
                    "status": "ok",
                    "uptime_seconds": snap.uptime_ms / 1000,
                    "blocks_synced": snap.blocks_synced,
                    "current_slot": snap.current_slot,
                })
                .to_string();
                ("200 OK", "application/json", body)
            } else if request.starts_with("GET /metrics/json") {
                let snap = metrics.snapshot();
                match serde_json::to_string_pretty(&snap) {
                    Ok(json) => ("200 OK", "application/json", json),
                    Err(_) => ("500 Internal Server Error", "text/plain", "serialization error".to_owned()),
                }
            } else if request.starts_with("GET /metrics") {
                let body = metrics.snapshot().to_prometheus_text();
                ("200 OK", "text/plain; version=0.0.4; charset=utf-8", body)
            } else {
                ("404 Not Found", "text/plain", "not found\n".to_owned())
            };

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHECKPOINT_TRACE_NAMESPACE, checkpoint_trace_config_mut,
        configured_fallback_peers, ledger_peer_snapshot_from_ledger_state,
        load_effective_config, preset_config_base_dir, status_report,
        validate_config_report,
    };
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use yggdrasil_ledger::{PoolParams, Relay, RewardAccount, StakeCredential, UnitInterval};
    use yggdrasil_network::{LedgerPeerSnapshot, LedgerStateJudgement};
    use yggdrasil_node::config::default_config;
    use yggdrasil_node::tracer::NodeTracer;

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

    #[test]
    fn ledger_peer_snapshot_from_ledger_state_uses_registered_pool_relays() {
        let mut ledger_state = yggdrasil_ledger::LedgerState::new(yggdrasil_ledger::Era::Shelley);
        ledger_state.pool_state_mut().register(PoolParams {
            operator: [1; 28],
            vrf_keyhash: [2; 32],
            pledge: 1,
            cost: 1,
            margin: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: StakeCredential::AddrKeyHash([3; 28]),
            },
            pool_owners: vec![[4; 28]],
            relays: vec![Relay::SingleHostAddr(Some(3001), Some([127, 0, 0, 9]), None)],
            pool_metadata: None,
        });

        let snapshot = ledger_peer_snapshot_from_ledger_state(&ledger_state);
        assert_eq!(
            snapshot,
            LedgerPeerSnapshot::new(
                ["127.0.0.9:3001".parse().expect("peer")],
                Vec::new(),
            )
        );
    }

    #[test]
    fn configured_fallback_peers_appends_eligible_ledger_state_peers() {
        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(0);
        cfg.peer_snapshot_file = None;
        let tracer = NodeTracer::from_config(&cfg);
        let ledger_snapshot = LedgerPeerSnapshot::new(
            ["127.0.0.9:3001".parse().expect("peer")],
            Vec::new(),
        );

        let fallback_peers = configured_fallback_peers(
            &cfg,
            None,
            &ledger_snapshot,
            Some(1),
            LedgerStateJudgement::YoungEnough,
            &tracer,
        );

        assert!(fallback_peers.contains(&"127.0.0.9:3001".parse().expect("peer")));
    }

    #[test]
    fn configured_fallback_peers_merges_snapshot_big_ledger_peers() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-peer-snapshot-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let snapshot_path = dir.join("peer-snapshot.json");
        std::fs::write(
            &snapshot_path,
            r#"{
                "version": 2,
                "slotNo": 10,
                "bigLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.10", "port": 3001 }
                        ]
                    }
                ]
            }"#,
        )
        .expect("write snapshot");

        let mut cfg = default_config();
        cfg.use_ledger_after_slot = Some(0);
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());
        let tracer = NodeTracer::from_config(&cfg);
        let ledger_snapshot = LedgerPeerSnapshot::new(
            ["127.0.0.9:3001".parse().expect("peer")],
            Vec::new(),
        );

        let fallback_peers = configured_fallback_peers(
            &cfg,
            Some(&dir),
            &ledger_snapshot,
            Some(10),
            LedgerStateJudgement::YoungEnough,
            &tracer,
        );

        assert!(fallback_peers.contains(&"127.0.0.9:3001".parse().expect("ledger")));
        assert!(fallback_peers.contains(&"127.0.0.10:3001".parse().expect("big ledger")));

        std::fs::remove_file(snapshot_path).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_warns_when_storage_is_uninitialized() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-validate-config-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = validate_config_report(&cfg, Some(&dir)).expect("validation report");

        assert_eq!(report.storage.status, "not-initialized");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("storage directories are not initialized")));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_loads_configured_peer_snapshot() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-validate-snapshot-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let snapshot_path = dir.join("peer-snapshot.json");
        std::fs::write(
            &snapshot_path,
            r#"{
                "version": 2,
                "slotNo": 10,
                "allLedgerPools": [
                    {
                        "accumulatedStake": 0.75,
                        "relativeStake": 0.50,
                        "relays": [
                            { "address": "127.0.0.11", "port": 3001 }
                        ]
                    }
                ]
            }"#,
        )
        .expect("write snapshot");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = Some("peer-snapshot.json".to_owned());

        let report = validate_config_report(&cfg, Some(&dir)).expect("validation report");

        assert_eq!(report.peer_snapshot.status, "loaded");
        assert_eq!(report.peer_snapshot.slot, Some(10));
        assert_eq!(report.peer_snapshot.ledger_peer_count, 1);
        assert_eq!(report.peer_snapshot.error, None);

        std::fs::remove_file(snapshot_path).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn validate_config_report_rejects_invalid_active_slot_coeff() {
        let mut cfg = default_config();
        cfg.active_slot_coeff = 0.0;

        assert!(validate_config_report(&cfg, None).is_err());
    }

    #[test]
    fn load_effective_config_uses_network_preset_when_file_is_absent() {
        let (cfg, config_base_dir) =
            load_effective_config(None, Some(yggdrasil_node::config::NetworkPreset::Preview))
                .expect("preset config");

        assert_eq!(cfg.network_magic, 2);
        assert_eq!(
            config_base_dir,
            Some(preset_config_base_dir(yggdrasil_node::config::NetworkPreset::Preview))
        );
    }

    #[test]
    fn validate_config_report_warns_when_peer_snapshot_file_is_missing() {
        let (cfg, config_base_dir) =
            load_effective_config(None, Some(yggdrasil_node::config::NetworkPreset::Preview))
                .expect("preset config");

        let report = validate_config_report(&cfg, config_base_dir.as_deref())
            .expect("validation report");

        assert_eq!(report.peer_snapshot.status, "unavailable");
        assert!(report.peer_snapshot.error.is_some());
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("configured peer snapshot file could not be loaded")));
    }

    #[test]
    fn status_report_shows_uninitialized_when_storage_absent() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-status-empty-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = status_report(&cfg, Some(&dir)).expect("status report");

        assert!(!report.storage_initialized);
        assert_eq!(report.immutable_block_count, 0);
        assert_eq!(report.volatile_block_count, 0);
        assert_eq!(report.ledger_checkpoint_count, 0);
        assert!(report.chain_tip_slot.is_none());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn status_report_shows_initialized_when_storage_exists() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-status-init-{unique}"));
        let data_dir = dir.join("data");
        std::fs::create_dir_all(data_dir.join("immutable")).expect("immutable dir");
        std::fs::create_dir_all(data_dir.join("volatile")).expect("volatile dir");
        std::fs::create_dir_all(data_dir.join("ledger")).expect("ledger dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = status_report(&cfg, Some(&dir)).expect("status report");

        assert!(report.storage_initialized);
        assert_eq!(report.immutable_block_count, 0);
        assert_eq!(report.volatile_block_count, 0);
        assert!(report.chain_tip.contains("Origin"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn status_report_serializes_to_json() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("yggdrasil-status-json-{unique}"));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut cfg = default_config();
        cfg.storage_dir = PathBuf::from("data");
        cfg.peer_snapshot_file = None;

        let report = status_report(&cfg, Some(&dir)).expect("status report");
        let json = serde_json::to_string_pretty(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(parsed["network_magic"], serde_json::Value::from(764_824_073u64));
        assert_eq!(parsed["storage_initialized"], serde_json::Value::Bool(false));

        std::fs::remove_dir_all(dir).ok();
    }
}
