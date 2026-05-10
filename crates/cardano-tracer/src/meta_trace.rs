//! Trace-event taxonomy for the cardano-tracer's own self-tracing.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/MetaTrace.hs.
//!
//! Direct port of upstream's `data TracerTrace` 25-variant sum
//! type + supporting types. The enum carries every meta-trace
//! event the tracer emits about itself (init/shutdown lifecycle,
//! per-server start banners, per-connection sock events, error
//! surface, resource samples, forwarder-interruption notices).
//!
//! Mapping summary:
//!
//! | Upstream                                    | Yggdrasil                              |
//! |---------------------------------------------|----------------------------------------|
//! | `data TracerTrace`                          | [`TracerTrace`] enum (25 variants)     |
//! | `data TraceBundle`                          | [`TraceBundle`] struct                 |
//! | `Trace IO TracerTrace`                      | [`Trace`] placeholder type alias       |
//! | `rtViewConfigWarning :: Text`               | [`RT_VIEW_CONFIG_WARNING`]             |
//! | `instance LogFormatting TracerTrace`        | [`TracerTrace::for_human`] / [`TracerTrace::for_machine`] |
//! | `instance MetaTrace TracerTrace`            | (deferred — see [`meta_trace_instance_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`MetaTrace TracerTrace` instance**: upstream's `MetaTrace`
//!   typeclass binds each variant to a severity + documentation
//!   string. Yggdrasil's port drops the typeclass (Rust has no
//!   typeclasses; the equivalent would be a `MetaTrace` trait with
//!   `severity` + `docs` methods on `TracerTrace`). The full
//!   severity-classification table is deferred — call sites that
//!   need it can request via [`meta_trace_instance_status`].
//! - **`Trace IO TracerTrace`**: upstream's `Trace` from
//!   `Cardano.Logging` is a contravariant tracer that runs an `IO`
//!   action per event. Yggdrasil's [`Trace<T>`] placeholder is an
//!   `Arc<dyn Fn(&T) + Send + Sync>` boxed-closure type. The
//!   `nullTracer` constructor returns a no-op closure suitable as
//!   a default field value.
//! - **`Cardano.Logging.Resources.ResourceStats`** + **`Cardano.Timeseries.Component.Trace.TimeseriesTrace`**:
//!   referenced from two TracerTrace variants + TraceBundle.
//!   Replaced with placeholder unit structs ([`ResourceStats`],
//!   [`TimeseriesTrace`]) until the upstream packages are vendored.
//! - **JSON forMachine output**: upstream emits a discriminated
//!   union with `"kind"` + named fields. The Rust port uses
//!   `#[serde(tag = "kind")]` for compatible byte-equivalent
//!   output, but variant-specific field renames (`AcceptorsAddr`,
//!   `connectionIncomingAt` for both Sock variants) are tracked
//!   verbatim via `#[serde(rename = ...)]` annotations.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::configuration::{Endpoint, HowToConnect, Network, TracerConfig};
use crate::severity::SeverityS;
use crate::types::{NodeId, NodeName};

/// Warning emitted when an operator config requests RTView but the
/// build-time `RTVIEW` flag is off. Mirror of upstream
/// `rtViewConfigWarning :: Text`.
pub const RT_VIEW_CONFIG_WARNING: &str =
    "RTView requested in config but cardano-tracer was built without it";

/// Resource-stats placeholder — full type is in
/// `Cardano.Logging.Resources` (unported). Yggdrasil's port keeps
/// the variant-shape so call sites can construct a `TracerResource`
/// event without the full payload.
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ResourceStats;

/// Timeseries-trace placeholder — full type is in
/// `Cardano.Timeseries.Component.Trace` (unported, package not
/// vendored).
#[derive(Clone, Debug, Default)]
pub struct TimeseriesTrace;

/// Closure-based tracer — runs an `IO` action per event. Mirror of
/// upstream `type Trace m a` from `Cardano.Logging`. Yggdrasil
/// represents it as a boxed sync closure since the events are
/// emitted from short-lived contexts that don't need full async
/// machinery (a future round can swap to
/// `Arc<dyn Fn(&T) -> BoxFuture<'_, ()> + Send + Sync>` if async
/// trace sinks become necessary).
pub type Trace<T> = Arc<dyn Fn(&T) + Send + Sync>;

/// Construct a no-op tracer suitable as a default field value.
/// Mirror of upstream `nullTracer :: Trace m a`.
pub fn null_tracer<T: 'static>() -> Trace<T> {
    Arc::new(|_: &T| {})
}

