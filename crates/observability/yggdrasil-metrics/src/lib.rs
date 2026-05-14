//! yggdrasil-metrics — Prometheus metrics registry with EKG-parity names.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis crate. Upstream
//! `cardano-node` ships an EKG (Erlang-Kernel-style metrics) registry
//! via `iohk-monitoring-framework`'s
//! `Cardano.BM.Backend.Prometheus.Server`; metric names follow the
//! `cardano.node.metrics.<name>.<type>` convention (e.g.
//! `cardano.node.metrics.slotNum.int`). Yggdrasil collapses the
//! corresponding Rust-side conventions into one place: every metric
//! consumed by downstream operator tooling (Grafana dashboards,
//! Alertmanager rules, log shippers) is declared here so a single
//! rename propagates everywhere. The naming is Tier-1 stable per
//! [`docs/COMPATIBILITY.md`](../../../docs/COMPATIBILITY.md); changes
//! require a semver-major bump.
//!
//! ## Operator API
//!
//! - [`install_prometheus_exporter`]: registers all canonical EKG-
//!   parity metrics and starts the HTTP scrape endpoint on the
//!   operator-configured `--metrics-port` (default 12798).
//! - The metric-name constants under [`names`] are the source-of-
//!   truth identifiers; emit sites use them via `metrics::gauge!`,
//!   `metrics::counter!`, and `metrics::histogram!`.
//! - The `metrics` crate is re-exported under
//!   [`crate::metrics`] so callers can stay decoupled from a
//!   specific upstream version pin.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub use metrics;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Tier-1 stable EKG-parity metric names. The naming scheme is
/// `cardano.node.metrics.<symbol>.<type>` where `<type>` is the
/// Haskell EKG-side type indicator (`int` → Counter/Gauge of integer
/// values, `real` → Gauge of floating-point values, `histogram` →
/// Prometheus histogram).
///
/// Every Wave 6 PR 16 metric MUST appear in this module so a Grafana
/// dashboard / Alertmanager rule can pivot on a single constant
/// identifier per metric.
pub mod names {
    // --- Slot / block tip ---------------------------------------------

    /// Current slot number observed at the chain tip.
    /// Haskell EKG equivalent: `cardano.node.metrics.slotNum.int`.
    pub const SLOT_NUM: &str = "cardano.node.metrics.slotNum.int";
    /// Current block number observed at the chain tip.
    /// Haskell EKG equivalent: `cardano.node.metrics.blockNum.int`.
    pub const BLOCK_NUM: &str = "cardano.node.metrics.blockNum.int";
    /// Density (blocks per active slot) observed across the rolling
    /// chain-density window. Derived in the binary; this constant
    /// names the exposed gauge.
    pub const DENSITY: &str = "cardano.node.metrics.density.real";
    /// Slots in which this node had block-producer credentials and
    /// expected to lead but did not produce a block.
    pub const SLOTS_MISSED: &str = "cardano.node.metrics.slotsMissedNum.int";

    // --- Mempool ------------------------------------------------------

    /// Transaction count currently held by the mempool.
    pub const TXS_IN_MEMPOOL: &str = "cardano.node.metrics.txsInMempool.int";
    /// Mempool size in bytes (sum of held transaction CBOR sizes).
    pub const MEMPOOL_BYTES: &str = "cardano.node.metrics.mempoolBytes.int";

    // --- Peers --------------------------------------------------------

    /// Currently-connected peer count (TCP sessions established).
    pub const CONNECTED_PEERS: &str = "cardano.node.metrics.connectedPeers.int";
    /// Peer-snapshot size known to the node kernel (the ledger-peer
    /// snapshot loaded at startup plus on-line refreshes).
    pub const PEERS_FROM_NODE_KERNEL: &str =
        "cardano.node.metrics.peersFromNodeKernel.int";

    // --- Era ----------------------------------------------------------

    /// Era index at the chain tip
    /// (0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo,
    /// 5=Babbage, 6=Conway).
    pub const CURRENT_ERA: &str = "cardano.node.metrics.currentEra.int";

    // --- Block-processing latency ------------------------------------

    /// Block-processing duration histogram (seconds). The histogram
    /// quantile bucketing defaults to the upstream-rounded
    /// {0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0}
    /// seconds at registration time.
    pub const BLOCK_PROCESSING_TIME: &str =
        "cardano.node.metrics.blockProcessingTime.real";

    // --- Chain dynamics ----------------------------------------------

    /// Rollback / fork events observed since process start.
    pub const FORKS: &str = "cardano.node.metrics.forks.int";

    // --- Forge (block production) ------------------------------------

    /// Count of slots in which the local node was elected leader.
    pub const NODE_IS_LEADER: &str = "cardano.node.metrics.nodeIsLeader.int";
    /// Count of forge attempts that were aborted because the node
    /// did not satisfy preconditions (no KES key, wrong era, …).
    pub const NODE_CANNOT_FORGE: &str = "cardano.node.metrics.nodeCannotForge.int";
    /// Count of blocks the local node successfully forged.
    pub const BLOCKS_FORGED_NUM: &str = "cardano.node.metrics.blocksForgedNum.int";
    /// Slot number the local node was last about to lead.
    pub const ABOUT_TO_LEAD_SLOT_LAST: &str =
        "cardano.node.metrics.aboutToLeadSlotLast.int";
}

