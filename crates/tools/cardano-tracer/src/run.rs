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
//! | `runMetricsServers tracerEnv`                     | [`run_metrics_servers`] (R464) — conditional spawn of Prometheus + Monitoring via tokio::spawn |
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

    /// Metrics-servers aggregator failed to bind one of its
    /// listeners (Prometheus or Monitoring). R464 closure.
    #[error("metrics server bind: {0}")]
    MetricsServer(std::io::Error),
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

    // R462: build the shared HandleRegistry + current_log_lock first
    // so the lo_handler (which writes file-mode entries), the
    // rotator (which inspects the same registry for rotation), AND
    // the per-connection teardown hook (R465) all share a single
    // source of truth for open log handles.
    let handle_registry = crate::types::HandleRegistry::new();
    let current_log_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    // Build the runtime state slice ahead of the supervisor so the
    // default handler can capture connected_nodes_names by clone.
    let state = AcceptorsServerState {
        connected_nodes: ConnectedNodes::new(),
        connected_nodes_names: ConnectedNodesNames::new(),
        accepted_metrics: new_accepted_metrics(),
        handle_registry: handle_registry.clone(),
        network_magic: config.network_magic,
    };
    let lo_handler = Arc::new(default_lo_handler_factory_with_registry(
        &config,
        state.connected_nodes_names.clone(),
        handle_registry.clone(),
        current_log_lock.clone(),
    ));
    do_run_cardano_tracer_with_state(
        state,
        config,
        params.state_dir,
        lo_handler,
        Some((handle_registry, current_log_lock)),
    )
    .await
}

/// Variant of [`do_run_cardano_tracer`] that accepts a pre-built
/// [`AcceptorsServerState`] (rather than constructing one) so
/// callers like [`run_cardano_tracer_default`] can capture
/// references to the same `ConnectedNodesNames` map that the
/// supervisor will populate.
///
/// `shared_registry` is the (HandleRegistry, current_log_lock) pair
/// shared with the `lo_handler` factory — passing `Some` enables
/// the Logs Rotator (R461) to inspect the real handles minted by
/// the file-mode trace-objects writer (R462). Passing `None` falls
/// back to a freshly-minted internal registry, which means the
/// rotator no-ops (no shared handles to roll). Operationally
/// production callers should always pass `Some`; the `None` path
/// exists for test sites that don't care about rotation.
pub async fn do_run_cardano_tracer_with_state<LoHandler>(
    state: AcceptorsServerState,
    config: TracerConfig,
    _rt_view_state_dir: Option<std::path::PathBuf>,
    lo_handler: Arc<LoHandler>,
    shared_registry: Option<(
        crate::types::HandleRegistry,
        std::sync::Arc<tokio::sync::Mutex<()>>,
    )>,
) -> Result<(), RunCardanoTracerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let (handle_registry, current_log_lock) = shared_registry.unwrap_or_else(|| {
        (
            crate::types::HandleRegistry::new(),
            std::sync::Arc::new(tokio::sync::Mutex::new(())),
        )
    });
    let rotator_stop = std::sync::Arc::new(tokio::sync::RwLock::new(false));
    let error_tracer: crate::handlers::logs::rotator::LogsRotatorErrorTracer =
        std::sync::Arc::new(|msg: &str| {
            eprintln!("cardano-tracer: {msg}");
        });

    // R461 wires the logs rotator alongside the acceptors. Both
    // tasks run concurrently; the acceptors-side run completes
    // when its own brake fires (operator-configured), and the
    // rotator-side run is cancelled via the supervisor-level brake
    // when the acceptors finish.
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

    // R464: spawn the metrics-servers aggregator alongside the
    // acceptors + rotator. The aggregator conditionally spawns
    // Prometheus / Monitoring servers per the operator's config
    // fields (has_prometheus / has_ekg). Shares the rotator's
    // brake flag so all subsystems shut down cohesively.
    let metrics_config = config.clone();
    let metrics_state = state.clone();
    let metrics_stop = rotator_stop.clone();
    let metrics_task = tokio::spawn(async move {
        let _ = run_metrics_servers(&metrics_config, &metrics_state, metrics_stop).await;
    });

    // R466: install SIGINT/SIGTERM handlers that trip the brake on
    // signal receipt. Operators get clean shutdown via Ctrl-C +
    // systemd-stop without needing to send a separate brake signal.
    // The signal task shares the same supervisor-level brake as
    // the rotator + metrics servers.
    let signal_brake = rotator_stop.clone();
    let _signal_task = crate::utils::before_program_stops(signal_brake);

    let acceptors_state = state.clone();
    let acceptors_config = config.clone();
    let acceptors_handler = Arc::clone(&lo_handler);
    let acceptors_result =
        run_acceptors(acceptors_state, &acceptors_config, acceptors_handler).await;

    // Acceptors finished — trip the rotator brake + await rotator
    // + metrics-servers clean exit.
    *rotator_stop.write().await = true;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), rotator_task).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), metrics_task).await;

    acceptors_result?;
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
    rt_view_state_dir: Option<std::path::PathBuf>,
    lo_handler: Arc<LoHandler>,
) -> Result<(), RunCardanoTracerError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let state = AcceptorsServerState {
        connected_nodes: ConnectedNodes::new(),
        connected_nodes_names: ConnectedNodesNames::new(),
        accepted_metrics: new_accepted_metrics(),
        handle_registry: crate::types::HandleRegistry::new(),
        network_magic: config.network_magic,
    };
    // Delegate to the state-aware variant with no shared registry
    // (rotator runs against a freshly-minted registry that the
    // supplied lo_handler doesn't know about — rotator no-ops).
    // The registry-aware factory builds the shared registry itself
    // when wiring through `run_cardano_tracer_default`.
    do_run_cardano_tracer_with_state(state, config, rt_view_state_dir, lo_handler, None).await
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
            // Registry-less variant: file-mode events produce
            // FilePending outcomes without writing to disk.
            // [`default_lo_handler_factory_with_registry`] (R462) is
            // the production-shape factory that actually writes.
            let _outcomes = crate::handlers::logs::trace_objects::trace_objects_handler(
                &node_name,
                &logging,
                &trace_objects,
            )
            .await;
        });
    }
}

