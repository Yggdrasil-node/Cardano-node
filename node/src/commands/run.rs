//! `run` subcommand: orchestrate node startup from operator CLI args.
//!
//! Builds a [`RunNodeRequest`] from the operator's `--config` /
//! `--topology` / per-flag overrides, recovers existing on-disk
//! storage, seeds the genesis-aware base ledger state, and hands the
//! request to [`run_node`] which mirrors upstream
//! `Cardano.Node.Run.run`.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Run.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side `run` subcommand —
//! the canonical node run-loop entry point. Mirrors upstream
//! `Cardano.Node.Run.runNode`. Upstream's `runNode` is the
//! main entry; Yggdrasil's `run.rs` parses the CLI flag-set
//! and delegates to `run_node.rs`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Result, WrapErr};
use serde_json::json;

use yggdrasil_consensus::{
    ActiveSlotCoeff, ClockSkew, EpochSize, NonceEvolutionConfig, OcertCounters, SecurityParam,
};
use yggdrasil_ledger::Nonce;
use yggdrasil_network::{
    BlockFetchInstrumentation, GovernorTargets, HandshakeVersion, LedgerStateJudgement,
    NodePeerSharing, blockfetch_pool::BlockFetchPool, blockfetch_pool::FetchMode,
};
use yggdrasil_node::genesis;
use yggdrasil_node::tracer::{NodeTracer, trace_fields};
use yggdrasil_node::{
    FutureBlockCheckConfig, LedgerCheckpointPolicy, NodeConfig, RuntimeGovernorConfig,
    VerificationConfig, VerifiedSyncServiceConfig, recover_ledger_state_chaindb_epoch_boundary,
};
use yggdrasil_storage::{ChainDb, FileImmutable, FileLedgerStore, FileVolatile};

use crate::commands::configuration::{
    apply_block_producer_credential_overrides, apply_inbound_listen_overrides,
    apply_topology_override, checkpoint_trace_config_mut, load_effective_config,
};
use crate::commands::validate_config::{
    load_configured_block_producer_credentials, node_role_report,
};
use crate::run_node::{RunNodeRequest, run_node};
use crate::{
    configured_fallback_peers, ledger_peer_snapshot_from_ledger_state, point_slot,
    resolve_config_path, resolve_storage_dir, strict_base_ledger_state,
    trace_genesis_hashes_verified,
};
use yggdrasil_node::config::NetworkPreset;

/// Operator-supplied arguments for the `run` subcommand.  Matches the
/// `Command::Run` clap variant 1:1 by field name.
pub(crate) struct RunCmdArgs {
    pub(crate) config: Option<PathBuf>,
    pub(crate) network: Option<NetworkPreset>,
    pub(crate) topology: Option<PathBuf>,
    pub(crate) peer: Option<SocketAddr>,
    pub(crate) network_magic: Option<u32>,
    pub(crate) database_path: Option<PathBuf>,
    pub(crate) port: Option<u16>,
    pub(crate) host_addr: Option<String>,
    pub(crate) no_verify: bool,
    pub(crate) batch_size: usize,
    pub(crate) checkpoint_interval_slots: Option<u64>,
    pub(crate) max_ledger_snapshots: Option<usize>,
    pub(crate) checkpoint_trace_max_frequency: Option<f64>,
    pub(crate) checkpoint_trace_severity: Option<String>,
    pub(crate) checkpoint_trace_backend: Vec<String>,
    pub(crate) metrics_port: Option<u16>,
    pub(crate) non_producing_node: bool,
    pub(crate) max_concurrent_block_fetch_peers: Option<u8>,
    pub(crate) socket_path: Option<PathBuf>,
    pub(crate) shelley_kes_key: Option<PathBuf>,
    pub(crate) shelley_vrf_key: Option<PathBuf>,
    pub(crate) shelley_operational_certificate: Option<PathBuf>,
    pub(crate) shelley_operational_certificate_issuer_vkey: Option<PathBuf>,
}

