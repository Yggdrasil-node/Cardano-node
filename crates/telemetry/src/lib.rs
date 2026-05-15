//! yggdrasil-telemetry — observability scaffolding.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis crate that
//! centralises observability primitives (log format selector, span
//! field-name registry, tracing-subscriber init builder). Upstream
//! `cardano-node` configures `iohk-monitoring-framework` (the
//! `contra-tracer` stack with EKG metrics and Katip JSON logs)
//! inside `Cardano.Node.Tracing`; Yggdrasil collapses the
//! corresponding Rust-side conventions into one place so all
//! binaries (yggdrasil-node plus every sister tool) initialise
//! observability identically.
//!
//! **Wave 6 PR 14 status:** `tracing` + `tracing-subscriber`
//! workspace dependencies landed; [`init_subscriber`] installs the
//! local Haskell-JSON log layer + an `EnvFilter` keyed off
//! `RUST_LOG` / `YGGDRASIL_LOG`. The OTLP forwarder layer is
//! still deferred (see PR 15/17 for the Haskell-JSON
//! formatter + the cardano-tracer Mux protocol).

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod haskell_json;
#[cfg(feature = "otlp")]
pub mod otlp;

/// Supported log output formats.
///
/// `HaskellJson` is the default so an operator migrating from
/// upstream `cardano-node` can point their existing Promtail /
/// fluentd / vector config at the new binary's stdout without
/// any schema changes. The five non-negotiable fields the schema
/// must emit are `at`, `ns`, `data`, `sev`, `thread`; `host`
/// and `app` are optional. Wave 6 PR 15 ships the formatter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LogFormat {
    /// Haskell-cardano-node-shaped JSON
    /// (`{at, ns, data, sev, thread, host?, app?}`).
    #[default]
    HaskellJson,
    /// Human-readable ANSI-coloured output. Suitable for
    /// `cargo run` and local dev; never default in production.
    Pretty,
    /// OpenTelemetry OTLP-shaped JSON for collectors that prefer
    /// the OTLP schema over the Haskell-Katip schema.
    Otel,
}

impl LogFormat {
    /// CLI string form used by `--log-format=<value>`.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogFormat::HaskellJson => "haskell-json",
            LogFormat::Pretty => "pretty",
            LogFormat::Otel => "otel",
        }
    }
}

/// Configuration passed by binary main entry-points to
/// `init_subscriber`. Wave 6 PR 14 populates the `init_subscriber`
/// function; Wave 6 PR 17 Phase 2.A wires `otlp_endpoint` through
/// to the actual exporter (`otlp` feature, off by default).
#[derive(Clone, Debug, Default)]
pub struct TracingConfig {
    /// Output format. Defaults to `HaskellJson`.
    pub format: LogFormat,
    /// Optional OTLP collector endpoint (e.g. `http://otel-collector:4317`).
    /// Honored only when the crate is compiled with `--features otlp`;
    /// non-OTLP builds leave the value in place and silently skip the
    /// exporter so a binary built without the feature still accepts
    /// the CLI flag without erroring.
    pub otlp_endpoint: Option<String>,
    /// Optional `cardano-tracer` Unix socket. When set, an additional
    /// tracer-forwarder layer is installed alongside the local logger.
    pub tracer_socket: Option<std::path::PathBuf>,
    /// OTel `service.name` attribute attached to every exported span.
    /// Defaults to `"yggdrasil-node"`; sister tools override per-binary
    /// (`cardano-submit-api`, `cardano-tracer`, …) so a shared OTel
    /// backend can multiplex multiple Yggdrasil binaries cleanly.
    pub service_name: Option<String>,
}

// Wave 6 PR 14: expose the `tracing` re-export so callers can write
// `use yggdrasil_telemetry::tracing::info;` and stay decoupled from
// the underlying crate version. The re-export costs nothing — Rust
// inlines it at compile time.
pub use tracing;

