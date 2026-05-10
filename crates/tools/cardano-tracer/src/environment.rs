//! Runtime environment for cardano-tracer — the 14-field record
//! threaded through every subsystem (Acceptors, Handlers/Logs,
//! Handlers/Metrics, Handlers/Notifications, Run supervisor).
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Environment.hs.
//!
//! Direct port of upstream's `TracerEnv` + `TracerEnvRTView`
//! records. Each field is documented inline with the upstream
//! field name + Haskell type + Yggdrasil-side replacement (where
//! the upstream type isn't yet ported).
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                              |
//! |---------------------------------------------------|----------------------------------------|
//! | `data TracerEnv`                                  | [`TracerEnv`] 14-field struct          |
//! | `data TracerEnvRTView`                            | [`TracerEnvRTView`] (carve-out: empty when RTView is off, matching upstream's `#else` branch) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`AcceptedMetrics`**: upstream's `Cardano.Tracer.Types.AcceptedMetrics`
//!   is `TVar (Map NodeId (TVar EKG.Store))` — placeholder type
//!   [`AcceptedMetrics`] ships as an empty newtype; full port lands
//!   when the EKG-equivalent metrics surface ships.
//! - **`DataPointRequestors`**: upstream's TVar of
//!   `Map NodeId (DataPointRequestor IO)` — placeholder type
//!   [`DataPointRequestors`] ships as an empty newtype.
//! - **`Trace IO TracerTrace`**: the tracer-trace channel from
//!   upstream's `MetaTrace` / `trace-dispatcher`. Replaced with a
//!   [`TracerTrace`] placeholder until MetaTrace.hs (331 lines) +
//!   the trace-dispatcher package surface land.
//! - **`[TraceObject] -> IO ()`** reforward closure: replaced with
//!   a `Box<dyn Fn(&[TraceObject]) + Send + Sync>` boxed-closure
//!   field. The placeholder closure is a no-op until
//!   Acceptors/Run.hs ships.
//! - **`TimeseriesHandle`**: upstream's `Cardano.Timeseries.Component`
//!   handle — placeholder type [`TimeseriesHandle`] ships as an
//!   empty newtype (workspace doesn't yet vendor cardano-timeseries-io).
//! - **`TracerEnvRTView` `#if RTVIEW` branch**: the 6-field record
//!   for the RTView UI is the entire RTView carve-out per the plan.
//!   Yggdrasil-side [`TracerEnvRTView`] is intentionally empty (mirrors
//!   upstream's `#else` branch with no fields) so the type stays
//!   referenceable from sites that thread it through.

use std::path::PathBuf;
use std::sync::Arc;

use crate::configuration::TracerConfig;
use crate::handlers::logs::journal::no_systemd::TraceObject;
use crate::types::{ConnectedNodes, ConnectedNodesNames, HandleRegistry, ProtocolsBrake, Registry};

/// Per-node metrics-store registry — placeholder until the EKG-
/// equivalent metrics surface ships. Mirror of upstream
/// `type AcceptedMetrics = TVar (Map NodeId (TVar EKG.Store))`.
#[derive(Clone, Debug, Default)]
pub struct AcceptedMetrics;

/// Per-node datapoint-requestor registry — placeholder until the
/// datapoint forwarder mini-protocol ships. Mirror of upstream
/// `type DataPointRequestors = TVar (Map NodeId (DataPointRequestor IO))`.
#[derive(Clone, Debug, Default)]
pub struct DataPointRequestors;

/// Tracer-trace event channel — placeholder until MetaTrace.hs
/// (331 lines) lands. Mirror of upstream
/// `Trace IO TracerTrace`.
#[derive(Clone, Debug, Default)]
pub struct TracerTrace;

/// Cardano-timeseries handle — placeholder until cardano-timeseries-io
/// is vendored. Mirror of upstream `Cardano.Timeseries.Component.TimeseriesHandle`.
#[derive(Clone, Debug, Default)]
pub struct TimeseriesHandle;

/// Closure invoked by the trace-forwarder to reforward incoming
/// trace objects to attached log handlers. Default is a no-op.
pub type ReforwardTraceObjects = Arc<dyn Fn(&[TraceObject]) + Send + Sync>;

/// Build a no-op [`ReforwardTraceObjects`] closure suitable as a
/// default field value.
pub fn no_op_reforward() -> ReforwardTraceObjects {
    Arc::new(|_: &[TraceObject]| {})
}