/// Drive the `run` subcommand: build a [`RunNodeRequest`] from the
/// CLI args, recover storage, seed the base ledger state, then call
/// [`run_node`] to start the runtime.
pub(crate) fn run_subcommand(args: RunCmdArgs) -> Result<()> {
    let RunCmdArgs {
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
    } = args;

    let (mut file_cfg, config_base_dir) = load_effective_config(config, network)?;
    // CLI --max-concurrent-block-fetch-peers overrides the config
    // file value.  Used by the §6.5 runbook rehearsal to flip the
    // multi-peer BlockFetch dispatch on without editing the
    // vendored config files.
    if let Some(knob) = max_concurrent_block_fetch_peers {
        file_cfg.max_concurrent_block_fetch_peers = knob;
    }
    apply_topology_override(
        &mut file_cfg,
        topology.as_deref(),
        config_base_dir.as_deref(),
    )?;

    // CLI --database-path overrides config file storage_dir.
    if let Some(ref db_path) = database_path {
        file_cfg.storage_dir = db_path.clone();
    }

    apply_inbound_listen_overrides(&mut file_cfg, port, host_addr)?;

    // CLI --socket-path overrides config file SocketPath.
    if let Some(ref sp) = socket_path {
        file_cfg.socket_path = Some(sp.display().to_string());
    }

    apply_block_producer_credential_overrides(
        &mut file_cfg,
        shelley_kes_key.as_ref(),
        shelley_vrf_key.as_ref(),
        shelley_operational_certificate.as_ref(),
        shelley_operational_certificate_issuer_vkey.as_ref(),
    );

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

    // Resolve the storage directory. Chain-dependent consensus state
    // is restored later by the coordinated runtime from the exact
    // slot-indexed ChainDepState sidecar at the recovered point.
    let storage_dir = resolve_storage_dir(&file_cfg.storage_dir, config_base_dir.as_deref());
    let initial_ocert_counters = OcertCounters::new();

    // Load the slot length and system start from shelley genesis for the
    // block producer's slot clock and the blocks-from-the-future check.
    // Falls back to 1.0 s slot length when the genesis file is missing.
    let shelley_genesis: Option<genesis::ShelleyGenesis> =
        file_cfg.shelley_genesis_file.as_deref().and_then(|path| {
            let full_path = if let Some(base) = config_base_dir.as_deref() {
                base.join(std::path::Path::new(path))
            } else {
                std::path::PathBuf::from(path)
            };
            genesis::load_shelley_genesis(&full_path).ok()
        });
    let genesis_slot_length: Option<f64> = shelley_genesis.as_ref().map(|g| g.slot_length);
    let genesis_system_start_unix_secs: Option<f64> = shelley_genesis
        .as_ref()
        .and_then(|g| g.system_start.as_deref())
        .and_then(genesis::chrono_parse_system_start);

    // Compute FutureBlockCheckConfig from genesis `system_start` and
    // slot length. The wall slot is derived dynamically per check.
    // Reference: `Ouroboros.Consensus.MiniProtocol.ChainSync.Client.InFutureCheck`
    let future_check: Option<FutureBlockCheckConfig> = shelley_genesis
        .as_ref()
        .and_then(|g| g.system_start.as_deref())
        .and_then(|start| {
            let slot_len = genesis_slot_length.unwrap_or(1.0);
            let system_start_unix_secs = genesis::chrono_parse_system_start(start)?;
            let clock_skew =
                ClockSkew::default_for_slot_length(std::time::Duration::from_secs_f64(slot_len));
            Some(FutureBlockCheckConfig {
                system_start_unix_secs,
                slot_length_secs: slot_len,
                clock_skew,
            })
        });

    // Construct a single shared BlockFetch instrumentation pool
    // that is observed by both the sync runtime (per-peer dispatch
    // / success / failure counters) and the governor (per-peer
    // concurrency cap driven by ledger-state judgement). Mirrors
    // upstream `mkReadFetchMode` from
    // `Ouroboros.Network.BlockFetch.ConsensusInterface`, which is
    // the single source of truth for the BlockFetch decision
    // policy's `bfcMaxConcurrency{BulkSync,Deadline}` selection.
    // Initialised in `FetchModeBulkSync` (the upstream default at
    // startup, before any judgement update); the governor tick
    // overwrites it on every tick once the live `LedgerStateJudgement`
    // is available.
    let block_fetch_pool: BlockFetchInstrumentation = Arc::new(std::sync::Mutex::new(
        BlockFetchPool::new(FetchMode::FetchModeBulkSync),
    ));

    let verification = if no_verify {
        None
    } else {
        Some(VerificationConfig {
            slots_per_kes_period: file_cfg.slots_per_kes_period,
            max_kes_evolutions: file_cfg.max_kes_evolutions,
            verify_body_hash: true,
            max_major_protocol_version: Some(file_cfg.max_major_protocol_version),
            future_check,
            ocert_counters: Some(initial_ocert_counters.clone()),
            pp_major_protocol_version: None,
            network_magic: Some(file_cfg.network_magic),
        })
    };

    let nonce_config = NonceEvolutionConfig {
        epoch_size: EpochSize(file_cfg.epoch_length),
        // stability_window = 3k/f
        stability_window: (3.0 * file_cfg.security_param_k as f64 / file_cfg.active_slot_coeff)
            as u64,
        extra_entropy: genesis::genesis_extra_entropy_to_nonce(
            shelley_genesis
                .as_ref()
                .and_then(|g| g.protocol_params.extra_entropy.as_ref()),
        )
        .wrap_err("invalid Shelley genesis extraEntropy")?,
        // R262 — feed the era-aware Byron→Shelley boundary so
        // `apply_block`'s epoch math doesn't fire `tick_epoch_transition`
        // at every multiple of `epoch_size` from slot 0.
        // For Shelley-only chains (preview) this stays `None`.
        byron_shelley_transition: file_cfg.byron_to_shelley_slot.map(|boundary| {
            (
                boundary,
                file_cfg
                    .first_shelley_epoch
                    .unwrap_or(boundary / file_cfg.byron_epoch_length.max(1)),
            )
        }),
    };

    let security_param = SecurityParam(file_cfg.security_param_k);
    let checkpoint_interval_slots =
        checkpoint_interval_slots.unwrap_or(file_cfg.checkpoint_interval_slots);
    let max_ledger_snapshots = max_ledger_snapshots.unwrap_or(file_cfg.max_ledger_snapshots);

    let active_slot_coeff = ActiveSlotCoeff::new(file_cfg.active_slot_coeff).ok();

    // Slice GD-Final — single density registry shared between
    // the sync service (writer: ChainSync RollForward hook) and
    // the governor loop (reader: density-aware hot-demotion
    // scoring). Cloning the Arc keeps both ends pointing at the
    // same `BTreeMap<SocketAddr, DensityWindow>`.
    let density_registry = yggdrasil_node::sync::new_density_registry();

    // Phase 6 — shared FetchWorkerPool for upstream-faithful
    // multi-peer BlockFetch dispatch.  Cloned into both the
    // sync-service config (reader path) and any future
    // governor-side wiring (writer path).  Default knob = 1
    // keeps the pool empty and the legacy single-peer path
    // active; opting into knob > 1 activates registration.
    let shared_fetch_worker_pool = yggdrasil_node::runtime::new_shared_fetch_worker_pool();
    let shared_chainsync_worker_pool = yggdrasil_node::new_shared_chainsync_worker_pool();

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
            verify_vrf: active_slot_coeff.is_some(),
            active_slot_coeff: active_slot_coeff.clone(),
            slot_length_secs: genesis_slot_length,
            system_start_unix_secs: genesis_system_start_unix_secs,
            epoch_schedule: Some(file_cfg.epoch_schedule()),
            block_fetch_pool: Some(block_fetch_pool.clone()),
            max_concurrent_block_fetch_peers: file_cfg.max_concurrent_block_fetch_peers,
            density_registry: Some(density_registry.clone()),
            shared_fetch_worker_pool: Some(shared_fetch_worker_pool.clone()),
            shared_chainsync_worker_pool: Some(shared_chainsync_worker_pool.clone()),
        }
    } else {
        VerifiedSyncServiceConfig {
            batch_size,
            verification: VerificationConfig {
                slots_per_kes_period: file_cfg.slots_per_kes_period,
                max_kes_evolutions: file_cfg.max_kes_evolutions,
                verify_body_hash: false,
                max_major_protocol_version: Some(file_cfg.max_major_protocol_version),
                future_check,
                ocert_counters: Some(initial_ocert_counters.clone()),
                pp_major_protocol_version: None,
                network_magic: Some(file_cfg.network_magic),
            },
            nonce_config: Some(nonce_config),
            security_param: Some(security_param),
            checkpoint_policy: LedgerCheckpointPolicy {
                min_slot_delta: checkpoint_interval_slots,
                max_snapshots: max_ledger_snapshots,
            },
            plutus_cost_model: plutus_cost_model.clone(),
            verify_vrf: false,
            active_slot_coeff,
            slot_length_secs: genesis_slot_length,
            system_start_unix_secs: genesis_system_start_unix_secs,
            epoch_schedule: Some(file_cfg.epoch_schedule()),
            block_fetch_pool: Some(block_fetch_pool.clone()),
            max_concurrent_block_fetch_peers: file_cfg.max_concurrent_block_fetch_peers,
            density_registry: Some(density_registry.clone()),
            shared_fetch_worker_pool: Some(shared_fetch_worker_pool.clone()),
            shared_chainsync_worker_pool: Some(shared_chainsync_worker_pool.clone()),
        }
    };

    let tracer = NodeTracer::from_config(&file_cfg);
    let base_ledger_state = strict_base_ledger_state(&file_cfg, config_base_dir.as_deref())?;

    // Positive audit-trail trace for the genesis-hash integrity
    // check. `strict_base_ledger_state` bails on mismatch before
    // returning `Ok`, so reaching this point means every declared
    // `*GenesisHash` matched the file on disk. Surfacing this in
    // the log gives operators confirmation that the integrity
    // check actually ran, alongside the count of verified pairs.
    trace_genesis_hashes_verified(&tracer, &file_cfg);
    let chain_db = ChainDb::new(
        FileImmutable::open(storage_dir.join("immutable"))?,
        FileVolatile::open(storage_dir.join("volatile"))?,
        FileLedgerStore::open(storage_dir.join("ledger"))?,
    );

    let peer_addr = peer.unwrap_or(file_cfg.peer_addr);
    let recovery = recover_ledger_state_chaindb_epoch_boundary(
        &chain_db,
        base_ledger_state.clone(),
        file_cfg.epoch_schedule(),
        None,
    );
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
        peer_sharing: file_cfg.peer_sharing,
    };

    let governor_config = RuntimeGovernorConfig::new(
        std::time::Duration::from_secs(file_cfg.governor_tick_interval_secs),
        file_cfg
            .keepalive_interval_secs
            .map(std::time::Duration::from_secs),
        NodePeerSharing::from_wire(file_cfg.peer_sharing),
        file_cfg.consensus_mode.to_network_mode(),
        GovernorTargets {
            target_known: file_cfg.governor_target_known,
            target_established: file_cfg.governor_target_established,
            target_active: file_cfg.governor_target_active,
            target_known_big_ledger: file_cfg.governor_target_known_big_ledger,
            target_established_big_ledger: file_cfg.governor_target_established_big_ledger,
            target_active_big_ledger: file_cfg.governor_target_active_big_ledger,
            ..Default::default()
        },
    )
    .with_block_fetch_pool(Some(block_fetch_pool.clone()))
    .with_density_registry(Some(density_registry.clone()))
    .with_max_concurrent_block_fetch_peers(file_cfg.max_concurrent_block_fetch_peers)
    .with_shared_fetch_worker_pool(Some(shared_fetch_worker_pool.clone()))
    .with_shared_chainsync_worker_pool(Some(shared_chainsync_worker_pool.clone()))
    .with_epoch_schedule(Some(file_cfg.epoch_schedule()))
    // Wire genesis-derived timing into the live LedgerStateJudgement
    // so the governor's per-tick `fetch_mode_from_judgement(...)`
    // signal actually reflects whether the recovered tip is fresh
    // or stale, instead of always claiming `YoungEnough`. Mirrors
    // upstream `mkLedgerStateJudgement` from
    // `Cardano.Node.Diffusion.Configuration` whose threshold is
    // `stabilityWindow * slotLength` ≈ `3 * k / f * slotLength`.
    .with_ledger_judgement_settings(yggdrasil_node::runtime::LedgerJudgementSettings {
        system_start_unix_secs: genesis_system_start_unix_secs,
        slot_length_secs: genesis_slot_length,
        max_ledger_state_age_secs: (3.0 * file_cfg.security_param_k as f64
            / file_cfg.active_slot_coeff)
            * genesis_slot_length.unwrap_or(1.0),
    });

    let mut topology_config = file_cfg.topology_config();
    if let Some(peer_snapshot_path) = &peer_snapshot_path {
        topology_config.peer_snapshot_file = Some(peer_snapshot_path.display().to_string());
    }

    let node_role = node_role_report(&file_cfg, non_producing_node)?;
    tracer.trace_runtime(
        "Startup.NodeRole",
        "Notice",
        "resolved node role",
        trace_fields([
            ("role", json!(node_role.role)),
            ("nonProducingNode", json!(node_role.non_producing_node)),
            (
                "inboundListenAddr",
                json!(node_role.inbound_listen_addr.as_deref()),
            ),
            (
                "blockProducerCredentials",
                json!(node_role.block_producer_credentials),
            ),
        ]),
    );

    let block_producer_credentials = load_configured_block_producer_credentials(
        &file_cfg,
        config_base_dir.as_deref(),
        non_producing_node,
    )?;

    // R214 — pre-encode `ShelleyGenesis` once at startup so
    // the `GetGenesisConfig` (era-specific tag 11) LSQ
    // response can return real genesis bytes instead of the
    // legacy `null_response()` placeholder.  See
    // [`yggdrasil_node::encode_shelley_genesis_for_lsq`] for
    // the upstream-aligned 15-element list shape per
    // `Cardano.Ledger.Shelley.Genesis.encCBOR`.
    let genesis_config_cbor: Option<std::sync::Arc<Vec<u8>>> = shelley_genesis.as_ref().map(|g| {
        let chain_start_unix_secs = g
            .system_start
            .as_deref()
            .and_then(genesis::chrono_parse_system_start)
            .unwrap_or(0.0);
        let pp = yggdrasil_ledger::ProtocolParameters {
            min_fee_a: g.protocol_params.min_fee_a,
            min_fee_b: g.protocol_params.min_fee_b,
            max_block_body_size: g.protocol_params.max_block_body_size,
            max_tx_size: g.protocol_params.max_tx_size,
            max_block_header_size: g.protocol_params.max_block_header_size,
            key_deposit: g.protocol_params.key_deposit,
            pool_deposit: g.protocol_params.pool_deposit,
            e_max: g.protocol_params.e_max,
            n_opt: g.protocol_params.n_opt,
            a0: yggdrasil_ledger::types::UnitInterval {
                numerator: g.protocol_params.a0.numerator,
                denominator: g.protocol_params.a0.denominator,
            },
            rho: yggdrasil_ledger::types::UnitInterval {
                numerator: g.protocol_params.rho.numerator,
                denominator: g.protocol_params.rho.denominator,
            },
            tau: yggdrasil_ledger::types::UnitInterval {
                numerator: g.protocol_params.tau.numerator,
                denominator: g.protocol_params.tau.denominator,
            },
            d: g.protocol_params.decentralisation_param.map(|f| {
                let denom = 1_000_000u64;
                yggdrasil_ledger::types::UnitInterval {
                    numerator: (f * denom as f64).round() as u64,
                    denominator: denom,
                }
            }),
            extra_entropy: None,
            protocol_version: Some((
                g.protocol_params.protocol_version.major,
                g.protocol_params.protocol_version.minor,
            )),
            min_utxo_value: Some(g.protocol_params.min_utxo_value),
            min_pool_cost: g.protocol_params.min_pool_cost,
            ..Default::default()
        };
        let bytes = yggdrasil_node::encode_shelley_genesis_for_lsq(g, &pp, chain_start_unix_secs);
        std::sync::Arc::new(bytes)
    });
    let initial_praos_nonce = file_cfg
        .shelley_genesis_hash
        .as_deref()
        .map(genesis::shelley_genesis_hash_to_praos_nonce)
        .transpose()
        .wrap_err("invalid ShelleyGenesisHash for Praos initial nonce")?
        .unwrap_or(Nonce::Neutral);

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
        socket_path: file_cfg.socket_path.map(PathBuf::from),
        block_producer_credentials,
        max_major_protocol_version: file_cfg.max_major_protocol_version,
        genesis_config_cbor,
        initial_praos_nonce,
    }))
}
