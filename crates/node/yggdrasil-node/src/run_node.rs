//! Node runtime entry point and its driver request struct.
//!
//! Mirrors upstream `Cardano.Node.Run.run` — orchestrates storage
//! recovery, tracer/metrics startup, network setup (mux server,
//! governor, peer registry), the verified-sync runtime, the optional
//! block producer, and the NtC server (Unix only). Returns when the
//! shutdown signal arrives.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Run.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime entry
//! point that bridges the CLI `run` subcommand to the verified-
//! sync service + governor + block-producer threads. Mirrors
//! upstream `Cardano.Node.Run::runNode`. Haskell wires the
//! entry-point inline; Yggdrasil isolates the entry shell from
//! the runtime body (in `runtime/*.rs`) so CLI dispatch stays
//! thin.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use eyre::Result;
use serde_json::json;

use yggdrasil_consensus::mempool::{SharedMempool, SharedTxState};
use yggdrasil_consensus::{DiffusionPipeliningSupport, NonceEvolutionState, TentativeState};
use yggdrasil_ledger::{LedgerState, Nonce};
use yggdrasil_network::{
    ConnectionManagerState, GovernorState, InboundGovernorState, NodePeerSharing, PeerListener,
};
use yggdrasil_node::{
    BlockProvider, ChainProvider, NodeConfig, ResumeReconnectingVerifiedSyncRequest,
    ResumedSyncServiceOutcome, RuntimeGovernorConfig, SharedChainDb, SharedPeerSharingProvider,
    SharedTxSubmissionConsumer, VerifiedSyncServiceConfig,
    resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer, run_governor_loop,
    run_inbound_accept_loop, seed_peer_registry,
};
#[cfg(feature = "forge")]
use yggdrasil_node_runtime::run_block_producer_loop;
use yggdrasil_node_tracer::{NodeMetrics, NodeTracer, trace_fields};
use yggdrasil_storage::{ChainDb, FileImmutable, FileLedgerStore, FileVolatile};

#[cfg(feature = "forge")]
use crate::forged_header_protocol_version;
use crate::{serve_metrics, wait_for_shutdown_signal};