/// Span / event field name conventions. Every emit-site (the
/// `node_span!` / `consensus_span!` / etc. macros land in a follow-on
/// PR once the binary's `eprintln!` callsites get swept) references
/// these constants so a single rename here propagates everywhere.
///
/// The three correlation fields (`SLOT`, `EPOCH`, `BLOCK_HASH`) are
/// universal — every span in the consensus / network / storage hot
/// path should carry them so a Grafana / Loki query can pivot on
/// slot or block-hash across crates.
pub mod trace_fields {
    /// Span/event field for slot number (`u64`).
    pub const SLOT: &str = "slot";
    /// Span/event field for epoch number (`u64`).
    pub const EPOCH: &str = "epoch";
    /// Span/event field for the 16-hex-prefix block hash (`String`).
    pub const BLOCK_HASH: &str = "block_hash";
    /// Span/event field for the crate-qualified namespace
    /// (`<crate>::<subsystem>`, e.g. `"consensus::praos"`).
    pub const NS: &str = "ns";
    /// Span/event field for the peer endpoint (e.g. `"1.2.3.4:3001"`).
    pub const PEER: &str = "peer";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_format_default_is_haskell_json() {
        assert_eq!(LogFormat::default(), LogFormat::HaskellJson);
    }

    #[test]
    fn log_format_as_str() {
        assert_eq!(LogFormat::HaskellJson.as_str(), "haskell-json");
        assert_eq!(LogFormat::Pretty.as_str(), "pretty");
        assert_eq!(LogFormat::Otel.as_str(), "otel");
    }

    #[test]
    fn trace_fields_constants_are_stable() {
        // These names are part of the Haskell-JSON schema parity
        // contract and the EKG-parity metric registry. Changing them
        // is a semver-major break per docs/COMPATIBILITY.md (Wave 10).
        assert_eq!(trace_fields::SLOT, "slot");
        assert_eq!(trace_fields::EPOCH, "epoch");
        assert_eq!(trace_fields::BLOCK_HASH, "block_hash");
        assert_eq!(trace_fields::NS, "ns");
        assert_eq!(trace_fields::PEER, "peer");
    }

    #[test]
    fn tracing_config_default() {
        let c = TracingConfig::default();
        assert_eq!(c.format, LogFormat::HaskellJson);
        assert!(c.otlp_endpoint.is_none());
        assert!(c.tracer_socket.is_none());
        assert!(c.service_name.is_none());
    }

    /// Pin the `otlp` feature gate: when ON, the `otlp` module is
    /// reachable and `OtlpInitError` is a public type that callers
    /// can pattern-match. When OFF, the entire module is absent and
    /// none of the OpenTelemetry transitive deps are pulled in.
    #[cfg(feature = "otlp")]
    #[test]
    fn otlp_module_reachable_when_feature_on() {
        // The function exists and returns a Result. We don't drive
        // it end-to-end here because building the OTLP gRPC exporter
        // needs a Tokio context and would attempt a DNS resolve on
        // the host portion of the endpoint.
        let _: fn(&str, &str) -> Result<_, otlp::OtlpInitError> = otlp::build_tracer;
    }

    /// Pin the public-API shape of `TracingConfig::service_name` —
    /// it MUST be optional and string-shaped so `service.name` is
    /// configurable at the binary boundary without further breaking
    /// changes (per the docs/COMPATIBILITY.md Tier-1 contract).
    #[test]
    fn tracing_config_service_name_is_optional_string() {
        let c = TracingConfig {
            service_name: Some("yggdrasil-test".to_string()),
            ..Default::default()
        };
        assert_eq!(c.service_name.as_deref(), Some("yggdrasil-test"));
    }

    #[test]
    fn init_subscriber_with_dispatcher_runs_idempotently() {
        // The global subscriber install is one-shot per process and
        // ignored on a second call — confirm the function is at least
        // safe to call from inside a Cargo test process.
        let cfg = TracingConfig::default();
        let outcome = init_subscriber(&cfg);
        // The first call may install or be a no-op depending on test
        // ordering; the second call must be a no-op without panicking.
        let _ = outcome;
        let _ = init_subscriber(&cfg);
    }
}

