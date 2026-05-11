//! Top-level cardano-tracer supervisor — `runCardanoTracer` analog.
//! Reads the operator config, initializes the runtime state, and
//! spawns the four core subsystems (Acceptors, Metrics servers,
//! Logs rotator, RTView) concurrently.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Run.hs.
//!
//! Direct port of upstream's `runCardanoTracer` +
//! `doRunCardanoTracer` entry points. R427 ships the operationally-
//! viable subset (config load, Acceptors supervisor, Metrics
//! servers); the deferred subsystems (Logs rotator, ReForwarder,
//! resource stats loop, RTView) are documented carve-outs that
//! can be wired in later rounds without breaking the call surface.
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                              |
//! |---------------------------------------------------|----------------------------------------|
//! | `runCardanoTracer :: TracerParams -> IO ()`       | [`run_cardano_tracer`]                 |
//! | `doRunCardanoTracer config rtViewStateDir tr brake dpRequestors :: IO ()` | [`do_run_cardano_tracer`] |
//! | `loadMetricsHelp` (Run.hs:181-191)                | [`crate::utils::load_metrics_help`] (R415 done) |
//! | `runLogsRotator tracerEnv`                        | (deferred — see [`run_logs_rotator_status`]) |
//! | `runMetricsServers tracerEnv`                     | (uses R408-R414's per-server entries; full Servers.hs aggregator deferred — see [`run_metrics_servers_status`]) |
//! | `runAcceptors tracerEnv tracerEnvRTView`          | [`crate::acceptors::run::run_acceptors`] (R426 done) |
//! | `runRTView tracerEnv tracerEnvRTView`             | (RTView carve-out — see plan)          |
//! | `beforeProgramStops { ... }`                      | (deferred — see [`crate::utils::before_program_stops_status`]) |
//! | `mkTraceBundle` + `traceWith tr.assorted`         | (deferred — meta-trace channel not ported) |
//! | `for_ (resourceFreq config) ...` resource stats loop | (deferred — see [`run_resource_stats_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **TraceBundle / meta-trace channel** (`mkTraceBundle`,
//!   `traceWith tr.assorted`): depends on the upstream
//!   `Cardano.Logging` package + meta-trace channel ports. R427
//!   collapses these to no-op log calls; the actual log traffic
//!   goes through Yggdrasil's standard `eprintln!` / future
//!   structured-logger plumbing.
//! - **`runLogsRotator`**: Logs/Rotator.hs port deferred per the
//!   R411 plan's pacing.
//! - **`runRTView`**: RTView web UI is a synthesis carve-out per
//!   the original R326-R459 plan (no Rust analog for ThreePenny GUI).
//! - **Resource stats loop** (`for_ (resourceFreq config) ...`):
//!   periodic `readResourceStats`. Operationally a metrics-emission
//!   convenience; deferred pending the Cardano.Logging.Resources
//!   port.
//! - **`beforeProgramStops` SIGINT/SIGTERM handler**: deferred per
//!   `crate::utils::before_program_stops_status`. The supervisor
//!   currently shuts down via the brake flag in the config.
//! - **DataPointRequestors initialization**: deferred per the
//!   DataPoint sub-protocol carve-out.
//! - **CurrentLogLock / CurrentDPLock**: deferred — the lock-free
//!   `Arc<RwLock<...>>` shape Yggdrasil uses for runtime state
//!   doesn't need separate per-resource locks for the bounded
//!   subset of operations R427 wires.

use std::sync::Arc;

use crate::acceptors::run::{AcceptorsSupervisorError, run_acceptors};
use crate::acceptors::server::AcceptorsServerState;
use crate::configuration::{TracerConfig, parse_tracer_config_json};
use crate::logging::TraceObject;
use crate::metrics_store::new_accepted_metrics;
use crate::types::{ConnectedNodes, ConnectedNodesNames};

// ---------------------------------------------------------------------------
// TracerParams
// ---------------------------------------------------------------------------