/// Cardano-tracer self-trace event taxonomy. Mirror of upstream
/// `data TracerTrace` (25 variants).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TracerTrace {
    /// Static information about the build.
    TracerBuildInfo {
        /// `true` when built with the `RTVIEW` flag enabled.
        #[serde(rename = "builtWithRTView")]
        tt_built_with_rt_view: bool,
    },
    /// Operator-supplied parameters echoed at boot.
    TracerParamsAre {
        /// Path to the operator config file.
        #[serde(rename = "configPath")]
        tt_config_path: PathBuf,
        /// Operator-supplied state directory (`--state-dir`).
        #[serde(rename = "stateDir")]
        tt_state_dir: Option<PathBuf>,
        /// Minimum log severity threshold.
        #[serde(rename = "minLogSeverity")]
        tt_min_log_severity: Option<SeverityS>,
    },
    /// Resolved [`TracerConfig`] echoed at boot.
    TracerConfigIs {
        /// The full resolved configuration.
        #[serde(rename = "config")]
        tt_config: TracerConfig,
        /// `true` when an RTView config was supplied but the build
        /// has the feature disabled (will trigger
        /// [`RT_VIEW_CONFIG_WARNING`]).
        #[serde(rename = "warnRTViewMissing")]
        tt_warn_rt_view_missing: bool,
    },
    /// Init pipeline started.
    TracerInitStarted,
    /// Event-queues init started.
    TracerInitEventQueues,
    /// Init pipeline complete.
    TracerInitDone,
    /// New `(node_id, node_name)` pair registered in the bidirectional
    /// connected-nodes map.
    TracerAddNewNodeIdMapping {
        /// The newly-inserted (id, name) pair.
        #[serde(rename = "bimapping")]
        tt_bimapping: (NodeId, NodeName),
    },
    /// Log-rotator subsystem started.
    TracerStartedLogRotator,
    /// Prometheus exporter started on the supplied endpoint.
    TracerStartedPrometheus {
        /// Bind endpoint.
        #[serde(rename = "endpoint")]
        tt_prometheus_endpoint: Endpoint,
    },
    /// Timeseries server started.
    TracerStartedTimeseries {
        /// Bind endpoint.
        #[serde(rename = "endpoint")]
        tt_timeseries_endpoint: Endpoint,
    },
    /// Monitoring (EKG) server started.
    TracerStartedMonitoring {
        /// Bind endpoint.
        #[serde(rename = "endpoint")]
        tt_monitoring_endpoint: Endpoint,
        /// Monitoring backend type identifier.
        #[serde(rename = "type")]
        tt_monitoring_type: String,
    },
    /// Trace-forwarder acceptor subsystem started.
    TracerStartedAcceptors {
        /// Network mode (server vs client).
        #[serde(rename = "AcceptorsAddr")]
        tt_acceptors_addr: Network,
    },
    /// RTView UI server started.
    TracerStartedRTView,
    /// Reforwarder subsystem started.
    TracerStartedReforwarder,
    /// Acceptor socket listening at path.
    TracerSockListen {
        /// Path being listened on.
        #[serde(rename = "listenAt")]
        tt_listen_at: PathBuf,
    },
    /// Incoming connection accepted.
    TracerSockIncoming {
        /// Path the connection arrived on.
        #[serde(rename = "connectionIncomingAt")]
        tt_connection_incoming_at: PathBuf,
        /// Remote address.
        #[serde(rename = "addr")]
        tt_addr: String,
    },
    /// Forwarder socket connecting to a peer.
    TracerSockConnecting {
        /// Path being connected to. Note: upstream uses
        /// `connectionIncomingAt` as the JSON key for both
        /// `TracerSockIncoming` and `TracerSockConnecting`. The
        /// rename below preserves that quirk for byte-equivalent
        /// output.
        #[serde(rename = "connectionIncomingAt")]
        tt_connecting_to: PathBuf,
    },
    /// Forwarder socket connected.
    TracerSockConnected {
        /// Path connected to.
        #[serde(rename = "connectedTo")]
        tt_connected_to: PathBuf,
    },
    /// Shutdown initiated.
    TracerShutdownInitiated,
    /// Shutdown history-backup phase complete.
    TracerShutdownHistBackup,
    /// Shutdown complete.
    TracerShutdownComplete,
    /// Generic error event.
    TracerError {
        /// Free-form error message.
        #[serde(rename = "error")]
        tt_error: String,
    },
    /// Operator-supplied TLS certificate not found at the configured
    /// path.
    TracerMissingCertificate {
        /// The endpoint that needed the certificate.
        #[serde(rename = "endpoint")]
        tt_missing_certificate_endpoint: Endpoint,
    },
    /// Resource-stats sample (CPU / memory / GC). Note: upstream's
    /// `forMachine` for this variant flattens the inner `ResourceStats`
    /// into the parent JSON object (no `"kind"` discriminant for
    /// the flattened fields). Yggdrasil's port keeps the
    /// `"TracerResource"` discriminant for serde tagging consistency
    /// — sites that need byte-equivalent flattened output should
    /// post-process the JSON.
    TracerResource {
        /// Resource-stats sample.
        #[serde(rename = "resource")]
        tt_resource: ResourceStats,
    },
    /// Trace-forwarder connection interrupted.
    TracerForwardingInterrupted {
        /// Connection target.
        #[serde(rename = "conn")]
        tt_connection: HowToConnect,
        /// Failure message.
        #[serde(rename = "msg")]
        tt_message: String,
    },
}

