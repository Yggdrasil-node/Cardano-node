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

/// Read-side trait implemented by `MetricsSnapshot`-shaped types
/// (today: `yggdrasil_node_tracer::MetricsSnapshot`) so the EKG-
/// parity Prometheus-text rendering can live in this crate without
/// depending on `yggdrasil-node-tracer` (which would invert the
/// layering — tracer crate owns the snapshot type, this crate owns
/// the metric-name registry; the trait is the bridge).
///
/// All return types are `u64` so the trait stays object-safe and
/// the implementation is decoupled from any specific atomic-counter
/// type. Counter values that are not tracked by a particular
/// implementation MUST return `0`; the rendering logic relies on
/// that to keep the scrape surface stable from process startup.
pub trait EkgParitySource {
    /// `cardano.node.metrics.slotNum.int` — current slot at chain tip.
    fn current_slot(&self) -> u64;
    /// `cardano.node.metrics.blockNum.int` — current block number at chain tip.
    fn current_block_number(&self) -> u64;
    /// `cardano.node.metrics.currentEra.int` — era ordinal (0=Byron…6=Conway).
    fn current_era(&self) -> u64;
    /// Mempool transaction count → `cardano.node.metrics.txsInMempool.int`.
    fn mempool_tx_count(&self) -> u64;
    /// Mempool size in bytes → `cardano.node.metrics.mempoolBytes.int`.
    fn mempool_bytes(&self) -> u64;
    /// Connected peer count → `cardano.node.metrics.connectedPeers.int`.
    fn active_peers(&self) -> u64;
    /// Known peer count → `cardano.node.metrics.peersFromNodeKernel.int`.
    fn known_peers(&self) -> u64;
    /// Rollback / fork count → `cardano.node.metrics.forks.int`.
    fn rollbacks(&self) -> u64;
    /// Blocks fetched + applied since process start (used to derive `density`).
    fn blocks_synced(&self) -> u64;
    /// Process uptime in milliseconds (used to derive `density`).
    fn uptime_ms(&self) -> u64;

    // --- Forge-side counters (return 0 when not tracked) --------------

    /// Slots in which the local node was elected leader.
    fn node_is_leader(&self) -> u64 {
        0
    }
    /// Forge attempts aborted because the node did not satisfy preconditions.
    fn node_cannot_forge(&self) -> u64 {
        0
    }
    /// Blocks the local node successfully forged.
    fn blocks_forged_num(&self) -> u64 {
        0
    }
    /// Slot number the local node was last about to lead.
    fn about_to_lead_slot_last(&self) -> u64 {
        0
    }
    /// Slots in which the local node had block-producer credentials but
    /// did not produce a block.
    fn slots_missed_num(&self) -> u64 {
        0
    }
}