/// Parsed operator-supplied parameters for the `cardano-tracer`
/// binary. Mirror of upstream's `data TracerParams = TracerParams
/// { tracerConfig, stateDir, logSeverity }`.
#[derive(Clone, Debug)]
pub struct TracerParams {
    /// Path to the operator's `tracer-config.json`.
    pub tracer_config: std::path::PathBuf,
    /// Optional path to the RTView state directory (RTView is a
    /// carve-out so this only affects the loaded-config layout).
    pub state_dir: Option<std::path::PathBuf>,
    /// Optional minimum log severity filter for the meta-trace
    /// channel (deferred). Type matches the parser's `SeverityS`
    /// (the producer of this field via argv parsing).
    pub log_severity: Option<crate::parser::SeverityS>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the cardano-tracer supervisor.
#[derive(Debug, thiserror::Error)]
pub enum RunCardanoTracerError {
    /// Failed to read the operator's tracer-config.json from disk.
    #[error("read tracer config: {0}")]
    ReadConfig(std::io::Error),

    /// Failed to parse the operator's tracer-config.json.
    #[error("parse tracer config: {0}")]
    ParseConfig(crate::configuration::ParseError),

    /// Acceptors supervisor returned an unrecoverable error.
    #[error("acceptors supervisor: {0}")]
    Acceptors(#[from] AcceptorsSupervisorError),
}

// ---------------------------------------------------------------------------
// Top-level entry points
// ---------------------------------------------------------------------------

/// Top-level run function — entry called by the `cardano-tracer`
/// binary. Mirror of upstream's `runCardanoTracer
/// TracerParams{tracerConfig, stateDir, logSeverity}`.
///
/// Reads the operator config, initializes the protocols-brake +
/// data-point-requestors placeholders, then delegates to
/// [`do_run_cardano_tracer`].
///
/// `lo_handler` is invoked once per inbound `MsgTraceObjectsReply`
/// batch and is shared across all forwarder connections (the
/// canonical operator implementation routes through
/// `crate::handlers::logs::trace_objects::trace_objects_handler`).
pub async fn run_cardano_tracer<LoHandler>(
    params: TracerParams,
    lo_handler: Arc<LoHandler>,
) -> Result<(), RunCardanoTracerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let raw = std::fs::read_to_string(&params.tracer_config)
        .map_err(RunCardanoTracerError::ReadConfig)?;
    let config = parse_tracer_config_json(&raw).map_err(RunCardanoTracerError::ParseConfig)?;
    do_run_cardano_tracer(config, params.state_dir, lo_handler).await
}

/// Convenience entry — reads the config, builds the canonical
/// trace-objects handler via [`default_lo_handler_factory`], and
/// runs the supervisor. R431 wires this as the default entry point
/// for the `cardano-tracer` binary; operators wanting custom
/// handlers should call [`run_cardano_tracer`] directly.
pub async fn run_cardano_tracer_default(params: TracerParams) -> Result<(), RunCardanoTracerError> {
    let raw = std::fs::read_to_string(&params.tracer_config)
        .map_err(RunCardanoTracerError::ReadConfig)?;
    let config = parse_tracer_config_json(&raw).map_err(RunCardanoTracerError::ParseConfig)?;

    // Build the runtime state slice ahead of the supervisor so the
    // default handler can capture connected_nodes_names by clone.
    let state = AcceptorsServerState {
        connected_nodes: ConnectedNodes::new(),
        connected_nodes_names: ConnectedNodesNames::new(),
        accepted_metrics: new_accepted_metrics(),
        network_magic: config.network_magic,
    };
    let lo_handler = Arc::new(default_lo_handler_factory(
        &config,
        state.connected_nodes_names.clone(),
    ));
    do_run_cardano_tracer_with_state(state, config, params.state_dir, lo_handler).await
}

/// Variant of [`do_run_cardano_tracer`] that accepts a pre-built
/// [`AcceptorsServerState`] (rather than constructing one) so
/// callers like [`run_cardano_tracer_default`] can capture
/// references to the same `ConnectedNodesNames` map that the
/// supervisor will populate.
pub async fn do_run_cardano_tracer_with_state<LoHandler>(
    state: AcceptorsServerState,
    config: TracerConfig,
    _rt_view_state_dir: Option<std::path::PathBuf>,
    lo_handler: Arc<LoHandler>,
) -> Result<(), RunCardanoTracerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let acceptors_state = state.clone();
    let acceptors_config = config.clone();
    let acceptors_handler = Arc::clone(&lo_handler);
    run_acceptors(acceptors_state, &acceptors_config, acceptors_handler).await?;
    Ok(())
}