pub(crate) struct RunNodeRequest {
    pub(crate) node_config: NodeConfig,
    pub(crate) bootstrap_peers: Vec<SocketAddr>,
    pub(crate) sync_config: VerifiedSyncServiceConfig,
    pub(crate) governor_config: RuntimeGovernorConfig,
    pub(crate) topology_config: yggdrasil_network::TopologyConfig,
    pub(crate) tracer: NodeTracer,
    pub(crate) storage_dir: PathBuf,
    pub(crate) chain_db: ChainDb<FileImmutable, FileVolatile, FileLedgerStore>,
    pub(crate) inbound_listen_addr: Option<SocketAddr>,
    pub(crate) use_ledger_peers: Option<yggdrasil_network::UseLedgerPeers>,
    pub(crate) peer_snapshot_path: Option<PathBuf>,
    pub(crate) metrics_port: Option<u16>,
    /// Genesis-seeded base ledger state used for recovery and fresh sync.
    pub(crate) base_ledger_state: LedgerState,
    /// NtC Unix domain socket path for local client queries.
    pub(crate) socket_path: Option<PathBuf>,
    /// Block producer credentials (VRF key, KES key, operational certificate).
    /// When present the node operates in block-producing mode. Only present
    /// when the binary is built with the `forge` feature; relay-only builds
    /// drop the field along with the rest of the producer dispatch.
    #[cfg(feature = "forge")]
    pub(crate) block_producer_credentials:
        Option<yggdrasil_node_block_producer::BlockProducerCredentials>,
    /// Maximum protocol-version major this node supports for forged headers.
    /// Only consulted by the forge dispatch — gated alongside `block_producer_
    /// credentials` so a relay-only build doesn't carry the unused parameter.
    #[cfg(feature = "forge")]
    pub(crate) max_major_protocol_version: u64,
    /// R214 — pre-encoded `ShelleyGenesis` CBOR bytes for the
    /// `GetGenesisConfig` (era-specific tag 11) LSQ response.  See
    /// [`yggdrasil_node::encode_shelley_genesis_for_lsq`].  When
    /// `None` the dispatcher falls back to `null_response()` (legacy
    /// pre-R214 behaviour).
    pub(crate) genesis_config_cbor: Option<std::sync::Arc<Vec<u8>>>,
    /// Initial TPraos epoch nonce derived from `ShelleyGenesisHash`.
    pub(crate) initial_praos_nonce: Nonce,
}
pub(crate) async fn run_node(request: RunNodeRequest) -> Result<()> {
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
        socket_path,
        #[cfg(feature = "forge")]
        block_producer_credentials,
        #[cfg(feature = "forge")]
        max_major_protocol_version,
        genesis_config_cbor,
        initial_praos_nonce,
    } = request;

    // Log block producer mode availability (forge-only).
    #[cfg(feature = "forge")]
    if let Some(ref bp) = block_producer_credentials {
        tracer.trace_runtime(
            "Startup.BlockProducer",
            "Notice",
            "block producer credentials loaded",
            trace_fields([
                (
                    "vrfVerificationKeyHash",
                    json!(hex::encode(
                        yggdrasil_crypto::blake2b::hash_bytes_256(&bp.vrf_verification_key.0).0
                    )),
                ),
                (
                    "opcertSequenceNumber",
                    json!(bp.operational_cert.sequence_number),
                ),
                ("kesPeriod", json!(bp.kes_current_period)),
                ("kesCurrentPeriod", json!(bp.kes_current_period)),
                ("opcertKesPeriod", json!(bp.operational_cert.kes_period)),
            ]),
        );
    }

    #[cfg(feature = "forge")]
    let block_producer_runtime_config = if block_producer_credentials.is_some() {
        let active_slot_coeff = sync_config
            .active_slot_coeff
            .clone()
            .ok_or_else(|| eyre::eyre!("block producer requires a valid active_slot_coeff"))?;
        let system_start_unix_secs = sync_config.system_start_unix_secs.ok_or_else(|| {
            eyre::eyre!(
                "block producer requires ShelleyGenesis.systemStart for absolute slot-clock parity"
            )
        })?;
        let slot_length_secs = sync_config.slot_length_secs.unwrap_or(1.0);
        let max_ledger_state_age_secs = sync_config
            .nonce_config
            .as_ref()
            .map(|nonce_config| nonce_config.stability_window as f64 * slot_length_secs);
        let protocol_version =
            forged_header_protocol_version(&base_ledger_state, max_major_protocol_version);
        let (max_block_body_size, protocol_version) = base_ledger_state
            .protocol_params()
            .map(|params| {
                (
                    params.max_block_body_size,
                    params.protocol_version.unwrap_or(protocol_version),
                )
            })
            .unwrap_or((65_536, protocol_version));

        Some(yggdrasil_node::RuntimeBlockProducerConfig {
            slot_length: std::time::Duration::from_secs_f64(slot_length_secs),
            system_start_unix_secs: Some(system_start_unix_secs),
            max_ledger_state_age_secs,
            active_slot_coeff,
            sigma_num: 1,
            sigma_den: 1,
            epoch_nonce: Nonce::Neutral,
            max_block_body_size,
            protocol_version,
        })
    } else {
        None
    };

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
            (
                "protocolVersions",
                json!(
                    node_config
                        .protocol_versions
                        .iter()
                        .map(|v| v.0)
                        .collect::<Vec<_>>()
                ),
            ),
        ]),
    );

    // Honestly disclose the trace-forwarder parity gap when the operator
    // has enabled the `Forwarder` backend.  See
    // [`trace_forwarder`] module docs for the full upstream protocol the
    // current stub does not implement.
    if let Some(socket_path) = tracer.forwarder_socket_path_if_configured() {
        tracer.trace_runtime(
            "Startup.TraceForwarderStub",
            "Warning",
            "Forwarder trace backend uses a stub transport that is not \
             interoperable with upstream cardano-tracer; events routed \
             only to the Forwarder backend will be silently dropped \
             unless a Yggdrasil-aware listener is bound to the socket. \
             Use `Stdout HumanFormatColoured` / `Stdout HumanFormat` / \
             `StdoutMachine` for guaranteed delivery.",
            trace_fields([
                ("socketPath", json!(socket_path)),
                (
                    "upstreamReference",
                    json!("Cardano.Logging.Forwarding (cardano-node)"),
                ),
            ]),
        );
    }

    let nonce_state = sync_config
        .nonce_config
        .as_ref()
        .map(|_| NonceEvolutionState::new(initial_praos_nonce));

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn signal handler for graceful shutdown.
    let signal_tracer = tracer.clone();
    let signal_shutdown_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        let signal = wait_for_shutdown_signal().await;
        signal_tracer.trace_runtime(
            "Node.Shutdown",
            "Notice",
            "shutdown signal received",
            trace_fields([("signal", json!(signal))]),
        );
        let _ = signal_shutdown_tx.send(true);
    });

    // Shared mempool for governor TTL purge and inbound TxSubmission admission.
    //
    // Capacity matches upstream `Ouroboros.Consensus.Mempool.Capacity`'s
    // default `NoMempoolCapacityBytesOverride`:
    //
    //     mempoolCapacity = 2 * maxBlockBodySize
    //
    // For preview / preprod / mainnet that resolves to ~131 KB / ~180 KB
    // / ~180 KB respectively, matching what `cardano-cli query
    // tx-mempool info` reports against an upstream node.  Falls back to
    // `2 * 65_536` when protocol params are unavailable (test fakes,
    // very early bootstrap before genesis params are loaded).
    let mempool_max_bytes = base_ledger_state
        .protocol_params()
        .map(|params| params.max_block_body_size as usize)
        .unwrap_or(65_536)
        .saturating_mul(2);
    let shared_mempool = SharedMempool::with_capacity(mempool_max_bytes);
    let shared_connection_manager = Arc::new(RwLock::new(ConnectionManagerState::new()));
    let shared_inbound_governor = Arc::new(RwLock::new(InboundGovernorState::new()));
    let shared_inbound_peers: Arc<RwLock<BTreeMap<SocketAddr, NodePeerSharing>>> =
        Arc::new(RwLock::new(BTreeMap::new()));

    let governor_task = {
        let mut governor_shutdown = shutdown_rx.clone();
        let governor_node_config = node_config.clone();
        let governor_chain_db = Arc::clone(&chain_db);
        let governor_registry = Arc::clone(&peer_registry);
        let governor_tracer = tracer.clone();
        let governor_metrics = std::sync::Arc::clone(&metrics);
        let governor_topology = topology_config.clone();
        let governor_base_ledger_state = base_ledger_state.clone();
        let governor_mempool = shared_mempool.clone();
        let governor_connection_manager = Arc::clone(&shared_connection_manager);
        let governor_inbound_peers = Arc::clone(&shared_inbound_peers);
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
                governor_connection_manager,
                GovernorState::default(),
                governor_config,
                governor_topology,
                governor_base_ledger_state,
                Some(governor_mempool),
                Some(governor_inbound_peers),
                governor_tracer,
                Some(governor_metrics),
                shutdown,
            )
            .await;
        })
    };

    // Shared chain-tip notification channel.  The block producer notifies
    // waiters when it inserts a new block so inbound ChainSync servers can
    // push updates without busy-looping.  The sync service also notifies
    // after each batch so locally-connected NtN clients see progress.
    let chain_tip_notify: yggdrasil_node::ChainTipNotify =
        std::sync::Arc::new(tokio::sync::Notify::new());

    // Whether diffusion pipelining is enabled for this node.  For now
    // it is always on; a future config flag may control this.
    let diffusion_pipelining = DiffusionPipeliningSupport::DiffusionPipeliningOn;

    // Shared diffusion pipelining state.  When pipelining is enabled, the
    // sync pipeline sets a tentative header after header validation but
    // before body validation completes; the ChainSync server may serve it
    // to downstream peers immediately.
    let shared_tentative_state: Option<Arc<RwLock<TentativeState>>> = match diffusion_pipelining {
        DiffusionPipeliningSupport::DiffusionPipeliningOff => None,
        DiffusionPipeliningSupport::DiffusionPipeliningOn => {
            Some(Arc::new(RwLock::new(TentativeState::initial())))
        }
    };

    // Shared block-producer state updated by the sync pipeline so the
    // producer loop reads live epoch nonce and stake sigma values.
    // Forge-only — relay builds drop the state along with the producer
    // task that would consume it.
    #[cfg(feature = "forge")]
    let shared_bp_state = std::sync::Arc::new(std::sync::RwLock::new(
        yggdrasil_node::SharedBlockProducerState::default(),
    ));

    // Compute issuer pool-key-hash (Blake2b-224) before credentials are
    // consumed by the block-producer task.  Used by the sync pipeline to
    // push stake sigma updates to the shared producer state.
    #[cfg(feature = "forge")]
    let bp_pool_key_hash: Option<[u8; 28]> = block_producer_credentials
        .as_ref()
        .map(|bp| yggdrasil_crypto::blake2b::hash_bytes_224(&bp.issuer_vkey.0).0);
    #[cfg(not(feature = "forge"))]
    let bp_pool_key_hash: Option<[u8; 28]> = None;

    #[cfg(feature = "forge")]
    let block_producer_task: Option<tokio::task::JoinHandle<()>> =
        if let (Some(block_producer_credentials), Some(block_producer_config)) =
            (block_producer_credentials, block_producer_runtime_config)
        {
            let mut producer_shutdown = shutdown_rx.clone();
            let producer_chain_db = Arc::clone(&chain_db);
            let producer_mempool = shared_mempool.clone();
            let producer_tracer = tracer.clone();
            let producer_metrics = std::sync::Arc::clone(&metrics);
            let producer_tip_notify = chain_tip_notify.clone();
            let producer_bp_state = std::sync::Arc::clone(&shared_bp_state);
            Some(tokio::spawn(async move {
                let shutdown = async move {
                    if *producer_shutdown.borrow() {
                        return;
                    }
                    while producer_shutdown.changed().await.is_ok() {
                        if *producer_shutdown.borrow() {
                            break;
                        }
                    }
                };

                run_block_producer_loop(
                    producer_chain_db,
                    producer_mempool,
                    block_producer_credentials,
                    block_producer_config,
                    Some(producer_tip_notify),
                    Some(producer_bp_state),
                    producer_tracer,
                    Some(producer_metrics),
                    shutdown,
                )
                .await;
            }))
        } else {
            None
        };
    // Relay-only build: no producer task; the JoinHandle slot is permanently None.
    #[cfg(not(feature = "forge"))]
    let block_producer_task: Option<tokio::task::JoinHandle<()>> = None;

    // Shared TxSubmission inbound dedup state; threaded into both the inbound
    // accept loop (populated when peers advertise TxIds) and the reconnecting
    // sync request (consulted during mempool eviction to avoid re-fetching
    // transactions already confirmed on the applied chain).  Cloning is cheap
    // (Arc<RwLock<_>>).
    let inbound_tx_state = SharedTxState::default();

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

        let shared_provider = if let Some(tentative) = shared_tentative_state.as_ref() {
            Arc::new(SharedChainDb::from_arc_with_tentative(
                Arc::clone(&chain_db),
                Arc::clone(tentative),
            ))
        } else {
            Arc::new(SharedChainDb::from_arc(Arc::clone(&chain_db)))
        };
        let block_provider: Arc<dyn BlockProvider> = shared_provider.clone();
        let chain_provider: Arc<dyn ChainProvider> = shared_provider;
        let tx_submission_consumer = Arc::new(
            SharedTxSubmissionConsumer::new(Arc::clone(&chain_db), shared_mempool.clone())
                .with_metrics(Arc::clone(&metrics)),
        );
        let peer_sharing = Arc::new(SharedPeerSharingProvider::with_inbound_governor(
            Arc::clone(&peer_registry),
            Some(Arc::clone(&shared_inbound_governor)),
        ));
        let inbound_connection_manager = Arc::clone(&shared_connection_manager);
        let inbound_governor = Arc::clone(&shared_inbound_governor);
        let inbound_tx_state = inbound_tx_state.clone();
        let mut inbound_shutdown = shutdown_rx.clone();
        let inbound_tracer = tracer.clone();
        let inbound_metrics = metrics.clone();
        let inbound_peers = Arc::clone(&shared_inbound_peers);
        let inbound_tip_notify = chain_tip_notify.clone();

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
                Some(inbound_peers),
                Some(inbound_connection_manager),
                Some(inbound_governor),
                Some(yggdrasil_network::AcceptedConnectionsLimit::default()),
                Some(inbound_tx_state),
                Some(inbound_tip_notify),
                Some(&inbound_tracer),
                Some(&inbound_metrics),
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

    // -- NtC local server (Unix socket for CLI queries / tx submission) ----
    #[cfg(unix)]
    let ntc_task = if let Some(ref ntc_path) = socket_path {
        let ntc_chain_db = Arc::clone(&chain_db);
        let ntc_mempool = shared_mempool.clone();
        let ntc_path = ntc_path.clone();
        let ntc_storage_dir = Some(storage_dir.clone());
        let ntc_tracer = tracer.clone();
        let mut ntc_shutdown = shutdown_rx.clone();
        let ntc_evaluator: Option<
            Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>,
        > = None;
        let ntc_metrics = Some(Arc::clone(&metrics));
        let ntc_network_magic = node_config.network_magic;

        // R214 — pre-encoded `ShelleyGenesis` CBOR bytes pulled from
        // the run request (computed at the CLI call site where
        // `file_cfg` + `config_base_dir` are in scope).
        let ntc_genesis_cbor = genesis_config_cbor.clone();

        tracer.trace_runtime(
            "Net.NtC",
            "Notice",
            "starting NtC local server",
            trace_fields([
                ("socketPath", json!(ntc_path.display().to_string())),
                (
                    "genesisConfigCborBytes",
                    json!(ntc_genesis_cbor.as_ref().map(|b| b.len()).unwrap_or(0)),
                ),
            ]),
        );

        Some(tokio::spawn(async move {
            let dispatcher: Arc<dyn yggdrasil_node::LocalQueryDispatcher> = {
                let mut d = yggdrasil_node::BasicLocalQueryDispatcher::new(
                    yggdrasil_node::NetworkPreset::from_network_magic(ntc_network_magic),
                );
                if let Some(bytes) = ntc_genesis_cbor.as_ref() {
                    d = d.with_genesis_config_cbor(std::sync::Arc::clone(bytes));
                }
                Arc::new(d)
            };
            let shutdown = async move {
                if *ntc_shutdown.borrow() {
                    return;
                }
                while ntc_shutdown.changed().await.is_ok() {
                    if *ntc_shutdown.borrow() {
                        break;
                    }
                }
            };
            if let Err(err) = yggdrasil_node::run_local_accept_loop(
                &ntc_path,
                ntc_network_magic,
                ntc_chain_db,
                ntc_mempool,
                dispatcher,
                ntc_evaluator,
                ntc_metrics,
                ntc_storage_dir,
                shutdown,
            )
            .await
            {
                ntc_tracer.trace_runtime(
                    "Net.NtC",
                    "Error",
                    "NtC local server stopped with error",
                    trace_fields([("error", json!(err.to_string()))]),
                );
            }
        }))
    } else {
        None
    };
    #[cfg(not(unix))]
    let ntc_task: Option<tokio::task::JoinHandle<()>> = {
        let _ = &socket_path;
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
    .with_metrics(Some(&metrics))
    .with_peer_registry(Some(Arc::clone(&peer_registry)))
    .with_mempool(Some(shared_mempool.clone()))
    .with_tentative_state(shared_tentative_state.clone())
    .with_tip_notify(Some(chain_tip_notify.clone()))
    .with_bp_state(
        {
            #[cfg(feature = "forge")]
            {
                bp_pool_key_hash.map(|_| std::sync::Arc::clone(&shared_bp_state))
            }
            #[cfg(not(feature = "forge"))]
            {
                // Relay-only build has no producer task to push state
                // updates to; sync drops sigma/nonce updates on the floor.
                None
            }
        },
        bp_pool_key_hash,
    )
    .with_inbound_tx_state(Some(inbound_tx_state))
    .with_chain_dep_persist_dir(Some(storage_dir.clone()));

    let mut sync_shutdown = shutdown_rx.clone();
    let outcome: ResumedSyncServiceOutcome =
        match resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer(
            &chain_db,
            request,
            &tracer,
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
        .await
        {
            Ok(outcome) => outcome,
            Err(err) => {
                let _ = shutdown_tx.send(true);
                let _ = governor_task.await;
                if let Some(handle) = block_producer_task {
                    let _ = handle.await;
                }
                if let Some(handle) = inbound_task {
                    let _ = handle.await;
                }
                if let Some(handle) = ntc_task {
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
    if let Some(handle) = block_producer_task {
        let _ = handle.await;
    }
    if let Some(handle) = inbound_task {
        let _ = handle.await;
    }
    if let Some(handle) = ntc_task {
        let _ = handle.await;
    }

    tracer.trace_runtime(
        "Node.Sync",
        "Notice",
        "sync complete",
        trace_fields([
            (
                "checkpointSlot",
                json!(outcome.recovery.checkpoint_slot.map(|slot| slot.0)),
            ),
            (
                "replayedVolatileBlocks",
                json!(outcome.recovery.replayed_volatile_blocks),
            ),
            (
                "recoveredPoint",
                json!(format!("{:?}", outcome.recovery.point)),
            ),
            ("totalBlocks", json!(outcome.sync.total_blocks)),
            ("totalRollbacks", json!(outcome.sync.total_rollbacks)),
            ("batchesCompleted", json!(outcome.sync.batches_completed)),
            ("stableBlockCount", json!(outcome.sync.stable_block_count)),
            ("reconnectCount", json!(outcome.sync.reconnect_count)),
            (
                "lastConnectedPeer",
                json!(
                    outcome
                        .sync
                        .last_connected_peer_addr
                        .map(|addr| addr.to_string())
                ),
            ),
            (
                "finalPoint",
                json!(format!("{:?}", outcome.sync.final_point)),
            ),
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