/// Cardano-tracer runtime environment. Mirror of upstream
/// `data TracerEnv = TracerEnv { ... }` 14-field record. Threaded
/// through every subsystem; consumers pluck individual fields per
/// the Haskell-style `TracerEnv{teX, teY}` named-field pattern via
/// the public `te_*` accessor names below.
#[derive(Clone)]
pub struct TracerEnv {
    /// Operator-supplied configuration (parsed from `--config` JSON).
    /// Upstream: `teConfig :: !TracerConfig`.
    pub te_config: TracerConfig,
    /// Set of currently-connected node IDs (R371 surface).
    /// Upstream: `teConnectedNodes :: !ConnectedNodes`.
    pub te_connected_nodes: ConnectedNodes,
    /// Bidirectional NodeId↔NodeName mapping (R371 surface).
    /// Upstream: `teConnectedNodesNames :: !ConnectedNodesNames`.
    pub te_connected_nodes_names: ConnectedNodesNames,
    /// Per-node metrics-store registry (placeholder pending EKG
    /// surface). Upstream: `teAcceptedMetrics :: !AcceptedMetrics`.
    pub te_accepted_metrics: AcceptedMetrics,
    /// Mutex guarding the current log-rotation cycle. Upstream uses
    /// `Control.Concurrent.Extra.Lock`; Yggdrasil uses
    /// `Arc<tokio::sync::Mutex<()>>` for single-acquirer semantics.
    /// Upstream: `teCurrentLogLock :: !Lock`.
    pub te_current_log_lock: Arc<tokio::sync::Mutex<()>>,
    /// Mutex guarding the current datapoint-request cycle.
    /// Upstream: `teCurrentDPLock :: !Lock`.
    pub te_current_dp_lock: Arc<tokio::sync::Mutex<()>>,
    /// Per-node datapoint-requestor registry (placeholder pending
    /// datapoint mini-protocol port). Upstream:
    /// `teDPRequestors :: !DataPointRequestors`.
    pub te_dp_requestors: DataPointRequestors,
    /// Stop-signal flag for protocols on the acceptor side (R371
    /// surface). Upstream: `teProtocolsBrake :: !ProtocolsBrake`.
    pub te_protocols_brake: ProtocolsBrake,
    /// Tracer-trace event sink (placeholder pending MetaTrace.hs
    /// port). Upstream: `teTracer :: !(Trace IO TracerTrace)`.
    pub te_tracer: TracerTrace,
    /// Closure invoked by the trace-forwarder to reforward incoming
    /// trace objects. Upstream:
    /// `teReforwardTraceObjects :: !([TraceObject] -> IO ())`.
    pub te_reforward_trace_objects: ReforwardTraceObjects,
    /// Per-node handle registry (R371 surface). Upstream:
    /// `teRegistry :: !HandleRegistry`.
    pub te_registry: HandleRegistry,
    /// Operator-supplied state directory (`--state-dir`). `None`
    /// means "fall back to XDG defaults" per [`crate::handlers::system`].
    /// Upstream: `teStateDir :: !(Maybe FilePath)`.
    pub te_state_dir: Option<PathBuf>,
    /// Cached metrics-help text from the most recent
    /// `parseMetricsHelp` pass. Upstream's `[(Text, Builder)]` pair
    /// list is collapsed to `Vec<(String, String)>` since the lazy-
    /// builder optimization isn't relevant for the metrics-help
    /// output volume. Upstream: `teMetricsHelp :: ![(Text, Builder)]`.
    pub te_metrics_help: Vec<(String, String)>,
    /// Optional cardano-timeseries handle (placeholder pending
    /// cardano-timeseries-io vendoring). Upstream:
    /// `teTimeseriesHandle :: !(Maybe TimeseriesHandle)`.
    pub te_timeseries_handle: Option<TimeseriesHandle>,
}

impl TracerEnv {
    /// Construct a fresh environment from a [`TracerConfig`] with all
    /// other fields default-initialized. Production sites populate
    /// the runtime-state fields (te_connected_nodes, te_registry,
    /// etc.) via dedicated wiring in `Run.hs`-equivalent supervisor
    /// code (pending port).
    pub fn new(config: TracerConfig) -> Self {
        TracerEnv {
            te_config: config,
            te_connected_nodes: ConnectedNodes::default(),
            te_connected_nodes_names: ConnectedNodesNames::default(),
            te_accepted_metrics: AcceptedMetrics,
            te_current_log_lock: Arc::new(tokio::sync::Mutex::new(())),
            te_current_dp_lock: Arc::new(tokio::sync::Mutex::new(())),
            te_dp_requestors: DataPointRequestors,
            te_protocols_brake: ProtocolsBrake::default(),
            te_tracer: TracerTrace,
            te_reforward_trace_objects: no_op_reforward(),
            te_registry: Registry::new(),
            te_state_dir: None,
            te_metrics_help: Vec::new(),
            te_timeseries_handle: None,
        }
    }

    /// Override the operator state directory (for `--state-dir`
    /// flag wiring). Returns `self` so the call chains cleanly.
    pub fn with_state_dir(mut self, state_dir: Option<PathBuf>) -> Self {
        self.te_state_dir = state_dir;
        self
    }
}