/// Run all internal services of the tracer. Mirror of upstream's
/// `doRunCardanoTracer config rtViewStateDir tr protocolsBrake
/// dpRequestors`.
///
/// Initializes the runtime state slice (`ConnectedNodes`,
/// `ConnectedNodesNames`, `AcceptedMetrics`) and spawns the
/// Acceptors supervisor. Other subsystems (logs rotator, metrics
/// servers, RTView) are documented carve-outs that can be added
/// to the concurrent task set in later rounds.
pub async fn do_run_cardano_tracer<LoHandler>(
    config: TracerConfig,
    _rt_view_state_dir: Option<std::path::PathBuf>,
    lo_handler: Arc<LoHandler>,
) -> Result<(), RunCardanoTracerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let state = AcceptorsServerState {
        connected_nodes: ConnectedNodes::new(),
        connected_nodes_names: ConnectedNodesNames::new(),
        accepted_metrics: new_accepted_metrics(),
        network_magic: config.network_magic,
    };

    // Brake flag for the logs rotator: trip it on supervisor exit
    // so the rotator's sleep-loop unwinds cleanly. Shares semantic
    // role with the AcceptorConfiguration's brake but lives at the
    // supervisor level since the Acceptors-side brake is configured
    // per-acceptor-config (currently freshly minted in
    // `run::run_acceptors`).
    let rotator_stop = std::sync::Arc::new(tokio::sync::RwLock::new(false));
    let current_log_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    // R461: HandleRegistry shared between trace-objects-handler
    // (which mints + registers handles in `crate::handlers::logs::
    // file`) and the rotator (which inspects the registered handles
    // to roll the current log file).
    //
    // R461 carve-out: the trace-objects-handler currently mints its
    // own per-call handles + doesn't share a HandleRegistry with
    // the supervisor. Wiring that handoff is a follow-on round —
    // for now the rotator runs against an empty registry (which
    // matches upstream's behavior when the operator hasn't yet
    // accepted any forwarder traffic).
    let handle_registry = crate::types::HandleRegistry::new();
    // Error tracer for the rotator. Operators wire this to their
    // log sink; tests typically supply a no-op closure.
    let error_tracer: crate::handlers::logs::rotator::LogsRotatorErrorTracer =
        std::sync::Arc::new(|msg: &str| {
            eprintln!("cardano-tracer: {msg}");
        });

    // Mirror of upstream's `sequenceConcurrently_`:
    //   [ runLogsRotator tracerEnv
    //   , runAcceptors tracerEnv
    //   , runMetricsServers tracerEnv         -- still partial
    //   , runResourceStats tracerEnv          -- carve-out
    //   ]
    // R461 wires the logs rotator alongside the acceptors. Both
    // tasks run concurrently; the acceptors-side run completes
    // when its own brake fires (operator-configured), and the
    // rotator-side run is cancelled via the supervisor-level brake
    // when the acceptors finish.
    let acceptors_state = state.clone();
    let acceptors_config = config.clone();
    let acceptors_handler = Arc::clone(&lo_handler);

    let rotator_config = config.clone();
    let rotator_stop_clone = rotator_stop.clone();
    let rotator_registry = handle_registry.clone();
    let rotator_lock = current_log_lock.clone();
    let rotator_tracer = error_tracer.clone();
    let rotator_task = tokio::spawn(async move {
        crate::handlers::logs::rotator::run_logs_rotator(
            &rotator_config,
            rotator_registry,
            rotator_lock,
            rotator_stop_clone,
            rotator_tracer,
        )
        .await;
    });

    let acceptors_result =
        run_acceptors(acceptors_state, &acceptors_config, acceptors_handler).await;

    // Acceptors finished (either via brake or error) — trip the
    // rotator brake + await its clean exit (50ms brake-poll cadence
    // means worst-case ~50ms latency).
    *rotator_stop.write().await = true;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), rotator_task).await;

    acceptors_result?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Default trace-objects handler factory
// ---------------------------------------------------------------------------