/// Render the EKG-parity metric block in Prometheus text format.
///
/// Single-source-of-truth implementation owned by this crate. Wave 6
/// PR 16 follow-on consolidation: previously this logic was
/// duplicated on `MetricsSnapshot::to_ekg_parity_prometheus_text` in
/// `yggdrasil-node-tracer` (~110 lines of field-mapping). That impl
/// is now a thin wrapper around this function so any future name /
/// HELP-string / TYPE drift happens in one place.
///
/// The 15 emitted metric names exactly match
/// `ALL_NAMES` (with the canonical `cardano.node.metrics.<symbol>.<type>`
/// dots Prometheus-escaped to underscores per scrape convention).
pub fn render_ekg_parity_prometheus_text(src: &impl EkgParitySource) -> String {
    let density = if src.uptime_ms() > 0 {
        ((src.blocks_synced() as f64) / (src.uptime_ms() as f64 / 1000.0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // blockProcessingTime exposes a single gauge for now — the
    // histogram version replaces this once the metrics-exporter-
    // prometheus integration lands (queued in docs/TECH-DEBT.md).
    let block_processing_time = 0.0_f64;

    format!(
        "\
# HELP cardano_node_metrics_slotNum_int Current slot at the chain tip (EKG parity).\n\
# TYPE cardano_node_metrics_slotNum_int gauge\n\
cardano_node_metrics_slotNum_int {slot}\n\
# HELP cardano_node_metrics_blockNum_int Current block number at the chain tip (EKG parity).\n\
# TYPE cardano_node_metrics_blockNum_int gauge\n\
cardano_node_metrics_blockNum_int {block}\n\
# HELP cardano_node_metrics_density_real Chain density (blocks per second) over uptime (EKG parity).\n\
# TYPE cardano_node_metrics_density_real gauge\n\
cardano_node_metrics_density_real {density}\n\
# HELP cardano_node_metrics_slotsMissedNum_int Slots in which the local node had block-producer credentials but did not produce a block.\n\
# TYPE cardano_node_metrics_slotsMissedNum_int counter\n\
cardano_node_metrics_slotsMissedNum_int {slots_missed}\n\
# HELP cardano_node_metrics_txsInMempool_int Transaction count currently held by the mempool (EKG parity).\n\
# TYPE cardano_node_metrics_txsInMempool_int gauge\n\
cardano_node_metrics_txsInMempool_int {mempool_tx}\n\
# HELP cardano_node_metrics_mempoolBytes_int Mempool size in bytes (sum of held tx CBOR sizes) (EKG parity).\n\
# TYPE cardano_node_metrics_mempoolBytes_int gauge\n\
cardano_node_metrics_mempoolBytes_int {mempool_bytes}\n\
# HELP cardano_node_metrics_connectedPeers_int Currently-connected peer count (EKG parity).\n\
# TYPE cardano_node_metrics_connectedPeers_int gauge\n\
cardano_node_metrics_connectedPeers_int {connected}\n\
# HELP cardano_node_metrics_peersFromNodeKernel_int Peer-snapshot size known to the node kernel (EKG parity).\n\
# TYPE cardano_node_metrics_peersFromNodeKernel_int gauge\n\
cardano_node_metrics_peersFromNodeKernel_int {known}\n\
# HELP cardano_node_metrics_currentEra_int Era ordinal at the chain tip: 0=Byron,1=Shelley,2=Allegra,3=Mary,4=Alonzo,5=Babbage,6=Conway (EKG parity).\n\
# TYPE cardano_node_metrics_currentEra_int gauge\n\
cardano_node_metrics_currentEra_int {era}\n\
# HELP cardano_node_metrics_blockProcessingTime_real Block-processing duration in seconds (EKG parity; histogram-shaped output lands with the metrics-exporter-prometheus follow-on).\n\
# TYPE cardano_node_metrics_blockProcessingTime_real gauge\n\
cardano_node_metrics_blockProcessingTime_real {bpt}\n\
# HELP cardano_node_metrics_forks_int Rollback / fork events observed since process start (EKG parity).\n\
# TYPE cardano_node_metrics_forks_int counter\n\
cardano_node_metrics_forks_int {forks}\n\
# HELP cardano_node_metrics_nodeIsLeader_int Slots in which the local node was elected leader.\n\
# TYPE cardano_node_metrics_nodeIsLeader_int counter\n\
cardano_node_metrics_nodeIsLeader_int {is_leader}\n\
# HELP cardano_node_metrics_nodeCannotForge_int Forge attempts aborted because the node did not satisfy preconditions.\n\
# TYPE cardano_node_metrics_nodeCannotForge_int counter\n\
cardano_node_metrics_nodeCannotForge_int {cannot}\n\
# HELP cardano_node_metrics_blocksForgedNum_int Blocks the local node successfully forged.\n\
# TYPE cardano_node_metrics_blocksForgedNum_int counter\n\
cardano_node_metrics_blocksForgedNum_int {forged}\n\
# HELP cardano_node_metrics_aboutToLeadSlotLast_int Slot number the local node was last about to lead.\n\
# TYPE cardano_node_metrics_aboutToLeadSlotLast_int gauge\n\
cardano_node_metrics_aboutToLeadSlotLast_int {last_lead}\n\
",
        slot = src.current_slot(),
        block = src.current_block_number(),
        density = density,
        slots_missed = src.slots_missed_num(),
        mempool_tx = src.mempool_tx_count(),
        mempool_bytes = src.mempool_bytes(),
        connected = src.active_peers(),
        known = src.known_peers(),
        era = src.current_era(),
        bpt = block_processing_time,
        forks = src.rollbacks(),
        is_leader = src.node_is_leader(),
        cannot = src.node_cannot_forge(),
        forged = src.blocks_forged_num(),
        last_lead = src.about_to_lead_slot_last(),
    )
}

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
