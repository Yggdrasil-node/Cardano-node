//! Trace events emitted by the submit API.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Tracing/TraceSubmitApi.hs.
//!
//! R339 landed the `TraceSubmitApi` data-only enum + `render_human`
//! (mirror of upstream `forHuman`). R341 completes the trace surface
//! by adding the structured-output methods that mirror upstream's
//! `LogFormatting` and `MetaTrace` instances:
//!
//! | Upstream method                           | Yggdrasil method                   |
//! |-------------------------------------------|------------------------------------|
//! | `LogFormatting.forMachine _ event`        | [`TraceSubmitApi::for_machine`]    |
//! | `LogFormatting.forHuman event`            | [`TraceSubmitApi::render_human`] (R339) |
//! | `LogFormatting.asMetrics event`           | [`TraceSubmitApi::as_metrics`]     |
//! | `MetaTrace.namespaceFor event`            | [`TraceSubmitApi::namespace_for`]  |
//! | `MetaTrace.severityFor namespace _`       | [`Namespace::severity`]            |
//! | `MetaTrace.metricsDocFor namespace`       | [`Namespace::metrics_doc`]         |
//! | `MetaTrace.allNamespaces`                 | [`ALL_NAMESPACES`]                 |
//!
//! The Rust port does not implement upstream's `LogFormatting` /
//! `MetaTrace` typeclasses (no Rust analog under our tracing
//! integration), but exposes the same data through inherent methods +
//! constants. Callers wanting structured-trace forwarding can map the
//! returned values into whatever tracing backend (`tracing`, `slog`,
//! cardano-tracer NtN protocol) is wired at runtime.

use std::net::SocketAddr;

use serde_json::{Map, Value};

use crate::types::TxCmdError;

/// Renderable tx-id form: first 16 hex characters of the underlying
/// 32-byte hash. Mirrors upstream `renderMediumTxId`.
///
/// Constructed by callers from the runtime's TxId surface; preserves the
/// type-level discriminator without binding the trace event to a specific
/// upstream `TxId` representation.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MediumTxId(String);

impl MediumTxId {
    /// Wrap a pre-rendered medium-form string (caller responsible for
    /// 16-character hex truncation; mirrors upstream `renderMediumTxId`).
    pub fn from_rendered(rendered: impl Into<String>) -> Self {
        MediumTxId(rendered.into())
    }

    /// Render a 32-byte hash (or any byte slice) to its 16-hex-char
    /// medium form; mirrors upstream `renderMediumHash`.
    pub fn from_hash_bytes(bytes: &[u8]) -> Self {
        let mut hex_string = String::with_capacity(2 * bytes.len());
        for byte in bytes {
            hex_string.push_str(&format!("{byte:02x}"));
        }
        hex_string.truncate(16);
        MediumTxId(hex_string)
    }
}

impl std::fmt::Display for MediumTxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Trace events emitted by the cardano-submit-api binary.
///
/// Upstream: `data TraceSubmitApi = ApplicationStopping | ApplicationInitializeMetrics | EndpointListeningOnPort SockAddr | EndpointException Text SomeException | EndpointFailedToSubmitTransaction TxCmdError | EndpointSubmittedTransaction TxId | EndpointExiting | MetricsServerStarted Int | MetricsServerError IOException | MetricsServerPortOccupied Int | MetricsServerPortNotBound Int`
#[derive(Clone, Debug)]
pub enum TraceSubmitApi {
    /// Web API server is shutting down.
    ApplicationStopping,
    /// Initial Prometheus counter zeroing.
    ApplicationInitializeMetrics,
    /// Web API server bound to a socket and is accepting connections.
    EndpointListeningOnPort(SocketAddr),
    /// An exception escaped a tx-submission code path (logged + rethrown).
    EndpointException {
        /// Caller-supplied context label.
        context: String,
        /// `Display` form of the underlying error.
        exception: String,
    },
    /// `POST /api/submit/tx` rejected the transaction.
    EndpointFailedToSubmitTransaction(TxCmdError),
    /// `POST /api/submit/tx` accepted the transaction (medium-form id).
    EndpointSubmittedTransaction(MediumTxId),
    /// Web API server thread exiting.
    EndpointExiting,
    /// Prometheus metrics server bound to a port.
    MetricsServerStarted(u16),
    /// Prometheus metrics server I/O error.
    MetricsServerError(String),
    /// Tried to bind metrics on a port that was already in use.
    MetricsServerPortOccupied(u16),
    /// Gave up trying to bind metrics after exhausting the retry window.
    MetricsServerPortNotBound(u16),
}

