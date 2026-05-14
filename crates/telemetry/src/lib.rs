//! yggdrasil-telemetry — observability scaffolding.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis crate that
//! centralises observability primitives (log format selector, span
//! field-name registry, tracing-subscriber init builder). Upstream
//! `cardano-node` configures `iohk-monitoring-framework` (`contra-tracer`
//! + EKG metrics + Katip JSON logs) inside `Cardano.Node.Tracing`;
//! Yggdrasil collapses the corresponding Rust-side conventions into
//! one place so all binaries (yggdrasil-node + every sister tool)
//! initialise observability identically.
//!
//! **Wave 2 status:** scaffold only. The crate declares the public
//! API shape (`LogFormat`, `TracingConfig`, `trace_fields::*`) so
//! consumers can begin importing from `yggdrasil-telemetry` ahead of
//! the Wave 6 PR 14 fill-in. That PR adds the `tracing`,
//! `tracing-subscriber`, and OTLP dependencies and implements
//! `init_subscriber(&TracingConfig) -> WorkerGuard`.

#![cfg_attr(test, allow(clippy::unwrap_used))]

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
/// function; for now this struct only carries the configured values
/// from CLI parsing through to where they will eventually be wired.
#[derive(Clone, Debug, Default)]
pub struct TracingConfig {
    /// Output format. Defaults to `HaskellJson`.
    pub format: LogFormat,
    /// Optional OTLP collector endpoint (e.g. `http://otel-collector:4317`).
    pub otlp_endpoint: Option<String>,
    /// Optional `cardano-tracer` Unix socket. When set, an additional
    /// tracer-forwarder layer is installed alongside the local logger.
    pub tracer_socket: Option<std::path::PathBuf>,
}

/// Span / event field name conventions. Every emit-site (Wave 6 PR 14
/// adds the `node_span!` / `consensus_span!` / etc. macros) references
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
    }
}
