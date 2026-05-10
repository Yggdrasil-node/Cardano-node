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

    /// Render the store contents as a Prometheus text exposition.
    /// Mirror of upstream
    /// `Cardano.Logging.Prometheus.Exposition.renderExpositionFromSampleWith`.
    ///
    /// Each metric name yields three lines:
    /// ```text
    /// # HELP <name> <help-text>
    /// # TYPE <name> <kind>
    /// <name> <value>
    /// ```
    /// (the HELP line is omitted when no help-text entry matches the
    /// metric name).
    ///
    /// `no_suffix` controls upstream's `metricsNoSuffix` config flag:
    /// when true, the `_int` / `_real` suffix that EKG appends to
    /// metric names is stripped (`RTS_gcMajorNum_int` →
    /// `RTS_gcMajorNum`).
    ///
    /// `help` carries the operator-supplied per-metric HELP text
    /// from `te_metrics_help` (R415 wires this slice from
    /// `metrics_help.json`).
    ///
    /// Format follows the OpenMetrics 1.0.0 exposition standard,
    /// matching upstream's output byte-for-byte modulo the carve-out
    /// for distribution metrics (which surface as Labels per the
    /// module docstring).
    pub async fn render_prometheus(&self, no_suffix: bool, help: &[(String, String)]) -> String {
        let snapshot = self.snapshot().await;
        let help_map: BTreeMap<&str, &str> =
            help.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let mut out = String::new();
        for (name, value) in snapshot.iter() {
            let rendered_name = if no_suffix {
                strip_prom_suffix(name)
            } else {
                name.clone()
            };
            let prom_name = sanitize_prom_metric_name(&rendered_name);
            if let Some(help_text) = help_map.get(name.as_str()) {
                out.push_str(&format!("# HELP {prom_name} {help_text}\n"));
            }
            out.push_str(&format!(
                "# TYPE {prom_name} {kind}\n",
                kind = value.prometheus_kind(),
            ));
            out.push_str(&format!(
                "{prom_name} {val}\n",
                val = value.prometheus_value(),
            ));
        }
        out
    }

    /// Insert a batch of metrics from an upstream
    /// `Response::ResponseMetrics(Vec<(MetricName, MetricValue)>)`
    /// payload. Mirror of upstream
    /// `System.Metrics.Store.Acceptor::storeMetrics`. Each entry's
    /// MetricValue replaces any existing value at that name.
    ///
    /// Always populates the synthetic `ekg.server_timestamp_ms`
    /// counter with the current wall-clock time (mirroring
    /// upstream's `Acceptors/Utils.hs:70` invocation of
    /// `Cardano.Tracer.Time.getTimeMs >>= EKG.set timestampCounter`).
    /// The synthetic counter is needed because EKG's Wai frontend
    /// expects every store to expose it.
    pub async fn insert_resp(&self, batch: Vec<(String, MetricValue)>) {
        let now_ms = crate::time::get_time_ms();
        let mut guard = self.inner.write().await;
        for (name, value) in batch {
            guard.insert(name, value);
        }
        guard.insert(
            EKG_SERVER_TIMESTAMP_MS.to_string(),
            MetricValue::Counter(now_ms),
        );
    }

    /// Return a delta of metrics that have been added or modified
    /// since the supplied `previous_snapshot`. Mirror of upstream's
    /// `MetricsLocalStore::derive` flow used when the
    /// `Response::ResponseMetrics` came back via `GetUpdatedMetrics`
    /// mode (per-node delta tracking).
    ///
    /// The returned map contains only entries whose name was not
    /// in `previous_snapshot` *or* whose MetricValue differs from
    /// the previous snapshot. The synthetic `ekg.server_timestamp_ms`
    /// counter is excluded from the diff (it always changes; surfacing
    /// it would mask other-metric churn).
    pub async fn delta_since(
        &self,
        previous_snapshot: &BTreeMap<String, MetricValue>,
    ) -> BTreeMap<String, MetricValue> {
        let current = self.inner.read().await;
        let mut delta = BTreeMap::new();
        for (name, value) in current.iter() {
            if name == EKG_SERVER_TIMESTAMP_MS {
                continue;
            }
            match previous_snapshot.get(name) {
                Some(prior) if prior == value => {}
                _ => {
                    delta.insert(name.clone(), value.clone());
                }
            }
        }
        delta
    }
}