impl TraceSubmitApi {
    /// Mirror of upstream `forHuman` — operator-readable single-line text
    /// per event. Useful for stdout/stderr fallback when no structured
    /// tracer is wired (R339 default before R340 integration).
    pub fn render_human(&self) -> String {
        match self {
            TraceSubmitApi::ApplicationStopping => {
                "runTxSubmitWebapi: Stopping TxSubmit API".to_string()
            }
            TraceSubmitApi::ApplicationInitializeMetrics => "Metrics initialized".to_string(),
            TraceSubmitApi::EndpointListeningOnPort(addr) => {
                format!("Web API listening on port {addr}")
            }
            TraceSubmitApi::EndpointException { context, exception } => {
                format!("{context}{exception}")
            }
            TraceSubmitApi::EndpointFailedToSubmitTransaction(err) => {
                format!(
                    "txSubmitPost: failed to submit transaction: {}",
                    crate::types::render_tx_cmd_error(err)
                )
            }
            TraceSubmitApi::EndpointSubmittedTransaction(tx_id) => {
                format!("txSubmitPost: successfully submitted transaction {tx_id}")
            }
            TraceSubmitApi::EndpointExiting => "txSubmitApp: exiting".to_string(),
            TraceSubmitApi::MetricsServerStarted(port) => {
                format!("Starting metrics server on port {port}")
            }
            TraceSubmitApi::MetricsServerError(msg) => format!("Metrics server error: {msg}"),
            TraceSubmitApi::MetricsServerPortOccupied(port) => {
                format!("Could not allocate metrics server port {port} - trying next available...")
            }
            TraceSubmitApi::MetricsServerPortNotBound(until) => format!(
                "Could not allocate any metrics port until {until} - metrics endpoint disabled"
            ),
        }
    }

    /// Mirror of upstream `LogFormatting.forMachine` — structured event
    /// payload as a JSON object.
    ///
    /// Per-event shape exactly matches upstream Aeson-derived output:
    ///
    /// | Variant                                | JSON shape                                              |
    /// |----------------------------------------|---------------------------------------------------------|
    /// | `ApplicationStopping`                  | `{}`                                                    |
    /// | `ApplicationInitializeMetrics`         | `{}`                                                    |
    /// | `EndpointListeningOnPort addr`         | `{"addr":"<addr>"}`                                     |
    /// | `EndpointException txt e`              | `{"txt":"<txt>","exception":"<exception>"}`             |
    /// | `EndpointFailedToSubmitTransaction err`| `{"error":"<rendered TxCmdError>"}`                     |
    /// | `EndpointSubmittedTransaction txid`    | `{"txId":"<medium-form txid>"}`                         |
    /// | `EndpointExiting`                      | `{}`                                                    |
    /// | `MetricsServerStarted port`            | `{"port":<port>}`                                       |
    /// | `MetricsServerError except`            | `{"exception":"<displayException except>"}`             |
    /// | `MetricsServerPortOccupied port`       | `{"port":<port>}`                                       |
    /// | `MetricsServerPortNotBound port`       | `{"port":<port>}`                                       |
    pub fn for_machine(&self) -> Map<String, Value> {
        let mut m = Map::new();
        match self {
            TraceSubmitApi::ApplicationStopping
            | TraceSubmitApi::ApplicationInitializeMetrics
            | TraceSubmitApi::EndpointExiting => {}
            TraceSubmitApi::EndpointListeningOnPort(addr) => {
                m.insert("addr".to_string(), Value::String(addr.to_string()));
            }
            TraceSubmitApi::EndpointException { context, exception } => {
                m.insert("txt".to_string(), Value::String(context.clone()));
                m.insert("exception".to_string(), Value::String(exception.clone()));
            }
            TraceSubmitApi::EndpointFailedToSubmitTransaction(err) => {
                m.insert(
                    "error".to_string(),
                    Value::String(crate::types::render_tx_cmd_error(err)),
                );
            }
            TraceSubmitApi::EndpointSubmittedTransaction(tx_id) => {
                m.insert("txId".to_string(), Value::String(tx_id.to_string()));
            }
            TraceSubmitApi::MetricsServerStarted(port)
            | TraceSubmitApi::MetricsServerPortOccupied(port)
            | TraceSubmitApi::MetricsServerPortNotBound(port) => {
                m.insert("port".to_string(), Value::Number((*port).into()));
            }
            TraceSubmitApi::MetricsServerError(msg) => {
                m.insert("exception".to_string(), Value::String(msg.clone()));
            }
        }
        m
    }

