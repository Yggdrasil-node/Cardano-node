//! Per-node metrics store — passive aggregator for metrics
//! delivered by the trace-forwarder EKG mini-protocol.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none.
//!
//! Yggdrasil-side synthesis stand-in for upstream
//! `System.Metrics.Store` (from the unvendored Hackage `ekg-core`
//! package). The cardano-tracer is a **passive** aggregator — it
//! stores whatever metric names the forwarding mini-protocol
//! delivers from upstream nodes. The store is therefore
//! schema-flexible: a `BTreeMap<String, MetricValue>` keyed by
//! whatever name the upstream node chose (`Mem_resident_int`,
//! `RTS_gcMajorNum_int`, `yggdrasil_chain_tip`, etc.).
//!
//! The MetricValue variant set is recovered from upstream's
//! `System.Metrics.ReqResp.MetricValue` typed-message surface as
//! consumed by `Acceptors/Utils.hs::store` — Counter (monotonic),
//! Gauge (point-in-time), Label (free-form text). When the
//! upstream `ekg-core` package is eventually vendored, this file
//! retires in favor of a strict 1:1 port.
//!
//! ## Field set
//!
//! | Upstream field             | Yggdrasil field        | Notes                                                  |
//! |----------------------------|------------------------|--------------------------------------------------------|
//! | `Counter Int64`            | `MetricValue::Counter(i64)` | Monotonically-increasing 64-bit signed counter.    |
//! | `Gauge Int64`              | `MetricValue::Gauge(i64)`   | Point-in-time 64-bit signed gauge.                  |
//! | `Label Text`               | `MetricValue::Label(String)`| Free-form label/text metric.                        |
//! | `Distribution Distribution`| (deferred)             | Distribution histograms — wait for ekg-core vendor.    |
//! | `Store` HashMap            | `Arc<RwLock<BTreeMap<String, MetricValue>>>` | per-node store value type for [`crate::environment::AcceptedMetrics`] map. |
//!
//! ## Carve-outs (NOT ported, by design)
//!
//! - **`System.Metrics.Distribution`**: upstream's distribution
//!   metric type carries quantile estimation state. Yggdrasil's
//!   port defers this until `ekg-core` is vendored — current
//!   ResponseMetrics protocol traffic that contains Distribution
//!   variants will surface a synthetic Label entry recording the
//!   metric name + "Distribution metric not yet rendered".
//! - **`System.Metrics.Store.sampleAll`**: upstream's store
//!   returns a `Sample` (frozen snapshot map). Yggdrasil's port
//!   uses [`MetricsStore::snapshot`] which returns a cloned map
//!   directly — same semantics, simpler shape.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::types::NodeId;

/// One metric-value variant carried by the upstream
/// `System.Metrics.ReqResp.Response::ResponseMetrics` payload.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum MetricValue {
    /// Monotonically-increasing 64-bit signed counter. Mirror of
    /// upstream `Counter Int64`.
    Counter(i64),
    /// Point-in-time 64-bit signed gauge. Mirror of upstream
    /// `Gauge Int64`.
    Gauge(i64),
    /// Free-form label/text metric. Mirror of upstream `Label Text`.
    Label(String),
}

impl MetricValue {
    /// Human-readable kind name — used by [`MetricsStore::render_prometheus`]
    /// to emit `# TYPE <name> counter`/`# TYPE <name> gauge` lines.
    /// Labels render as gauges with value 0 since Prometheus has no
    /// native string-metric type (the actual string is conveyed via
    /// the `# HELP` line and the metric label itself).
    pub fn prometheus_kind(&self) -> &'static str {
        match self {
            MetricValue::Counter(_) => "counter",
            MetricValue::Gauge(_) => "gauge",
            MetricValue::Label(_) => "gauge",
        }
    }

    /// Render the value as the right-hand-side of a Prometheus
    /// exposition line. Labels render as 0 (their string content
    /// goes into the metric's namespace).
    pub fn prometheus_value(&self) -> i64 {
        match self {
            MetricValue::Counter(v) => *v,
            MetricValue::Gauge(v) => *v,
            MetricValue::Label(_) => 0,
        }
    }
}

/// Per-node metrics aggregator. Constructed once per connected
/// node by `Acceptors/Utils.hs::prepareMetricsStores` (R422
/// pending); populated by `Acceptors/Utils.hs::store` from incoming
/// `Response::ResponseMetrics` payloads.
#[derive(Clone, Debug, Default)]
pub struct MetricsStore {
    inner: Arc<RwLock<BTreeMap<String, MetricValue>>>,
}