/// Install the workspace's tracing subscriber.
///
/// Wave 6 PR 14 status: the subscriber installs the local logger
/// layer keyed off `RUST_LOG` / `YGGDRASIL_LOG` (via
/// `tracing_subscriber::EnvFilter`). Output format selection from
/// `TracingConfig::format` is wired:
///
///   - [`LogFormat::HaskellJson`]: Wave 6 PR 15 ships
///     [`haskell_json::HaskellJsonFormat`] — emits the upstream
///     Katip schema `{at, ns, data, sev, thread, host, app}` so
///     SPO log-shippers (Promtail, fluentd, vector) consume it
///     without re-pipelining.
///   - [`LogFormat::Pretty`]: ANSI-coloured stdout output.
///   - [`LogFormat::Otel`]: today behaves identically to
///     `HaskellJson`; the actual OTLP exporter layer waits on the
///     workspace adding `tracing-opentelemetry` + `opentelemetry`
///     in a follow-on PR.
///
/// Idempotent: a second call (e.g. from a test process) is a no-op
/// because `tracing-subscriber`'s global dispatcher is one-shot.
///
/// Returns `Ok(())` on first install, `Err(InitSubscriberError::
/// AlreadyInstalled)` on subsequent calls. Binary `main` functions
/// can ignore the error.
pub fn init_subscriber(config: &TracingConfig) -> Result<(), InitSubscriberError> {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    // Honor both `RUST_LOG` (standard Rust convention) and
    // `YGGDRASIL_LOG` (operator-facing alias documented in
    // docs/COMPATIBILITY.md). YGGDRASIL_LOG wins when both are set.
    let env_filter = std::env::var("YGGDRASIL_LOG")
        .ok()
        .map(EnvFilter::new)
        .or_else(|| std::env::var("RUST_LOG").ok().map(EnvFilter::new))
        .unwrap_or_else(|| EnvFilter::new("info"));

    // Wave 6 PR 17 Phase 2.A — when the operator supplies an OTLP
    // endpoint, `--log-format=otel`, AND the binary was built with
    // `--features otlp`, attach the OTLP forwarder layer alongside
    // the Haskell-JSON formatter. Out of the otlp-feature path the
    // endpoint is recorded but no exporter is constructed (the
    // binary still accepts the CLI flag, but no events leave the
    // process — flag is doc'd as a no-op in non-otlp builds).
    let registry = tracing_subscriber::registry().with(env_filter);

    let result = match config.format {
        LogFormat::HaskellJson => {
            // Wave 6 PR 15 — Haskell-Katip-shaped JSON.
            // Field set: {at, ns, data, sev, thread, host, app}.
            // SPOs migrating from upstream cardano-node 11.0.1 keep
            // their Promtail / fluentd configs unchanged. See
            // `haskell_json::HaskellJsonFormat` for the schema.
            // Never forwards to OTLP even if the endpoint is set —
            // the format selector is the operator's explicit choice
            // between "local Promtail/fluentd schema" and "OTLP+local".
            let fmt_layer = tracing_subscriber::fmt::layer()
                .event_format(haskell_json::HaskellJsonFormat::new());
            registry.with(fmt_layer).try_init()
        }
        LogFormat::Otel => {
            // Same local formatter as HaskellJson, plus the OTLP
            // exporter when feature-on AND endpoint configured.
            let fmt_layer = tracing_subscriber::fmt::layer()
                .event_format(haskell_json::HaskellJsonFormat::new());
            #[cfg(feature = "otlp")]
            {
                let service_name = config.service_name.as_deref().unwrap_or("yggdrasil-node");
                if let Some(endpoint) = &config.otlp_endpoint {
                    match otlp::build_tracer(endpoint, service_name) {
                        Ok(tracer) => {
                            let otlp_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                            registry.with(fmt_layer).with(otlp_layer).try_init()
                        }
                        Err(err) => {
                            // Don't fail-stop on a transient collector
                            // outage; emit a single warning and continue
                            // with the local formatter only.
                            eprintln!("yggdrasil-telemetry: OTLP layer disabled — {err}");
                            registry.with(fmt_layer).try_init()
                        }
                    }
                } else {
                    registry.with(fmt_layer).try_init()
                }
            }
            #[cfg(not(feature = "otlp"))]
            {
                registry.with(fmt_layer).try_init()
            }
        }
        LogFormat::Pretty => {
            // `Pretty` is dev-only; never forwards to OTLP even if
            // the endpoint is configured. Operators wanting OTLP
            // must pick `--log-format=otel`.
            registry
                .with(tracing_subscriber::fmt::layer().compact())
                .try_init()
        }
    };

    result.map_err(|_| InitSubscriberError::AlreadyInstalled)
}

/// Surfaced when `init_subscriber` is called more than once per
/// process or when something else (a downstream test harness,
/// usually) has already installed a global dispatcher.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InitSubscriberError {
    /// A global subscriber was already installed before this call.
    AlreadyInstalled,
}

impl core::fmt::Display for InitSubscriberError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AlreadyInstalled => {
                f.write_str("a global tracing subscriber was already installed")
            }
        }
    }
}

impl std::error::Error for InitSubscriberError {}