    /// Mirror of upstream `LogFormatting.asMetrics` — counter
    /// increments to apply per event.
    ///
    /// Upstream returns `[CounterM "name" maybeValue]` where
    /// `maybeValue: Maybe Int` — `Nothing` means "increment by 1",
    /// `Just v` means "set absolute value to v".
    ///
    /// Per-event mapping:
    ///
    /// | Variant                                | Counter updates                                                       |
    /// |----------------------------------------|-----------------------------------------------------------------------|
    /// | `EndpointFailedToSubmitTransaction _`  | `[Counter("tx_submit_fail", Inc)]`                                    |
    /// | `EndpointSubmittedTransaction _`       | `[Counter("tx_submit", Inc)]`                                         |
    /// | `ApplicationInitializeMetrics`         | `[Counter("tx_submit_fail", Set 0), Counter("tx_submit", Set 0)]`     |
    /// | other variants                         | `[]`                                                                  |
    pub fn as_metrics(&self) -> Vec<MetricUpdate> {
        match self {
            TraceSubmitApi::EndpointFailedToSubmitTransaction(_) => {
                vec![MetricUpdate::counter_inc("tx_submit_fail")]
            }
            TraceSubmitApi::EndpointSubmittedTransaction(_) => {
                vec![MetricUpdate::counter_inc("tx_submit")]
            }
            TraceSubmitApi::ApplicationInitializeMetrics => vec![
                MetricUpdate::counter_set("tx_submit_fail", 0),
                MetricUpdate::counter_set("tx_submit", 0),
            ],
            _ => Vec::new(),
        }
    }

    /// Mirror of upstream `MetaTrace.namespaceFor`. Returns the
    /// canonical namespace path for this event.
    pub fn namespace_for(&self) -> Namespace {
        match self {
            TraceSubmitApi::ApplicationStopping => Namespace::ApplicationStopping,
            TraceSubmitApi::ApplicationInitializeMetrics => Namespace::ApplicationInitializeMetrics,
            TraceSubmitApi::EndpointListeningOnPort(_) => Namespace::EndpointListeningOnPort,
            TraceSubmitApi::EndpointException { .. } => Namespace::EndpointException,
            TraceSubmitApi::EndpointFailedToSubmitTransaction(_) => {
                Namespace::EndpointFailedToSubmitTransaction
            }
            TraceSubmitApi::EndpointSubmittedTransaction(_) => {
                Namespace::EndpointSubmittedTransaction
            }
            TraceSubmitApi::EndpointExiting => Namespace::EndpointExiting,
            TraceSubmitApi::MetricsServerStarted(_) => Namespace::MetricsServerStarted,
            TraceSubmitApi::MetricsServerError(_) => Namespace::MetricsServerError,
            TraceSubmitApi::MetricsServerPortOccupied(_) => Namespace::MetricsServerPortOccupied,
            TraceSubmitApi::MetricsServerPortNotBound(_) => Namespace::MetricsServerPortNotBound,
        }
    }
}