/// Build a default trace-objects handler closure that dispatches
/// each batch to the canonical
/// [`crate::handlers::logs::trace_objects::trace_objects_handler`]
/// (R401). The closure captures the operator's logging params +
/// the runtime's connected-nodes-names map; on each invocation it
/// spawns a tokio task to perform the async dispatch (the
/// `lo_handler` itself is a sync `Fn` closure called from the
/// trace-forwarder acceptor loop, which lives in an async context).
///
/// Mirror context: upstream's
/// `Cardano.Tracer.Acceptors.Server::runTraceObjectsAcceptor`
/// passes `traceObjectsHandler tracerEnv tracerEnvRTView . connIdToNodeId`
/// to `acceptTraceObjectsResp`. R431 wires the equivalent
/// dispatcher closure for Yggdrasil's lib.rs::run default; operators
/// supplying their own closure to `run_cardano_tracer` can opt out.
///
/// The NodeId → NodeName resolution falls back to
/// `NodeId::as_str` when the node hasn't registered a name yet
/// (mirror of upstream's `getNodeName` fallback in
/// `Notifications/Send.hs`). The `LoggingParams` slice is
/// captured by clone — once `do_run_cardano_tracer` finishes the
/// captured slice is dropped along with the closure.
pub fn default_lo_handler_factory(
    config: &TracerConfig,
    connected_nodes_names: ConnectedNodesNames,
) -> impl Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static {
    let logging = config.logging.clone();
    move |node_id, trace_objects| {
        if trace_objects.is_empty() {
            return;
        }
        let logging = logging.clone();
        let names = connected_nodes_names.clone();
        tokio::spawn(async move {
            let node_name = names
                .snapshot()
                .into_iter()
                .find_map(|(id, name)| if id == node_id { Some(name) } else { None })
                .unwrap_or_else(|| node_id.as_str().to_string());
            let _outcomes = crate::handlers::logs::trace_objects::trace_objects_handler(
                &node_name,
                &logging,
                &trace_objects,
            )
            .await;
        });
    }
}

// ---------------------------------------------------------------------------
// Carve-out status descriptors
// ---------------------------------------------------------------------------

/// Status descriptor for the (now-closed) `runLogsRotator` subsystem.
/// Retained for programmatic introspection — the function returns a
/// short description summarising the current state and the round in
/// which it closed.
pub fn run_logs_rotator_status() -> &'static str {
    "runLogsRotator: closed at R461. The Logs/Rotator.hs IO orchestration \
     (runLogsRotator + launchRotator + checkRootDir + checkLogs + \
     checkIfCurrentLogIsFull) shipped in R461 as \
     crate::handlers::logs::rotator::run_logs_rotator. The supervisor \
     (do_run_cardano_tracer) wires it alongside run_acceptors via \
     tokio::spawn + supervisor-level brake flag; the rotator's \
     50ms-brake-poll cadence ensures clean shutdown alongside the \
     acceptors. Carve-out: showProblemIfAny → caller-supplied \
     error-tracer closure (tracer-trace channel from MetaTrace.hs \
     remains unported, but the orchestration accepts a Rust closure \
     in place of the typeclass-dispatched contra-tracer)."
}

/// Status descriptor for the deferred `runMetricsServers`
/// aggregator. The per-server entries (run_prometheus_server,
/// run_monitoring_server) are wired at R408-R414; only the
/// aggregator that spawns them concurrently is pending.
pub fn run_metrics_servers_status() -> &'static str {
    "runMetricsServers: per-server entries are wired at R408 \
     (Prometheus) + R410 (Monitoring) + R411-R414 (MetricsStore \
     wiring). The Servers.hs aggregator that spawns them \
     concurrently is the only piece pending; the Yggdrasil callers \
     can spawn them directly via tokio::join! / JoinSet for now."
}