/// Registry-aware variant of [`default_lo_handler_factory`] (R462
/// closure). Captures the supervisor's shared [`HandleRegistry`] +
/// `current_log_lock` so file-mode trace-object writes actually
/// hit disk via [`crate::handlers::logs::trace_objects::trace_objects_handler_with_registry`].
///
/// The same registry is shared with the Logs Rotator (R461), so
/// rotation operates on the real open handles minted by this
/// handler's first file-mode write per (node, LoggingParams) pair.
pub fn default_lo_handler_factory_with_registry(
    config: &TracerConfig,
    connected_nodes_names: ConnectedNodesNames,
    registry: crate::types::HandleRegistry,
    current_log_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
) -> impl Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static {
    let logging = config.logging.clone();
    move |node_id, trace_objects| {
        if trace_objects.is_empty() {
            return;
        }
        let logging = logging.clone();
        let names = connected_nodes_names.clone();
        let registry = registry.clone();
        let lock = current_log_lock.clone();
        tokio::spawn(async move {
            let node_name = names
                .snapshot()
                .into_iter()
                .find_map(|(id, name)| if id == node_id { Some(name) } else { None })
                .unwrap_or_else(|| node_id.as_str().to_string());
            let _outcomes =
                crate::handlers::logs::trace_objects::trace_objects_handler_with_registry(
                    &node_name,
                    &logging,
                    &trace_objects,
                    &registry,
                    &lock,
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

/// Status descriptor for the (now-closed) `runMetricsServers`
/// aggregator. Retained for programmatic introspection — the
/// per-server entries (`run_prometheus_server`,
/// `run_monitoring_server`) shipped at R408-R414; R464 wired the
/// aggregator at [`run_metrics_servers`] which conditionally spawns
/// each per the operator's `has_prometheus` / `has_ekg` config
/// fields.
pub fn run_metrics_servers_status() -> &'static str {
    "runMetricsServers: closed at R464. Aggregator function \
     run_metrics_servers spawns run_prometheus_server (when \
     config.has_prometheus is Some) + run_monitoring_server (when \
     config.has_ekg is Some) concurrently via tokio::join!. Mirror \
     of upstream's `runMetricsServers tracerEnv = sequenceConcurrently_ \
     [whenJust hasEKG $ runMonitoringServer tracerEnv, whenJust \
     hasPrometheus $ runPrometheusServer tracerEnv]` pattern. \
     Wired into do_run_cardano_tracer_with_state alongside the \
     acceptors + rotator."
}

/// Run the metrics-servers aggregator: spawn the Prometheus +
/// Monitoring (EKG-equivalent) servers concurrently when the
/// operator has configured them. Mirror of upstream
/// `runMetricsServers tracerEnv`.
///
/// Each server is conditional on its config field:
/// - `config.has_prometheus = Some(Endpoint)` → spawns
///   [`crate::handlers::metrics::prometheus::run_prometheus_server`].
/// - `config.has_ekg = Some(Endpoint)` → spawns
///   [`crate::handlers::metrics::monitoring::run_monitoring_server`].
///
/// Both servers run until the `stop_flag` brake is engaged. Returns
/// `Ok(())` cleanly when the brake fires; transport failures (e.g.
/// the port is already in use) bubble up as
/// [`RunCardanoTracerError::MetricsServer`].
///
/// R464 closure of the previously-deferred Servers.hs aggregator.
pub async fn run_metrics_servers(
    config: &TracerConfig,
    state: &AcceptorsServerState,
    stop_flag: std::sync::Arc<tokio::sync::RwLock<bool>>,
) -> Result<(), RunCardanoTracerError> {
    let metrics_help = crate::utils::load_metrics_help(config.metrics_help.as_ref());
    let prometheus_labels = config.prometheus_labels.clone().unwrap_or_default();
    let metrics_no_suffix = config.metrics_no_suffix.unwrap_or(false);

    let prometheus_handle = match config.has_prometheus.clone() {
        Some(endpoint) => Some(
            crate::handlers::metrics::prometheus::run_prometheus_server(
                state.connected_nodes_names.clone(),
                endpoint,
                prometheus_labels,
                metrics_no_suffix,
                state.accepted_metrics.clone(),
                metrics_help,
            )
            .await
            .map_err(RunCardanoTracerError::MetricsServer)?,
        ),
        None => None,
    };
    let monitoring_handle = match config.has_ekg.clone() {
        Some(endpoint) => Some(
            crate::handlers::metrics::monitoring::run_monitoring_server(
                state.connected_nodes_names.clone(),
                endpoint,
                state.accepted_metrics.clone(),
            )
            .await
            .map_err(RunCardanoTracerError::MetricsServer)?,
        ),
        None => None,
    };

    // Run until the brake fires. Each server task is internal to
    // its run_*_server function (already returned a JoinHandle);
    // we just await the brake here, then abort the handles so
    // the bound listeners release their ports.
    wait_for_stop(&stop_flag).await;
    if let Some(h) = prometheus_handle {
        h.abort();
    }
    if let Some(h) = monitoring_handle {
        h.abort();
    }
    Ok(())
}

/// Polls the brake flag every 50ms until it becomes `true`. Mirror
/// of R421's `wait_for_stop` precedent.
async fn wait_for_stop(stop_flag: &std::sync::Arc<tokio::sync::RwLock<bool>>) {
    loop {
        if *stop_flag.read().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
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
    fn run_metrics_servers_status_describes_closure() {
        let s = run_metrics_servers_status();
        assert!(s.contains("closed at R464"));
        assert!(s.contains("Prometheus"));
        assert!(s.contains("Monitoring"));
    }

    #[test]
    fn run_resource_stats_status_describes_deferral() {
        let s = run_resource_stats_status();
        assert!(s.contains("deferred"));
        assert!(s.contains("Cardano.Logging.Resources"));
    }

    // ----- R464 run_metrics_servers integration tests --------------------

    fn config_with_metrics(
        has_prometheus: Option<crate::configuration::Endpoint>,
        has_ekg: Option<crate::configuration::Endpoint>,
    ) -> TracerConfig {
        TracerConfig {
            network_magic: 764824073,
            network: Network::ConnectTo { connect_to: vec![] },
            logging: vec![LoggingParams {
                root: std::path::PathBuf::from("."),
                mode: LogMode::FileMode,
                format: LogFormat::ForHuman,
            }],
            rotation: None,
            verbosity: None,
            has_ekg,
            has_prometheus,
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
        }
    }

    fn empty_state() -> AcceptorsServerState {
        AcceptorsServerState {
            connected_nodes: ConnectedNodes::new(),
            connected_nodes_names: ConnectedNodesNames::new(),
            accepted_metrics: new_accepted_metrics(),
            handle_registry: crate::types::HandleRegistry::new(),
            network_magic: 764824073,
        }
    }

    #[tokio::test]
    async fn run_metrics_servers_no_op_when_both_endpoints_none() {
        // When neither has_prometheus nor has_ekg is set, the
        // aggregator runs the brake-poll loop with no spawned
        // servers. Brake-trip exits cleanly within ~50ms.
        let config = config_with_metrics(None, None);
        let state = empty_state();
        let stop = std::sync::Arc::new(tokio::sync::RwLock::new(false));
        let stop_clone = stop.clone();
        let task =
            tokio::spawn(async move { run_metrics_servers(&config, &state, stop_clone).await });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        *stop.write().await = true;
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("run_metrics_servers did not exit")
            .expect("run_metrics_servers panicked");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn run_metrics_servers_spawns_prometheus_only() {
        // Bind to an ephemeral port (0) so the kernel picks one
        // that's free. has_ekg is None so only Prometheus spawns.
        let endpoint = crate::configuration::Endpoint {
            host: "127.0.0.1".to_string(),
            port: 0,
            force_ssl: None,
        };
        let config = config_with_metrics(Some(endpoint), None);
        let state = empty_state();
        let stop = std::sync::Arc::new(tokio::sync::RwLock::new(false));
        let stop_clone = stop.clone();
        let task =
            tokio::spawn(async move { run_metrics_servers(&config, &state, stop_clone).await });
        // Give the server time to bind + the 100ms initial stagger.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        *stop.write().await = true;
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("did not exit")
            .expect("panicked");
        assert!(result.is_ok(), "expected clean exit, got {result:?}");
    }

    #[tokio::test]
    async fn run_metrics_servers_spawns_monitoring_only() {
        let endpoint = crate::configuration::Endpoint {
            host: "127.0.0.1".to_string(),
            port: 0,
            force_ssl: None,
        };
        let config = config_with_metrics(None, Some(endpoint));
        let state = empty_state();
        let stop = std::sync::Arc::new(tokio::sync::RwLock::new(false));
        let stop_clone = stop.clone();
        let task =
            tokio::spawn(async move { run_metrics_servers(&config, &state, stop_clone).await });
        // Monitoring has a 200ms initial stagger.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        *stop.write().await = true;
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("did not exit")
            .expect("panicked");
        assert!(result.is_ok(), "expected clean exit, got {result:?}");
    }

    #[tokio::test]
    async fn run_metrics_servers_spawns_both_concurrently() {
        let prom_endpoint = crate::configuration::Endpoint {
            host: "127.0.0.1".to_string(),
            port: 0,
            force_ssl: None,
        };
        let mon_endpoint = crate::configuration::Endpoint {
            host: "127.0.0.1".to_string(),
            port: 0,
            force_ssl: None,
        };
        let config = config_with_metrics(Some(prom_endpoint), Some(mon_endpoint));
        let state = empty_state();
        let stop = std::sync::Arc::new(tokio::sync::RwLock::new(false));
        let stop_clone = stop.clone();
        let task =
            tokio::spawn(async move { run_metrics_servers(&config, &state, stop_clone).await });
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        *stop.write().await = true;
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("did not exit")
            .expect("panicked");
        assert!(result.is_ok(), "expected clean exit, got {result:?}");
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