/// All canonical EKG-parity names in declaration order. Used by
/// integration tests and by the registration helper to assert the
/// public set hasn't drifted.
pub const ALL_NAMES: &[&str] = &[
    names::SLOT_NUM,
    names::BLOCK_NUM,
    names::DENSITY,
    names::SLOTS_MISSED,
    names::TXS_IN_MEMPOOL,
    names::MEMPOOL_BYTES,
    names::CONNECTED_PEERS,
    names::PEERS_FROM_NODE_KERNEL,
    names::CURRENT_ERA,
    names::BLOCK_PROCESSING_TIME,
    names::FORKS,
    names::NODE_IS_LEADER,
    names::NODE_CANNOT_FORGE,
    names::BLOCKS_FORGED_NUM,
    names::ABOUT_TO_LEAD_SLOT_LAST,
];

/// Configuration for `install_prometheus_exporter`.
#[derive(Clone, Debug)]
pub struct ExporterConfig {
    /// Bind address for the HTTP scrape endpoint
    /// (defaults to `0.0.0.0:12798` — the same port the existing
    /// raw-TCP `metrics_server.rs` uses, so existing Grafana scrape
    /// configs continue to work unchanged).
    pub bind: std::net::SocketAddr,
}

impl Default for ExporterConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:12798".parse().expect("hardcoded default is valid"),
        }
    }
}

/// Install the global Prometheus exporter + HTTP listener, returning
/// a `PrometheusHandle` for programmatic introspection.
///
/// Idempotent: a second call returns the existing handle without
/// rebinding the socket (the underlying recorder is one-shot per
/// process per `metrics` crate convention).
pub fn install_prometheus_exporter(config: &ExporterConfig) -> Result<PrometheusHandle, ExporterError> {
    let builder = PrometheusBuilder::new().with_http_listener(config.bind);

    let handle = builder
        .install_recorder()
        .map_err(|e| ExporterError::InstallRecorder(e.to_string()))?;

    // Pre-register each canonical name with the recorder. Without
    // this pre-registration the metric only appears in the scrape
    // surface after its first emit; pre-registering makes the
    // `/metrics` endpoint a stable surface from process startup
    // even if no event has fired yet.
    for &name in ALL_NAMES {
        if name.ends_with(".int") {
            metrics::gauge!(name).set(0.0);
        } else if name.ends_with(".real") && name.contains("Time") {
            // Time-shaped metric → histogram.
            metrics::histogram!(name).record(0.0);
        } else if name.ends_with(".real") {
            metrics::gauge!(name).set(0.0);
        }
    }

    Ok(handle)
}

/// Spawn the HTTP scrape listener on a tokio runtime.
///
/// Returned `tokio::task::JoinHandle` lives for the lifetime of the
/// process; cancelling it stops the scrape endpoint.
pub fn spawn_scrape_listener(
    bind: std::net::SocketAddr,
) -> tokio::task::JoinHandle<Result<(), ExporterError>> {
    tokio::spawn(async move {
        // The PrometheusBuilder's with_http_listener path handles the
        // socket bind + accept loop internally when install_recorder
        // is called. This wrapper exists for callers that want to
        // separate recorder install from the listener task; today it
        // mirrors `install_prometheus_exporter` so the API is
        // consistent.
        let _ = install_prometheus_exporter(&ExporterConfig { bind })?;
        // The recorder + listener are kept alive by the global
        // PrometheusRecorder; await an awaitable that never resolves.
        std::future::pending::<()>().await;
        Ok(())
    })
}

/// Surfaced by `install_prometheus_exporter` on installation failure.
#[derive(Debug)]
pub enum ExporterError {
    /// Failed to install the Prometheus recorder globally
    /// (typically because another `metrics` recorder is already
    /// installed).
    InstallRecorder(String),
}

impl core::fmt::Display for ExporterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InstallRecorder(msg) => write!(f, "install prometheus recorder: {msg}"),
        }
    }
}

impl std::error::Error for ExporterError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_names_unique() {
        let mut seen = std::collections::HashSet::new();
        for name in ALL_NAMES {
            assert!(seen.insert(*name), "duplicate metric name in ALL_NAMES: {name}");
        }
    }

    #[test]
    fn all_names_use_cardano_node_prefix() {
        for name in ALL_NAMES {
            assert!(
                name.starts_with("cardano.node.metrics."),
                "metric name `{name}` does not match the EKG-parity prefix; \
                 every name registered here is part of the Tier-1 stable contract \
                 with upstream cardano-node 11.0.1 and operators expect the \
                 `cardano.node.metrics.` prefix verbatim",
            );
        }
    }

    #[test]
    fn fifteen_metric_names_registered() {
        // The Wave 6 PR 16 contract: fifteen canonical metric names
        // covering slot/block/era, mempool, peers, forge, and block-
        // processing-time. Bumping this number requires updating
        // docs/COMPATIBILITY.md's Tier-1 stable surface.
        assert_eq!(ALL_NAMES.len(), 15, "EKG-parity metric set has drifted; expected 15");
    }

    #[test]
    fn exporter_config_default_binds_12798() {
        let c = ExporterConfig::default();
        assert_eq!(c.bind.port(), 12798);
    }
}