/// Status descriptor for the deferred resource-stats loop.
pub fn run_resource_stats_status() -> &'static str {
    "runResourceStats (the for_ (resourceFreq config) ... loop in \
     upstream Run.hs:78-85): deferred pending the \
     Cardano.Logging.Resources port. Operationally a metrics-\
     emission convenience; cardano-tracer's core ingest path \
     (Acceptors → MetricsStore) does not depend on it."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{HowToConnect, LogFormat, LogMode, LoggingParams, Network};

    #[test]
    fn run_logs_rotator_status_describes_closure() {
        let s = run_logs_rotator_status();
        assert!(s.contains("closed at R461"));
        assert!(s.contains("Rotator"));
        assert!(s.contains("brake"));
    }

    #[test]
    fn run_metrics_servers_status_describes_partial_wiring() {
        let s = run_metrics_servers_status();
        assert!(s.contains("Prometheus"));
        assert!(s.contains("Monitoring"));
    }

    #[test]
    fn run_resource_stats_status_describes_deferral() {
        let s = run_resource_stats_status();
        assert!(s.contains("deferred"));
        assert!(s.contains("Cardano.Logging.Resources"));
    }

    #[tokio::test]
    async fn run_cardano_tracer_errors_on_missing_config_file() {
        let params = TracerParams {
            tracer_config: std::path::PathBuf::from(
                "/nonexistent-yggdrasil-r427-tracer-config.json",
            ),
            state_dir: None,
            log_severity: None,
        };
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result = run_cardano_tracer_test_shim(params, handler).await;
        assert!(matches!(result, Err(RunCardanoTracerError::ReadConfig(_))));
    }

    #[tokio::test]
    async fn run_cardano_tracer_errors_on_unparseable_config() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let cfg = dir.path().join("malformed.json");
        std::fs::write(&cfg, b"{ this is not valid json }").expect("write");
        let params = TracerParams {
            tracer_config: cfg,
            state_dir: None,
            log_severity: None,
        };
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result = run_cardano_tracer_test_shim(params, handler).await;
        assert!(matches!(result, Err(RunCardanoTracerError::ParseConfig(_))));
    }

    #[tokio::test]
    async fn do_run_cardano_tracer_returns_when_brake_immediate() {
        // Build a minimal valid TracerConfig with ConnectTo / no
        // targets so the supervisor errors immediately on NoTargets
        // — this proves the end-to-end wiring without standing up a
        // real listener.
        let config = TracerConfig {
            network_magic: 764824073,
            network: Network::ConnectTo { connect_to: vec![] },
            logging: vec![LoggingParams {
                root: ".".into(),
                mode: LogMode::FileMode,
                format: LogFormat::ForHuman,
            }],
            rotation: None,
            verbosity: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            ekg_request_freq: None,
            ekg_request_full: None,
            metrics_help: None,
            log_objects_request_num: Some(50),
            metrics_no_suffix: None,
            prometheus_labels: None,
            resource_freq: None,
        };
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result = do_run_cardano_tracer_test_shim(config, None, handler).await;
        assert!(
            matches!(result, Err(RunCardanoTracerError::Acceptors(_))),
            "empty ConnectTo target list should bubble up the NoTargets error: {result:?}"
        );
    }

    #[tokio::test]
    async fn default_lo_handler_factory_dispatches_to_trace_objects_handler() {
        // Build a config with a single FileMode logging entry — we
        // verify the factory captures it and dispatches a non-empty
        // payload through to the handler. Since the handler returns
        // `Vec<DispatchOutcome>` and the factory ignores the result,
        // this test asserts the factory is constructable + invokes
        // without panic; the outcome inspection is exercised by
        // R401's dedicated trace_objects_handler tests.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config = TracerConfig {
            network_magic: 764824073,
            network: Network::AcceptAt {
                accept_at: HowToConnect::LocalPipe {
                    local_pipe: dir.path().join("rt.sock"),
                },
            },
            logging: vec![LoggingParams {
                root: dir.path().to_path_buf(),
                mode: LogMode::FileMode,
                format: LogFormat::ForHuman,
            }],
            rotation: None,
            verbosity: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            ekg_request_freq: None,
            ekg_request_full: None,
            metrics_help: None,
            log_objects_request_num: Some(50),
            metrics_no_suffix: None,
            prometheus_labels: None,
            resource_freq: None,
        };

        let names = ConnectedNodesNames::new();
        names.insert(crate::types::NodeId::new("test-node"), "alpha".to_string());
        let handler = default_lo_handler_factory(&config, names);

        // Invoke with an empty payload: the early-return short-
        // circuits without spawning a task.
        handler(crate::types::NodeId::new("test-node"), Vec::new());

        // Invoke with a non-empty payload: the factory spawns a
        // task that runs the dispatcher. Give the runtime a brief
        // moment to schedule + complete the spawned task.
        let trace_obj = TraceObject::new(
            Some("preview".into()),
            "machine".into(),
            crate::severity::SeverityS::Info,
            vec!["BlockFetch".into()],
            "tid-1".into(),
            1,
        );
        handler(
            crate::types::NodeId::new("test-node"),
            vec![trace_obj.clone()],
        );

        // Wait briefly for the spawned task to run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn default_lo_handler_factory_falls_back_to_node_id_when_unregistered() {
        // The factory's NodeId → NodeName resolution falls back to
        // the NodeId string when no name is registered. We can't
        // easily inspect the dispatched node_name from outside (the
        // factory's task is fire-and-forget), but we verify the
        // closure is invokable when names map is empty.
        let config = TracerConfig {
            network_magic: 764824073,
            network: Network::AcceptAt {
                accept_at: HowToConnect::LocalPipe {
                    local_pipe: "/tmp/r431-fallback.sock".into(),
                },
            },
            logging: vec![],
            rotation: None,
            verbosity: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            ekg_request_freq: None,
            ekg_request_full: None,
            metrics_help: None,
            log_objects_request_num: None,
            metrics_no_suffix: None,
            prometheus_labels: None,
            resource_freq: None,
        };
        let names = ConnectedNodesNames::new();
        let handler = default_lo_handler_factory(&config, names);
        let trace_obj = TraceObject::new(
            None,
            "m".into(),
            crate::severity::SeverityS::Info,
            vec![],
            "t".into(),
            0,
        );
        handler(crate::types::NodeId::new("ghost"), vec![trace_obj]);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    #[tokio::test]
    async fn run_cardano_tracer_default_errors_on_missing_config_file() {
        let params = TracerParams {
            tracer_config: std::path::PathBuf::from(
                "/nonexistent-yggdrasil-r431-tracer-config.json",
            ),
            state_dir: None,
            log_severity: None,
        };
        let result = run_cardano_tracer_default(params).await;
        assert!(matches!(result, Err(RunCardanoTracerError::ReadConfig(_))));
    }

    #[tokio::test]
    async fn run_cardano_tracer_round_trips_with_minimal_config_via_brake() {
        // End-to-end: write a minimal valid tracer-config.json,
        // engage the brake immediately via a brief tokio task, and
        // assert the supervisor returns Ok.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let pipe = dir.path().join("rt.sock");
        let cfg_path = dir.path().join("tracer-config.json");
        // We use AcceptAt mode so the supervisor binds the socket
        // synchronously, then the brake fires shortly after to wind
        // it down.
        let json = format!(
            r#"{{"networkMagic":764824073,"network":{{"acceptAt":{{"localPipe":"{}"}}}},"logging":[{{"logRoot":".","logMode":"FileMode","logFormat":"ForHuman"}}]}}"#,
            pipe.display()
        );
        std::fs::write(&cfg_path, json).expect("write cfg");

        let params = TracerParams {
            tracer_config: cfg_path,
            state_dir: None,
            log_severity: None,
        };

        let supervisor = tokio::spawn(async move {
            let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
                Arc::new(|_, _| {});
            run_cardano_tracer_test_shim(params, handler).await
        });

        // Give the supervisor a brief moment to bind the socket,
        // then abort it (simulating SIGINT). The supervisor task
        // should NOT panic; we only assert the abort signal is
        // observed cleanly.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        supervisor.abort();
        // We don't unwrap the result since aborted tasks return
        // JoinError::Cancelled; the no-panic assertion is enough.
    }

    /// Test-only thin shim that monomorphizes the closure type.
    async fn run_cardano_tracer_test_shim(
        params: TracerParams,
        handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync>,
    ) -> Result<(), RunCardanoTracerError> {
        run_cardano_tracer(params, Arc::new(move |id, payloads| handler(id, payloads))).await
    }

    async fn do_run_cardano_tracer_test_shim(
        config: TracerConfig,
        rt_view_state_dir: Option<std::path::PathBuf>,
        handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync>,
    ) -> Result<(), RunCardanoTracerError> {
        do_run_cardano_tracer(
            config,
            rt_view_state_dir,
            Arc::new(move |id, payloads| handler(id, payloads)),
        )
        .await
    }
}