impl TracerTrace {
    /// Render a human-friendly one-line summary. Mirror of
    /// upstream's `forHuman` instance method (which only produces
    /// non-empty output for [`TracerTrace::TracerConfigIs`] with
    /// `warnRTViewMissing = true` and [`TracerTrace::TracerForwardingInterrupted`]).
    pub fn for_human(&self) -> String {
        match self {
            TracerTrace::TracerConfigIs {
                tt_warn_rt_view_missing: true,
                ..
            } => format!("{RT_VIEW_CONFIG_WARNING}: "),
            TracerTrace::TracerForwardingInterrupted {
                tt_connection,
                tt_message,
            } => format!("connection with {tt_connection:?} failed: {tt_message}"),
            _ => String::new(),
        }
    }

    /// Render the structured JSON form. Mirror of upstream's
    /// `forMachine` instance method. Returns the serialized JSON
    /// value; sites that need a string can call
    /// `serde_json::to_string` directly on the variant.
    pub fn for_machine(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Bundle of domain-split tracers used by the cardano-tracer
/// application. Mirror of upstream `data TraceBundle`.
#[derive(Clone)]
pub struct TraceBundle {
    /// Tracer used for general application events.
    pub assorted: Trace<TracerTrace>,
    /// Tracer used for timeseries events.
    pub timeseries: Trace<TimeseriesTrace>,
}

impl Default for TraceBundle {
    fn default() -> Self {
        TraceBundle {
            assorted: null_tracer(),
            timeseries: null_tracer(),
        }
    }
}

impl std::fmt::Debug for TraceBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TraceBundle")
            .field("assorted", &"<closure>")
            .field("timeseries", &"<closure>")
            .finish()
    }
}