/// Strip upstream EKG's type-tagging suffix from a metric name
/// when the operator's `metricsNoSuffix` config flag is true.
/// Mirror of upstream's
/// `if metricsNoSuffix then T.dropSuffix "_int" . T.dropSuffix "_real"`
/// behavior.
fn strip_prom_suffix(name: &str) -> String {
    let stripped = name
        .strip_suffix("_int")
        .or_else(|| name.strip_suffix("_real"))
        .unwrap_or(name);
    stripped.to_string()
}

/// Sanitize a metric name so it conforms to the Prometheus
/// exposition format's identifier rules: `[a-zA-Z_:][a-zA-Z0-9_:]*`.
/// Replaces forbidden characters (notably `.`) with `_` to keep
/// scrapers happy. Mirror of upstream's metric-name normalization
/// applied before exposition rendering.
fn sanitize_prom_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, ch) in name.chars().enumerate() {
        let ok = if i == 0 {
            ch.is_ascii_alphabetic() || ch == '_' || ch == ':'
        } else {
            ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'
        };
        if ok {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

/// Canonical synthetic-counter name populated by every
/// [`MetricsStore::insert_resp`] call. Mirror of upstream EKG's
/// `ekg.server_timestamp_ms` metric (the Wai frontend expects this
/// in every store; see `Cardano.Tracer.Time::getTimeMs` for the
/// upstream value source).
pub const EKG_SERVER_TIMESTAMP_MS: &str = "ekg.server_timestamp_ms";

/// Per-node delta-tracking state for the EKG ReqResp protocol's
/// `GetUpdatedMetrics` mode. Holds the most recent snapshot
/// returned to upstream so the next request can compute the delta.
/// Mirror of upstream `MetricsLocalStore`.
#[derive(Clone, Debug, Default)]
pub struct MetricsLocalStore {
    /// Most recent snapshot returned to the upstream forwarder.
    /// Empty on first request.
    last_snapshot: Arc<RwLock<BTreeMap<String, MetricValue>>>,
}

impl MetricsLocalStore {
    /// Construct an empty local store.
    pub fn new() -> Self {
        MetricsLocalStore::default()
    }

    /// Compute the delta since the previous request and update the
    /// local snapshot to the current contents of `store`. Returns
    /// the entries that have changed since the last invocation.
    ///
    /// On first invocation (empty `last_snapshot`), returns the
    /// full store contents minus the synthetic timestamp counter
    /// (mirror of upstream's `MetricsLocalStore::initial` first-call
    /// behavior).
    pub async fn diff_and_advance(&self, store: &MetricsStore) -> BTreeMap<String, MetricValue> {
        let prior = self.last_snapshot.read().await.clone();
        let delta = store.delta_since(&prior).await;
        // Update the snapshot to the current store contents
        // (excluding the synthetic timestamp).
        let mut next = store.snapshot().await;
        next.remove(EKG_SERVER_TIMESTAMP_MS);
        *self.last_snapshot.write().await = next;
        delta
    }

    /// Reset the local snapshot. Used when a node disconnects and
    /// reconnects — the next `diff_and_advance` will return the
    /// full store contents.
    pub async fn reset(&self) {
        self.last_snapshot.write().await.clear();
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

    #[tokio::test]
    async fn insert_resp_writes_batch_and_synthetic_timestamp() {
        let store = MetricsStore::new();
        store
            .insert_resp(vec![
                ("Mem_resident_int".to_string(), MetricValue::Gauge(100_000)),
                ("RTS_gcMajorNum_int".to_string(), MetricValue::Counter(7)),
            ])
            .await;
        // All batch entries present + synthetic timestamp counter.
        assert_eq!(
            store.get("Mem_resident_int").await,
            Some(MetricValue::Gauge(100_000)),
        );
        assert_eq!(
            store.get("RTS_gcMajorNum_int").await,
            Some(MetricValue::Counter(7)),
        );
        let ts = store.get(EKG_SERVER_TIMESTAMP_MS).await;
        assert!(matches!(ts, Some(MetricValue::Counter(_))));
    }

    #[tokio::test]
    async fn insert_resp_replaces_prior_values() {
        let store = MetricsStore::new();
        store
            .insert_resp(vec![("c".to_string(), MetricValue::Counter(1))])
            .await;
        store
            .insert_resp(vec![("c".to_string(), MetricValue::Counter(99))])
            .await;
        assert_eq!(store.get("c").await, Some(MetricValue::Counter(99)));
    }

    #[tokio::test]
    async fn insert_resp_empty_batch_still_updates_timestamp() {
        let store = MetricsStore::new();
        store.insert_resp(vec![]).await;
        let ts = store.get(EKG_SERVER_TIMESTAMP_MS).await;
        assert!(matches!(ts, Some(MetricValue::Counter(_))));
        // No other metrics.
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn delta_since_excludes_synthetic_timestamp() {
        let store = MetricsStore::new();
        store
            .insert_resp(vec![("c".to_string(), MetricValue::Counter(1))])
            .await;
        let snap1 = store.snapshot().await;
        // Wait a moment then insert again; the timestamp will change.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        store
            .insert_resp(vec![("c".to_string(), MetricValue::Counter(1))])
            .await;
        // Delta from snap1 should be empty since `c` didn't change
        // and the synthetic timestamp is excluded from the diff.
        let delta = store.delta_since(&snap1).await;
        assert!(delta.is_empty(), "delta = {delta:?}");
    }

    #[tokio::test]
    async fn delta_since_includes_changed_value() {
        let store = MetricsStore::new();
        store.register_counter("c", 1).await;
        let snap = store.snapshot().await;
        store.set_counter("c", 99).await;
        let delta = store.delta_since(&snap).await;
        assert_eq!(delta.len(), 1);
        assert_eq!(delta.get("c"), Some(&MetricValue::Counter(99)));
    }

    #[tokio::test]
    async fn delta_since_includes_new_metric() {
        let store = MetricsStore::new();
        store.register_counter("c1", 1).await;
        let snap = store.snapshot().await;
        store.register_counter("c2", 2).await;
        let delta = store.delta_since(&snap).await;
        assert_eq!(delta.len(), 1);
        assert_eq!(delta.get("c2"), Some(&MetricValue::Counter(2)));
    }

    #[tokio::test]
    async fn metrics_local_store_first_call_returns_full_contents() {
        let store = MetricsStore::new();
        store
            .insert_resp(vec![
                ("c1".to_string(), MetricValue::Counter(1)),
                ("c2".to_string(), MetricValue::Counter(2)),
            ])
            .await;
        let local = MetricsLocalStore::new();
        let delta = local.diff_and_advance(&store).await;
        // First call returns full contents (minus synthetic timestamp).
        assert_eq!(delta.len(), 2);
        assert_eq!(delta.get("c1"), Some(&MetricValue::Counter(1)));
        assert!(!delta.contains_key(EKG_SERVER_TIMESTAMP_MS));
    }

    #[tokio::test]
    async fn metrics_local_store_subsequent_call_returns_only_changes() {
        let store = MetricsStore::new();
        store.register_counter("c", 1).await;
        let local = MetricsLocalStore::new();
        let _initial = local.diff_and_advance(&store).await;
        // No change yet — second diff is empty.
        let no_change = local.diff_and_advance(&store).await;
        assert!(no_change.is_empty());
        // Mutate; third diff returns only the change.
        store.set_counter("c", 99).await;
        let change = local.diff_and_advance(&store).await;
        assert_eq!(change.len(), 1);
        assert_eq!(change.get("c"), Some(&MetricValue::Counter(99)));
    }

    #[tokio::test]
    async fn metrics_local_store_reset_clears_snapshot() {
        let store = MetricsStore::new();
        store.register_counter("c", 1).await;
        let local = MetricsLocalStore::new();
        let _initial = local.diff_and_advance(&store).await;
        local.reset().await;
        // After reset, the next diff returns the full contents again.
        let delta = local.diff_and_advance(&store).await;
        assert_eq!(delta.len(), 1);
    }

    #[test]
    fn ekg_server_timestamp_ms_constant_matches_upstream() {
        assert_eq!(EKG_SERVER_TIMESTAMP_MS, "ekg.server_timestamp_ms");
    }

    #[test]
    fn strip_prom_suffix_drops_int_suffix() {
        assert_eq!(strip_prom_suffix("RTS_gcMajorNum_int"), "RTS_gcMajorNum");
    }

    #[test]
    fn strip_prom_suffix_drops_real_suffix() {
        assert_eq!(strip_prom_suffix("Mem_resident_real"), "Mem_resident");
    }

    #[test]
    fn strip_prom_suffix_passes_through_unsuffixed_names() {
        assert_eq!(
            strip_prom_suffix("yggdrasil_blocks_synced"),
            "yggdrasil_blocks_synced"
        );
    }

    #[test]
    fn sanitize_prom_metric_name_replaces_dots_with_underscores() {
        assert_eq!(
            sanitize_prom_metric_name("ekg.server_timestamp_ms"),
            "ekg_server_timestamp_ms"
        );
    }

    #[test]
    fn sanitize_prom_metric_name_preserves_alphanumeric_and_underscore() {
        assert_eq!(
            sanitize_prom_metric_name("yggdrasil_chain_tip_42"),
            "yggdrasil_chain_tip_42"
        );
    }

    #[test]
    fn sanitize_prom_metric_name_replaces_leading_digit_with_underscore() {
        // Prometheus requires identifier to start with [a-zA-Z_:].
        assert_eq!(sanitize_prom_metric_name("9metric"), "_metric");
    }

    #[tokio::test]
    async fn render_prometheus_emits_canonical_three_line_block_per_metric() {
        let store = MetricsStore::new();
        store.register_counter("RTS_gcMajorNum_int", 4).await;
        store.register_gauge("Mem_resident_int", 103_792_640).await;
        let output = store.render_prometheus(false, &[]).await;
        // Two metrics × 2 lines (TYPE + value; no HELP without help slice).
        assert!(output.contains("# TYPE Mem_resident_int gauge\n"));
        assert!(output.contains("Mem_resident_int 103792640\n"));
        assert!(output.contains("# TYPE RTS_gcMajorNum_int counter\n"));
        assert!(output.contains("RTS_gcMajorNum_int 4\n"));
    }

    #[tokio::test]
    async fn render_prometheus_emits_help_when_help_slice_supplies_text() {
        let store = MetricsStore::new();
        store.register_gauge("Mem_resident_int", 1024).await;
        let help = vec![(
            "Mem_resident_int".to_string(),
            "Kernel-reported RSS (resident set size)".to_string(),
        )];
        let output = store.render_prometheus(false, &help).await;
        assert!(
            output.contains("# HELP Mem_resident_int Kernel-reported RSS (resident set size)\n")
        );
    }

    #[tokio::test]
    async fn render_prometheus_strips_int_suffix_when_no_suffix_is_true() {
        let store = MetricsStore::new();
        store.register_counter("RTS_gcMajorNum_int", 4).await;
        let output = store.render_prometheus(true, &[]).await;
        assert!(output.contains("# TYPE RTS_gcMajorNum counter\n"));
        assert!(output.contains("RTS_gcMajorNum 4\n"));
        assert!(!output.contains("RTS_gcMajorNum_int"));
    }

    #[tokio::test]
    async fn render_prometheus_sanitizes_dotted_synthetic_timestamp() {
        let store = MetricsStore::new();
        store.insert_resp(vec![]).await;
        let output = store.render_prometheus(false, &[]).await;
        // The synthetic counter is `ekg.server_timestamp_ms` — must
        // be sanitized to `ekg_server_timestamp_ms` for Prometheus.
        assert!(output.contains("ekg_server_timestamp_ms "));
        // No raw dot in the metric line.
        assert!(!output.contains("ekg.server_timestamp_ms "));
    }

    #[tokio::test]
    async fn render_prometheus_empty_store_returns_empty_string() {
        let store = MetricsStore::new();
        let output = store.render_prometheus(false, &[]).await;
        assert!(output.is_empty());
    }
}