/// Severity level for a trace event. Mirrors upstream `Cardano.Logging.SeverityS`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Severity {
    /// Verbose diagnostic; off by default.
    Debug,
    /// Routine operational event.
    Info,
    /// Recoverable degradation (e.g. metrics port retry).
    Warning,
    /// Operator-visible failure.
    Error,
}

/// Namespace path for a trace event. Mirrors upstream `Namespace [] [...]`.
///
/// `allNamespaces` from upstream is encoded as the discriminant set;
/// per-namespace severity/metricsDoc are inherent methods that return
/// the same data the upstream `severityFor` / `metricsDocFor` tables do.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Namespace {
    ApplicationStopping,
    ApplicationInitializeMetrics,
    EndpointListeningOnPort,
    EndpointException,
    EndpointFailedToSubmitTransaction,
    EndpointSubmittedTransaction,
    EndpointExiting,
    MetricsServerStarted,
    MetricsServerError,
    MetricsServerPortOccupied,
    MetricsServerPortNotBound,
}

/// Set of all namespaces — mirror of upstream `MetaTrace.allNamespaces`.
pub const ALL_NAMESPACES: &[Namespace] = &[
    Namespace::ApplicationStopping,
    Namespace::ApplicationInitializeMetrics,
    Namespace::EndpointListeningOnPort,
    Namespace::EndpointException,
    Namespace::EndpointFailedToSubmitTransaction,
    Namespace::EndpointSubmittedTransaction,
    Namespace::EndpointExiting,
    Namespace::MetricsServerStarted,
    Namespace::MetricsServerError,
    Namespace::MetricsServerPortOccupied,
    Namespace::MetricsServerPortNotBound,
];

impl Namespace {
    /// Mirror of upstream `MetaTrace.namespaceFor` — return the path
    /// segments for this namespace as a `["Application","Stopping"]`-
    /// shaped array (matching upstream's `Namespace [] [...]` outer
    /// `[]` plus inner segments).
    pub fn segments(&self) -> &'static [&'static str] {
        match self {
            Namespace::ApplicationStopping => &["Application", "Stopping"],
            Namespace::ApplicationInitializeMetrics => &["Application", "InitializeMetrics"],
            Namespace::EndpointListeningOnPort => &["Endpoint", "ListeningOnPort"],
            Namespace::EndpointException => &["Endpoint", "Exception"],
            Namespace::EndpointFailedToSubmitTransaction => {
                &["Endpoint", "FailedToSubmitTransaction"]
            }
            Namespace::EndpointSubmittedTransaction => &["Endpoint", "SubmittedTransaction"],
            Namespace::EndpointExiting => &["Endpoint", "Exiting"],
            Namespace::MetricsServerStarted => &["Metrics", "Started"],
            Namespace::MetricsServerError => &["Metrics", "Error"],
            Namespace::MetricsServerPortOccupied => &["Metrics", "PortOccupied"],
            Namespace::MetricsServerPortNotBound => &["Metrics", "PortNotBound"],
        }
    }

    /// Mirror of upstream `MetaTrace.severityFor`. Returns `Some(Severity)`
    /// if the upstream table has a row for this namespace, `None`
    /// otherwise. (Upstream's catch-all `severityFor _ _ = Nothing`
    /// has no rows for our enum — every variant has a defined level.)
    pub fn severity(&self) -> Option<Severity> {
        match self {
            Namespace::ApplicationStopping => Some(Severity::Info),
            Namespace::ApplicationInitializeMetrics => Some(Severity::Debug),
            Namespace::EndpointListeningOnPort => Some(Severity::Info),
            Namespace::EndpointException => Some(Severity::Error),
            Namespace::EndpointExiting => Some(Severity::Info),
            Namespace::EndpointFailedToSubmitTransaction => Some(Severity::Info),
            Namespace::EndpointSubmittedTransaction => Some(Severity::Info),
            Namespace::MetricsServerStarted => Some(Severity::Info),
            Namespace::MetricsServerError => Some(Severity::Warning),
            Namespace::MetricsServerPortOccupied => Some(Severity::Warning),
            Namespace::MetricsServerPortNotBound => Some(Severity::Error),
        }
    }

    /// Mirror of upstream `MetaTrace.metricsDocFor`. Returns the
    /// metric-name → metric-description pairs documented for this
    /// namespace. Empty list if the namespace has no metrics.
    pub fn metrics_doc(&self) -> &'static [(&'static str, &'static str)] {
        match self {
            Namespace::EndpointFailedToSubmitTransaction => {
                &[("tx_submit_fail", "Number of failed tx submissions")]
            }
            Namespace::EndpointSubmittedTransaction => {
                &[("tx_submit", "Number of successful tx submissions")]
            }
            Namespace::ApplicationInitializeMetrics => &[
                (
                    "tx_submit_fail",
                    "Initialize and set the number of successful tx submissions to 0",
                ),
                (
                    "tx_submit",
                    "Initialize and set the number of successful tx submissions to 0",
                ),
            ],
            _ => &[],
        }
    }
}