impl std::fmt::Debug for TracerEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TracerEnv")
            .field("te_config", &self.te_config)
            .field("te_connected_nodes", &self.te_connected_nodes)
            .field("te_connected_nodes_names", &self.te_connected_nodes_names)
            .field("te_accepted_metrics", &self.te_accepted_metrics)
            .field("te_current_log_lock", &"<Mutex>")
            .field("te_current_dp_lock", &"<Mutex>")
            .field("te_dp_requestors", &self.te_dp_requestors)
            .field("te_protocols_brake", &self.te_protocols_brake)
            .field("te_tracer", &self.te_tracer)
            .field("te_reforward_trace_objects", &"<closure>")
            .field("te_registry", &self.te_registry)
            .field("te_state_dir", &self.te_state_dir)
            .field("te_metrics_help", &self.te_metrics_help)
            .field("te_timeseries_handle", &self.te_timeseries_handle)
            .finish()
    }
}

/// RTView-specific runtime environment. Mirror of upstream
/// `data TracerEnvRTView`. Yggdrasil's port is intentionally empty
/// (mirrors upstream's `#else` branch with `data TracerEnvRTView =
/// TracerEnvRTView`) per the workspace-wide RTView UI carve-out
/// documented in the sister-tools port plan.
#[derive(Clone, Debug, Default)]
pub struct TracerEnvRTView;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{
        HowToConnect, LogFormat, LogMode, LoggingParams, Network, TracerConfig,
    };

    fn sample_config() -> TracerConfig {
        TracerConfig {
            network_magic: 764_824_073,
            network: Network::AcceptAt {
                accept_at: HowToConnect::LocalPipe {
                    local_pipe: PathBuf::from("/tmp/test.sock"),
                },
            },
            log_objects_request_num: Some(10),
            ekg_request_freq: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            logging: vec![LoggingParams {
                root: PathBuf::from("/var/log/cardano-tracer"),
                mode: LogMode::FileMode,
                format: LogFormat::ForMachine,
            }],
            rotation: None,
            verbosity: None,
            metrics_no_suffix: None,
            metrics_help: None,
            resource_freq: None,
            ekg_request_full: None,
            prometheus_labels: None,
        }
    }

    #[test]
    fn tracer_env_new_uses_supplied_config() {
        let cfg = sample_config();
        let env = TracerEnv::new(cfg.clone());
        assert_eq!(env.te_config.network_magic, cfg.network_magic);
        assert_eq!(env.te_config.verbosity, cfg.verbosity);
    }

    #[test]
    fn tracer_env_new_default_initializes_runtime_state_fields() {
        let env = TracerEnv::new(sample_config());
        assert!(env.te_state_dir.is_none());
        assert!(env.te_metrics_help.is_empty());
        assert!(env.te_timeseries_handle.is_none());
    }

    #[tokio::test]
    async fn tracer_env_locks_acquire_independently() {
        let env = TracerEnv::new(sample_config());
        let log_guard = env.te_current_log_lock.lock().await;
        // Different lock — should acquire without contention.
        let dp_guard = env.te_current_dp_lock.lock().await;
        drop(log_guard);
        drop(dp_guard);
    }

    #[test]
    fn tracer_env_with_state_dir_overrides_field() {
        let env = TracerEnv::new(sample_config())
            .with_state_dir(Some(PathBuf::from("/var/cardano-tracer")));
        assert_eq!(env.te_state_dir, Some(PathBuf::from("/var/cardano-tracer")),);
    }

    #[test]
    fn tracer_env_with_state_dir_can_clear_to_none() {
        let env = TracerEnv::new(sample_config())
            .with_state_dir(Some(PathBuf::from("/x")))
            .with_state_dir(None);
        assert!(env.te_state_dir.is_none());
    }

    #[test]
    fn tracer_env_debug_renders_all_fields() {
        let env = TracerEnv::new(sample_config());
        let debug_str = format!("{env:?}");
        assert!(debug_str.starts_with("TracerEnv {"));
        assert!(debug_str.contains("te_config"));
        assert!(debug_str.contains("te_state_dir"));
        // Lock/closure fields collapsed to placeholder strings
        // (not Debug-derivable in the closure case).
        assert!(debug_str.contains("<Mutex>"));
        assert!(debug_str.contains("<closure>"));
    }

    #[test]
    fn no_op_reforward_does_not_panic_on_empty_input() {
        let cb = no_op_reforward();
        cb(&[]);
    }

    #[test]
    fn no_op_reforward_does_not_panic_on_non_empty_input() {
        let cb = no_op_reforward();
        cb(&[TraceObject::default(), TraceObject::default()]);
    }

    #[test]
    fn placeholder_types_construct() {
        let _: AcceptedMetrics = AcceptedMetrics;
        let _: DataPointRequestors = DataPointRequestors;
        let _: TracerTrace = TracerTrace;
        let _: TimeseriesHandle = TimeseriesHandle;
        let _: TracerEnvRTView = TracerEnvRTView;
    }

    #[test]
    fn tracer_env_clone_produces_independent_value() {
        let env = TracerEnv::new(sample_config());
        let cloned = env.clone();
        assert_eq!(env.te_config.network_magic, cloned.te_config.network_magic);
        // Locks share the Arc internally, so the two values are
        // pointing at the same Mutex (correct for the upstream
        // sharing pattern).
        assert!(Arc::ptr_eq(
            &env.te_current_log_lock,
            &cloned.te_current_log_lock,
        ));
    }
}
