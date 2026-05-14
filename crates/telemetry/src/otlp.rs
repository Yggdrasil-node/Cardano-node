//! OTLP exporter wiring for the `--otlp-endpoint=<URL>` operator surface.
//!
//! Compiled in only when the `otlp` Cargo feature is enabled. The
//! workspace default leaves it off so non-Tokio sister tools
//! (cardano-cli, bech32, …) don't drag in the OpenTelemetry +
//! gRPC + protobuf transitive deps unnecessarily; binaries that
//! benefit from OTLP (`yggdrasil-node` itself, and any sister tool
//! that elects to ship it) opt in via the feature flag.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side adapter; upstream
//! `cardano-tracer` itself does NOT speak OTLP — the upstream
//! observability surface is the `TraceForward` mini-protocol over
//! a Unix socket plus the EKG HTTP scrape endpoint. OTLP is an
//! orthogonal collector that operators already running an OTLP-
//! capable observability backend (Tempo, Grafana Agent, Datadog
//! Agent, etc.) can wire in without standing up `cardano-tracer`.
//! Wave 6 PR 17 Phase 2.B adds the cardano-tracer Mux 2/3 path
//! separately for parity with the upstream forwarder protocol.
//!
//! ## Operator surface
//!
//! - CLI: `yggdrasil-node --otlp-endpoint=http://collector:4317`
//!   (or any host with a Tonic-compatible OTLP gRPC listener).
//! - Env: `OTEL_EXPORTER_OTLP_ENDPOINT=...` honoured by the
//!   exporter builder when the CLI flag is omitted.
//! - Resource: each exported span carries `service.name=yggdrasil-node`
//!   (or the configured binary name) so collectors can route by
//!   service.

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{Config, Tracer, TracerProvider as SdkTracerProvider};

/// Build an OpenTelemetry tracer that batch-exports spans over OTLP
/// gRPC to `endpoint`.
///
/// `service_name` is the OTel resource-level `service.name`
/// attribute, propagated to every exported span. Use the binary
/// name (`yggdrasil-node`, `cardano-submit-api`, …) so a single
/// OTel backend can multiplex multiple Yggdrasil binaries.
///
/// `init_subscriber` then wraps the returned tracer in a
/// [`tracing_opentelemetry::layer`] and attaches it to the
/// global subscriber chain. Splitting tracer construction
/// (here) from layer construction (at the call site) lets the
/// type-parameter `S` of `tracing_opentelemetry::OpenTelemetryLayer<S, _>`
/// be inferred against the actual outer subscriber type — building
/// the layer here and returning `impl Layer<S>` runs into
/// inference / object-safety surprises.
pub fn build_tracer(endpoint: &str, service_name: &str) -> Result<Tracer, OtlpInitError> {
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint.to_string())
        .build_span_exporter()
        .map_err(|e| OtlpInitError::Build(e.to_string()))?;

    let resource = Resource::new(vec![KeyValue::new(
        "service.name",
        service_name.to_string(),
    )]);

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_config(Config::default().with_resource(resource))
        .build();

    Ok(provider.tracer(service_name.to_string()))
}

/// Errors surfaced from `build_tracer`. Kept opaque so callers
/// don't grow a transitive dependency on the OTLP exporter's
/// concrete error types — a future move to `reqwest` / `http`
/// would otherwise be a breaking change to `init_subscriber`'s
/// public Result type.
#[derive(Debug, Clone)]
pub enum OtlpInitError {
    /// The OTLP gRPC exporter could not be built. Typical cause: the
    /// `endpoint` URL is malformed (missing scheme, wrong port).
    Build(String),
}

impl core::fmt::Display for OtlpInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Build(msg) => write!(f, "failed to build OTLP gRPC exporter: {msg}"),
        }
    }
}

impl std::error::Error for OtlpInitError {}