impl MetricsStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        MetricsStore::default()
    }

    /// Register a counter at `name`. If the entry already exists,
    /// replaces its value (mirroring upstream's
    /// `EKG.createCounter` + `EKG.set` flow, which is also
    /// idempotent on the name). Returns the previous value if any.
    pub async fn register_counter(
        &self,
        name: impl Into<String>,
        value: i64,
    ) -> Option<MetricValue> {
        self.inner
            .write()
            .await
            .insert(name.into(), MetricValue::Counter(value))
    }

    /// Register a gauge at `name`. Same idempotency semantics as
    /// [`MetricsStore::register_counter`].
    pub async fn register_gauge(&self, name: impl Into<String>, value: i64) -> Option<MetricValue> {
        self.inner
            .write()
            .await
            .insert(name.into(), MetricValue::Gauge(value))
    }

    /// Register a label at `name`.
    pub async fn register_label(
        &self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Option<MetricValue> {
        self.inner
            .write()
            .await
            .insert(name.into(), MetricValue::Label(value.into()))
    }

    /// Set a counter's value (no-op if the entry isn't a counter).
    /// Mirror of upstream `EKG.set`.
    pub async fn set_counter(&self, name: &str, value: i64) -> bool {
        let mut guard = self.inner.write().await;
        if let Some(MetricValue::Counter(v)) = guard.get_mut(name) {
            *v = value;
            true
        } else {
            false
        }
    }

    /// Set a gauge's value (no-op if the entry isn't a gauge).
    pub async fn set_gauge(&self, name: &str, value: i64) -> bool {
        let mut guard = self.inner.write().await;
        if let Some(MetricValue::Gauge(v)) = guard.get_mut(name) {
            *v = value;
            true
        } else {
            false
        }
    }

    /// Look up a metric by name. Returns `None` when no such name
    /// is registered.
    pub async fn get(&self, name: &str) -> Option<MetricValue> {
        self.inner.read().await.get(name).cloned()
    }

    /// Snapshot the current store contents. Mirror of upstream
    /// `System.Metrics.Store.sampleAll`.
    pub async fn snapshot(&self) -> BTreeMap<String, MetricValue> {
        self.inner.read().await.clone()
    }

    /// Number of registered metrics.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    /// `true` when the store has no registered metrics.
    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

/// Per-node `MetricsStore` registry. Mirror of upstream
/// `type AcceptedMetrics = TVar (Map NodeId (TVar EKG.Store))`.
///
/// This replaces the unit-struct placeholder
/// [`crate::environment::AcceptedMetrics`] from R393 — the field
/// type in `TracerEnv::te_accepted_metrics` upgrades to this real
/// shape at R411 land.
pub type AcceptedMetrics = Arc<RwLock<BTreeMap<NodeId, MetricsStore>>>;

/// Construct an empty [`AcceptedMetrics`] registry.
pub fn new_accepted_metrics() -> AcceptedMetrics {
    Arc::new(RwLock::new(BTreeMap::new()))
}

/// Look up (or create) the per-node store for a given `NodeId`.
/// Mirror of upstream `Acceptors/Utils.hs::prepareMetricsStores`'s
/// per-node store-allocation step.
pub async fn get_or_insert_store(accepted: &AcceptedMetrics, node_id: NodeId) -> MetricsStore {
    {
        let guard = accepted.read().await;
        if let Some(store) = guard.get(&node_id) {
            return store.clone();
        }
    }
    let mut guard = accepted.write().await;
    guard
        .entry(node_id)
        .or_insert_with(MetricsStore::new)
        .clone()
}

/// Remove the per-node store for a `NodeId` (called when a node
/// disconnects). Mirror of upstream
/// `Acceptors/Utils.hs::removeDisconnectedNode`'s metrics-store
/// cleanup step.
pub async fn remove_store(accepted: &AcceptedMetrics, node_id: &NodeId) -> Option<MetricsStore> {
    accepted.write().await.remove(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn metrics_store_default_is_empty() {
        let store = MetricsStore::new();
        assert!(store.is_empty().await);
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn register_counter_inserts_then_replaces() {
        let store = MetricsStore::new();
        let prior = store.register_counter("RTS_gcMajorNum_int", 0).await;
        assert!(prior.is_none());
        let again = store.register_counter("RTS_gcMajorNum_int", 5).await;
        assert_eq!(again, Some(MetricValue::Counter(0)));
        assert_eq!(
            store.get("RTS_gcMajorNum_int").await,
            Some(MetricValue::Counter(5))
        );
    }

    #[tokio::test]
    async fn register_gauge_round_trips() {
        let store = MetricsStore::new();
        store.register_gauge("Mem_resident_int", 103_792_640).await;
        assert_eq!(
            store.get("Mem_resident_int").await,
            Some(MetricValue::Gauge(103_792_640)),
        );
    }

    #[tokio::test]
    async fn register_label_round_trips() {
        let store = MetricsStore::new();
        store.register_label("nodeName", "alpha-pool").await;
        assert_eq!(
            store.get("nodeName").await,
            Some(MetricValue::Label("alpha-pool".to_string())),
        );
    }

    #[tokio::test]
    async fn set_counter_updates_existing() {
        let store = MetricsStore::new();
        store.register_counter("c", 0).await;
        let updated = store.set_counter("c", 42).await;
        assert!(updated);
        assert_eq!(store.get("c").await, Some(MetricValue::Counter(42)));
    }

    #[tokio::test]
    async fn set_counter_returns_false_when_not_a_counter() {
        let store = MetricsStore::new();
        store.register_gauge("g", 0).await;
        let updated = store.set_counter("g", 42).await;
        assert!(!updated);
        assert_eq!(store.get("g").await, Some(MetricValue::Gauge(0)));
    }

    #[tokio::test]
    async fn set_counter_returns_false_when_missing() {
        let store = MetricsStore::new();
        let updated = store.set_counter("missing", 42).await;
        assert!(!updated);
    }

    #[tokio::test]
    async fn set_gauge_updates_existing() {
        let store = MetricsStore::new();
        store.register_gauge("g", 100).await;
        let updated = store.set_gauge("g", 200).await;
        assert!(updated);
        assert_eq!(store.get("g").await, Some(MetricValue::Gauge(200)));
    }

    #[tokio::test]
    async fn snapshot_clones_full_map() {
        let store = MetricsStore::new();
        store.register_counter("c1", 1).await;
        store.register_counter("c2", 2).await;
        store.register_gauge("g1", 3).await;
        let snapshot = store.snapshot().await;
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot["c1"], MetricValue::Counter(1));
        assert_eq!(snapshot["c2"], MetricValue::Counter(2));
        assert_eq!(snapshot["g1"], MetricValue::Gauge(3));
    }

    #[test]
    fn metric_value_prometheus_kind_matches_variant() {
        assert_eq!(MetricValue::Counter(0).prometheus_kind(), "counter");
        assert_eq!(MetricValue::Gauge(0).prometheus_kind(), "gauge");
        assert_eq!(
            MetricValue::Label("x".to_string()).prometheus_kind(),
            "gauge",
        );
    }

    #[test]
    fn metric_value_prometheus_value_returns_i64_for_each_kind() {
        assert_eq!(MetricValue::Counter(42).prometheus_value(), 42);
        assert_eq!(MetricValue::Gauge(-7).prometheus_value(), -7);
        // Labels render as 0 since their content goes into the
        // metric name's namespace, not the value column.
        assert_eq!(MetricValue::Label("x".to_string()).prometheus_value(), 0);
    }

    #[tokio::test]
    async fn new_accepted_metrics_starts_empty() {
        let accepted = new_accepted_metrics();
        assert!(accepted.read().await.is_empty());
    }

    #[tokio::test]
    async fn get_or_insert_store_creates_then_reuses() {
        let accepted = new_accepted_metrics();
        let id = NodeId::new("n1");
        let s1 = get_or_insert_store(&accepted, id.clone()).await;
        let s2 = get_or_insert_store(&accepted, id.clone()).await;
        // Same node id → same store; verified by mutating one and
        // observing the other (Arc shares the underlying RwLock).
        s1.register_counter("c", 42).await;
        assert_eq!(s2.get("c").await, Some(MetricValue::Counter(42)));
    }

    #[tokio::test]
    async fn get_or_insert_store_separates_per_node() {
        let accepted = new_accepted_metrics();
        let s1 = get_or_insert_store(&accepted, NodeId::new("n1")).await;
        let s2 = get_or_insert_store(&accepted, NodeId::new("n2")).await;
        s1.register_counter("c", 1).await;
        s2.register_counter("c", 99).await;
        assert_eq!(s1.get("c").await, Some(MetricValue::Counter(1)));
        assert_eq!(s2.get("c").await, Some(MetricValue::Counter(99)));
    }

    #[tokio::test]
    async fn remove_store_returns_then_drops_node() {
        let accepted = new_accepted_metrics();
        let id = NodeId::new("transient");
        let _ = get_or_insert_store(&accepted, id.clone()).await;
        assert_eq!(accepted.read().await.len(), 1);
        let removed = remove_store(&accepted, &id).await;
        assert!(removed.is_some());
        assert_eq!(accepted.read().await.len(), 0);
    }

    #[tokio::test]
    async fn remove_store_returns_none_for_unknown_node() {
        let accepted = new_accepted_metrics();
        let removed = remove_store(&accepted, &NodeId::new("missing")).await;
        assert!(removed.is_none());
    }
}