/// Status descriptor for the deferred `MetaTrace TracerTrace`
/// instance.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MetaTraceInstanceStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references upstream's typeclass surface.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for the `MetaTrace TracerTrace`
/// instance.
pub fn meta_trace_instance_status() -> MetaTraceInstanceStatus {
    MetaTraceInstanceStatus {
        status: "deferred",
        depends_on: "Cardano.Logging.MetaTrace typeclass — would be a Rust trait with severity()/docs() methods. Defer to the future round that ships the trace-dispatcher port.",
        deferred_round: "R398+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{HowToConnect, LogFormat, LogMode, LoggingParams, Network};

    fn sample_config() -> TracerConfig {
        TracerConfig {
            network_magic: 764_824_073,
            network: Network::AcceptAt {
                accept_at: HowToConnect::LocalPipe {
                    local_pipe: PathBuf::from("/tmp/test.sock"),
                },
            },
            log_objects_request_num: None,
            ekg_request_freq: None,
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            logging: vec![LoggingParams {
                root: PathBuf::from("/var/log"),
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
    fn rt_view_config_warning_matches_upstream() {
        assert_eq!(
            RT_VIEW_CONFIG_WARNING,
            "RTView requested in config but cardano-tracer was built without it",
        );
    }

    #[test]
    fn null_tracer_does_not_panic() {
        let t: Trace<TracerTrace> = null_tracer();
        t(&TracerTrace::TracerInitStarted);
    }

    #[test]
    fn tracer_trace_init_started_serializes_with_kind_discriminant() {
        let event = TracerTrace::TracerInitStarted;
        let json = serde_json::to_value(&event).expect("serializes");
        assert_eq!(json["kind"], "TracerInitStarted");
    }

    #[test]
    fn tracer_trace_build_info_serializes_with_camel_case_field() {
        let event = TracerTrace::TracerBuildInfo {
            tt_built_with_rt_view: true,
        };
        let json = serde_json::to_value(&event).expect("serializes");
        assert_eq!(json["kind"], "TracerBuildInfo");
        assert_eq!(json["builtWithRTView"], true);
    }

    #[test]
    fn tracer_trace_error_serializes_with_error_field() {
        let event = TracerTrace::TracerError {
            tt_error: "synthetic test error".to_string(),
        };
        let json = serde_json::to_value(&event).expect("serializes");
        assert_eq!(json["kind"], "TracerError");
        assert_eq!(json["error"], "synthetic test error");
    }

    #[test]
    fn tracer_trace_sock_listen_serializes_with_listen_at_field() {
        let event = TracerTrace::TracerSockListen {
            tt_listen_at: PathBuf::from("/tmp/sock"),
        };
        let json = serde_json::to_value(&event).expect("serializes");
        assert_eq!(json["kind"], "TracerSockListen");
        assert_eq!(json["listenAt"], "/tmp/sock");
    }

    #[test]
    fn tracer_trace_add_new_node_id_mapping_serializes() {
        let event = TracerTrace::TracerAddNewNodeIdMapping {
            tt_bimapping: (NodeId::new("node-1"), "alpha".to_string()),
        };
        let json = serde_json::to_value(&event).expect("serializes");
        assert_eq!(json["kind"], "TracerAddNewNodeIdMapping");
    }

    #[test]
    fn tracer_trace_for_human_empty_for_init_events() {
        assert_eq!(TracerTrace::TracerInitStarted.for_human(), "");
        assert_eq!(TracerTrace::TracerInitDone.for_human(), "");
    }

    #[test]
    fn tracer_trace_for_human_returns_warning_when_rt_view_missing() {
        let event = TracerTrace::TracerConfigIs {
            tt_config: sample_config(),
            tt_warn_rt_view_missing: true,
        };
        let human = event.for_human();
        assert!(human.contains(RT_VIEW_CONFIG_WARNING));
    }

    #[test]
    fn tracer_trace_for_human_renders_forwarding_interrupted() {
        let event = TracerTrace::TracerForwardingInterrupted {
            tt_connection: HowToConnect::LocalPipe {
                local_pipe: PathBuf::from("/tmp/x.sock"),
            },
            tt_message: "EOF".to_string(),
        };
        let human = event.for_human();
        assert!(human.contains("connection with"));
        assert!(human.contains("EOF"));
    }

    #[test]
    fn tracer_trace_for_machine_returns_json_value() {
        let event = TracerTrace::TracerInitDone;
        let val = event.for_machine();
        assert_eq!(val["kind"], "TracerInitDone");
    }

    #[test]
    fn tracer_trace_round_trips_through_json() {
        for event in [
            TracerTrace::TracerInitStarted,
            TracerTrace::TracerInitDone,
            TracerTrace::TracerStartedLogRotator,
            TracerTrace::TracerShutdownInitiated,
            TracerTrace::TracerShutdownComplete,
        ] {
            let json = serde_json::to_string(&event).expect("serializes");
            let back: TracerTrace = serde_json::from_str(&json).expect("round-trips");
            assert_eq!(back, event);
        }
    }

    #[test]
    fn tracer_trace_sock_connecting_uses_upstream_typo_key() {
        // Upstream uses `connectionIncomingAt` as the JSON key for
        // TracerSockConnecting too — Yggdrasil preserves this
        // verbatim for byte-equivalent output.
        let event = TracerTrace::TracerSockConnecting {
            tt_connecting_to: PathBuf::from("/tmp/peer.sock"),
        };
        let json = serde_json::to_value(&event).expect("serializes");
        assert!(json.get("connectionIncomingAt").is_some());
    }

    #[test]
    fn trace_bundle_default_uses_null_tracers() {
        let bundle = TraceBundle::default();
        // Both tracers are no-ops; calling them shouldn't panic.
        (bundle.assorted)(&TracerTrace::TracerInitStarted);
        (bundle.timeseries)(&TimeseriesTrace);
    }

    #[test]
    fn trace_bundle_debug_renders_closures_as_placeholders() {
        let bundle = TraceBundle::default();
        let s = format!("{bundle:?}");
        assert!(s.contains("<closure>"));
    }

    #[test]
    fn meta_trace_instance_status_describes_deferral() {
        let s = meta_trace_instance_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("MetaTrace"));
    }
}