/// Counter increment instruction emitted by [`TraceSubmitApi::as_metrics`].
///
/// Mirrors upstream `Cardano.Logging.MetricM` constructors. The two
/// shapes correspond to upstream `CounterM name Nothing` (increment by
/// 1) vs `CounterM name (Just v)` (set absolute value).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum MetricUpdate {
    /// Increment the named counter by 1.
    CounterInc {
        /// Counter name (e.g. `"tx_submit"`).
        name: &'static str,
    },
    /// Set the named counter to an absolute value.
    CounterSet {
        /// Counter name (e.g. `"tx_submit_fail"`).
        name: &'static str,
        /// Absolute value to set.
        value: u64,
    },
}

impl MetricUpdate {
    /// Construct an inc-by-1 update.
    pub fn counter_inc(name: &'static str) -> Self {
        MetricUpdate::CounterInc { name }
    }

    /// Construct a set-absolute update.
    pub fn counter_set(name: &'static str, value: u64) -> Self {
        MetricUpdate::CounterSet { name, value }
    }

    /// Counter name for both variants.
    pub fn name(&self) -> &'static str {
        match self {
            MetricUpdate::CounterInc { name } | MetricUpdate::CounterSet { name, .. } => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{IpAddr, Ipv4Addr};

    use crate::types::{EnvSocketError, RawCborDecodeError, TxCmdError};

    #[test]
    fn medium_tx_id_from_hash_bytes_truncates_to_16_chars() {
        let bytes = [0xab; 32];
        let id = MediumTxId::from_hash_bytes(&bytes);
        assert_eq!(id.to_string(), "abababababababab");
        assert_eq!(id.to_string().len(), 16);
    }

    #[test]
    fn medium_tx_id_from_rendered_passes_through() {
        let id = MediumTxId::from_rendered("deadbeefdeadbeef");
        assert_eq!(id.to_string(), "deadbeefdeadbeef");
    }

    #[test]
    fn render_human_application_stopping() {
        let event = TraceSubmitApi::ApplicationStopping;
        assert_eq!(
            event.render_human(),
            "runTxSubmitWebapi: Stopping TxSubmit API"
        );
    }

    #[test]
    fn render_human_endpoint_listening() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8090);
        let event = TraceSubmitApi::EndpointListeningOnPort(addr);
        assert_eq!(
            event.render_human(),
            "Web API listening on port 127.0.0.1:8090"
        );
    }

    #[test]
    fn render_human_endpoint_failed_to_submit() {
        let err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
            message: "absent".to_string(),
        });
        let event = TraceSubmitApi::EndpointFailedToSubmitTransaction(err);
        assert_eq!(
            event.render_human(),
            "txSubmitPost: failed to submit transaction: socket env error \"absent\""
        );
    }

    #[test]
    fn render_human_endpoint_submitted() {
        let event = TraceSubmitApi::EndpointSubmittedTransaction(MediumTxId::from_rendered(
            "a1b2c3d4e5f6a7b8",
        ));
        assert_eq!(
            event.render_human(),
            "txSubmitPost: successfully submitted transaction a1b2c3d4e5f6a7b8"
        );
    }

    #[test]
    fn render_human_metrics_server_started() {
        let event = TraceSubmitApi::MetricsServerStarted(8081);
        assert_eq!(event.render_human(), "Starting metrics server on port 8081");
    }

    #[test]
    fn render_human_metrics_port_occupied() {
        let event = TraceSubmitApi::MetricsServerPortOccupied(8081);
        assert_eq!(
            event.render_human(),
            "Could not allocate metrics server port 8081 - trying next available..."
        );
    }

    #[test]
    fn render_human_metrics_port_not_bound() {
        let event = TraceSubmitApi::MetricsServerPortNotBound(9081);
        assert_eq!(
            event.render_human(),
            "Could not allocate any metrics port until 9081 - metrics endpoint disabled"
        );
    }

    #[test]
    fn render_human_endpoint_exception_concatenates_context_and_exception() {
        let event = TraceSubmitApi::EndpointException {
            context: "submit-tx-handler: ".to_string(),
            exception: "ECONNRESET".to_string(),
        };
        assert_eq!(event.render_human(), "submit-tx-handler: ECONNRESET");
    }

    #[test]
    fn render_human_metrics_server_error() {
        let event = TraceSubmitApi::MetricsServerError("EADDRINUSE".to_string());
        assert_eq!(event.render_human(), "Metrics server error: EADDRINUSE");
    }

    #[test]
    fn render_human_endpoint_exiting() {
        let event = TraceSubmitApi::EndpointExiting;
        assert_eq!(event.render_human(), "txSubmitApp: exiting");
    }

    #[test]
    fn render_human_application_initialize_metrics() {
        let event = TraceSubmitApi::ApplicationInitializeMetrics;
        assert_eq!(event.render_human(), "Metrics initialized");
    }

    #[test]
    fn endpoint_failed_to_submit_with_read_error_renders() {
        let err = TxCmdError::TxCmdTxReadError(RawCborDecodeError(vec![]));
        let event = TraceSubmitApi::EndpointFailedToSubmitTransaction(err);
        assert!(
            event
                .render_human()
                .starts_with("txSubmitPost: failed to submit transaction: transaction read error")
        );
    }

    // -- for_machine ----------------------------------------------------

    #[test]
    fn for_machine_application_stopping_is_empty() {
        let m = TraceSubmitApi::ApplicationStopping.for_machine();
        assert!(m.is_empty());
    }

    #[test]
    fn for_machine_endpoint_listening_emits_addr() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8090);
        let m = TraceSubmitApi::EndpointListeningOnPort(addr).for_machine();
        assert_eq!(
            m.get("addr"),
            Some(&Value::String("127.0.0.1:8090".to_string()))
        );
    }

    #[test]
    fn for_machine_endpoint_exception_emits_txt_and_exception() {
        let event = TraceSubmitApi::EndpointException {
            context: "submit".to_string(),
            exception: "ECONNRESET".to_string(),
        };
        let m = event.for_machine();
        assert_eq!(m.get("txt"), Some(&Value::String("submit".to_string())));
        assert_eq!(
            m.get("exception"),
            Some(&Value::String("ECONNRESET".to_string()))
        );
    }

    #[test]
    fn for_machine_failed_to_submit_emits_rendered_error() {
        let err = TxCmdError::TxCmdSocketEnvError(EnvSocketError::CliEnvVarLookup {
            message: "missing".to_string(),
        });
        let m = TraceSubmitApi::EndpointFailedToSubmitTransaction(err).for_machine();
        assert_eq!(
            m.get("error"),
            Some(&Value::String("socket env error \"missing\"".to_string()))
        );
    }

    #[test]
    fn for_machine_submitted_emits_tx_id() {
        let event = TraceSubmitApi::EndpointSubmittedTransaction(MediumTxId::from_rendered(
            "abcdef0123456789",
        ));
        let m = event.for_machine();
        assert_eq!(
            m.get("txId"),
            Some(&Value::String("abcdef0123456789".to_string()))
        );
    }

    #[test]
    fn for_machine_metrics_server_started_emits_port_number() {
        let m = TraceSubmitApi::MetricsServerStarted(8081).for_machine();
        assert_eq!(m.get("port"), Some(&Value::Number(8081.into())));
    }

    #[test]
    fn for_machine_metrics_server_error_emits_exception_string() {
        let m = TraceSubmitApi::MetricsServerError("EADDRINUSE".to_string()).for_machine();
        assert_eq!(
            m.get("exception"),
            Some(&Value::String("EADDRINUSE".to_string()))
        );
    }

    // -- as_metrics -----------------------------------------------------

    #[test]
    fn as_metrics_failed_increments_tx_submit_fail() {
        let err = TxCmdError::TxCmdTxSubmitConnectionError("x".to_string());
        let updates = TraceSubmitApi::EndpointFailedToSubmitTransaction(err).as_metrics();
        assert_eq!(updates, vec![MetricUpdate::counter_inc("tx_submit_fail")]);
    }

    #[test]
    fn as_metrics_submitted_increments_tx_submit() {
        let updates = TraceSubmitApi::EndpointSubmittedTransaction(MediumTxId::from_rendered("x"))
            .as_metrics();
        assert_eq!(updates, vec![MetricUpdate::counter_inc("tx_submit")]);
    }

    #[test]
    fn as_metrics_initialize_zeroes_both_counters() {
        let updates = TraceSubmitApi::ApplicationInitializeMetrics.as_metrics();
        assert_eq!(
            updates,
            vec![
                MetricUpdate::counter_set("tx_submit_fail", 0),
                MetricUpdate::counter_set("tx_submit", 0),
            ]
        );
    }

    #[test]
    fn as_metrics_other_events_emit_no_updates() {
        let events = [
            TraceSubmitApi::ApplicationStopping,
            TraceSubmitApi::EndpointExiting,
            TraceSubmitApi::EndpointListeningOnPort(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                8090,
            )),
            TraceSubmitApi::MetricsServerStarted(8081),
            TraceSubmitApi::MetricsServerError("x".to_string()),
            TraceSubmitApi::MetricsServerPortOccupied(8081),
            TraceSubmitApi::MetricsServerPortNotBound(8081),
            TraceSubmitApi::EndpointException {
                context: "x".to_string(),
                exception: "y".to_string(),
            },
        ];
        for event in events {
            assert!(event.as_metrics().is_empty(), "event: {event:?}");
        }
    }

    // -- namespace_for / Namespace --------------------------------------

    #[test]
    fn namespace_for_application_stopping() {
        assert_eq!(
            TraceSubmitApi::ApplicationStopping.namespace_for(),
            Namespace::ApplicationStopping
        );
        assert_eq!(
            Namespace::ApplicationStopping.segments(),
            &["Application", "Stopping"]
        );
    }

    #[test]
    fn namespace_for_endpoint_failed_to_submit() {
        let event = TraceSubmitApi::EndpointFailedToSubmitTransaction(
            TxCmdError::TxCmdTxSubmitConnectionError("x".to_string()),
        );
        assert_eq!(
            event.namespace_for(),
            Namespace::EndpointFailedToSubmitTransaction
        );
        assert_eq!(
            Namespace::EndpointFailedToSubmitTransaction.segments(),
            &["Endpoint", "FailedToSubmitTransaction"]
        );
    }

    #[test]
    fn all_namespaces_covers_every_event_variant_namespace() {
        let event_namespaces: std::collections::HashSet<Namespace> = [
            TraceSubmitApi::ApplicationStopping,
            TraceSubmitApi::ApplicationInitializeMetrics,
            TraceSubmitApi::EndpointListeningOnPort(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                0,
            )),
            TraceSubmitApi::EndpointException {
                context: String::new(),
                exception: String::new(),
            },
            TraceSubmitApi::EndpointFailedToSubmitTransaction(
                TxCmdError::TxCmdTxSubmitConnectionError(String::new()),
            ),
            TraceSubmitApi::EndpointSubmittedTransaction(MediumTxId::from_rendered("")),
            TraceSubmitApi::EndpointExiting,
            TraceSubmitApi::MetricsServerStarted(0),
            TraceSubmitApi::MetricsServerError(String::new()),
            TraceSubmitApi::MetricsServerPortOccupied(0),
            TraceSubmitApi::MetricsServerPortNotBound(0),
        ]
        .into_iter()
        .map(|e| e.namespace_for())
        .collect();
        let all: std::collections::HashSet<Namespace> = ALL_NAMESPACES.iter().copied().collect();
        assert_eq!(event_namespaces, all);
    }

    // -- severity / metrics_doc -----------------------------------------

    #[test]
    fn severity_application_stopping_is_info() {
        assert_eq!(
            Namespace::ApplicationStopping.severity(),
            Some(Severity::Info)
        );
    }

    #[test]
    fn severity_application_initialize_metrics_is_debug() {
        assert_eq!(
            Namespace::ApplicationInitializeMetrics.severity(),
            Some(Severity::Debug)
        );
    }

    #[test]
    fn severity_endpoint_exception_is_error() {
        assert_eq!(
            Namespace::EndpointException.severity(),
            Some(Severity::Error)
        );
    }

    #[test]
    fn severity_metrics_warning_levels() {
        assert_eq!(
            Namespace::MetricsServerError.severity(),
            Some(Severity::Warning)
        );
        assert_eq!(
            Namespace::MetricsServerPortOccupied.severity(),
            Some(Severity::Warning)
        );
        assert_eq!(
            Namespace::MetricsServerPortNotBound.severity(),
            Some(Severity::Error)
        );
    }

    #[test]
    fn metrics_doc_failed_to_submit() {
        assert_eq!(
            Namespace::EndpointFailedToSubmitTransaction.metrics_doc(),
            &[("tx_submit_fail", "Number of failed tx submissions")]
        );
    }

    #[test]
    fn metrics_doc_submitted_transaction() {
        assert_eq!(
            Namespace::EndpointSubmittedTransaction.metrics_doc(),
            &[("tx_submit", "Number of successful tx submissions")]
        );
    }

    #[test]
    fn metrics_doc_initialize_metrics_lists_both_counters() {
        let docs = Namespace::ApplicationInitializeMetrics.metrics_doc();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].0, "tx_submit_fail");
        assert_eq!(docs[1].0, "tx_submit");
    }

    #[test]
    fn metrics_doc_other_namespaces_empty() {
        for ns in [
            Namespace::ApplicationStopping,
            Namespace::EndpointListeningOnPort,
            Namespace::EndpointException,
            Namespace::EndpointExiting,
            Namespace::MetricsServerStarted,
            Namespace::MetricsServerError,
            Namespace::MetricsServerPortOccupied,
            Namespace::MetricsServerPortNotBound,
        ] {
            assert!(ns.metrics_doc().is_empty(), "ns: {ns:?}");
        }
    }

    // -- MetricUpdate constructors -------------------------------------

    #[test]
    fn metric_update_counter_inc_round_trip() {
        let m = MetricUpdate::counter_inc("x");
        assert_eq!(m.name(), "x");
        match m {
            MetricUpdate::CounterInc { name } => assert_eq!(name, "x"),
            _ => panic!("expected CounterInc"),
        }
    }

    #[test]
    fn metric_update_counter_set_round_trip() {
        let m = MetricUpdate::counter_set("y", 7);
        assert_eq!(m.name(), "y");
        match m {
            MetricUpdate::CounterSet { name, value } => {
                assert_eq!(name, "y");
                assert_eq!(value, 7);
            }
            _ => panic!("expected CounterSet"),
        }
    }
}
