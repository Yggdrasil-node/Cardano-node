//! Trace events emitted by the submit API.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-submit-api/src/Cardano/TxSubmit/Tracing/TraceSubmitApi.hs.
//!
//! R339 lands the `TraceSubmitApi` data-only enum so [`super::super::util::log_exception`]
//! and the [`super::super::types::TxCmdError`] integration paths can
//! type-check end-to-end. The full trace surface — i.e. upstream's
//! `LogFormatting`, `MetaTrace`, `forMachine`, `forHuman`, `asMetrics`,
//! and namespace/severity/metricsDoc tables — lands at R340 alongside
//! the web round, when the trace receiver wiring is decided. Until then
//! callers can pattern-match on the enum and forward to a concrete
//! tracing backend.

use std::net::SocketAddr;

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
}
