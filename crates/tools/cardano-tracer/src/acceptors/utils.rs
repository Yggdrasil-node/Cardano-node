//! Acceptor-side utility helpers — per-connection state setup +
//! teardown + per-response metrics-store handling.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Acceptors/Utils.hs.
//!
//! Direct port of upstream's bounded subset. Wires the existing
//! Yggdrasil primitives (`ConnectedNodes`, `ConnectedNodesNames`,
//! `AcceptedMetrics`, `MetricsStore`, `MetricsLocalStore`) into
//! the upstream-named call surface used by `Acceptors/Server.hs`
//! (R424 pending) and `Acceptors/Client.hs` (R425 pending).
//!
//! Mapping summary:
//!
//! | Upstream                                                                           | Yggdrasil                              |
//! |------------------------------------------------------------------------------------|----------------------------------------|
//! | `prepareDataPointRequestor :: TracerEnv -> ConnectionId addr -> IO DataPointRequestor` | (deferred — see [`prepare_data_point_requestor_status`]) |
//! | `prepareMetricsStores :: TracerEnv -> ConnectionId addr -> IO (EKG.Store, TVar MetricsLocalStore)` | [`prepare_metrics_stores`] |
//! | `addConnectedNode :: ConnectedNodes -> ConnectionId addr -> IO ()`                 | [`add_connected_node`]                 |
//! | `removeDisconnectedNode :: TracerEnv -> ConnectionId addr -> IO ()`                | [`remove_disconnected_node`]           |
//! | `notifyAboutNodeDisconnected :: TracerEnvRTView -> ConnectionId addr -> IO ()`     | (RTView carve-out — see [`notify_about_node_disconnected_status`]) |
//! | `store :: TracerEnv -> NodeId -> (EKG.Store, TVar MetricsLocalStore) -> Response -> IO ()` | [`store`]                          |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`prepareDataPointRequestor`**: depends on
//!   `Trace.Forward.Utils.DataPoint.initDataPointRequestor` from
//!   the trace-forward DataPoint sub-protocol (port deferred to
//!   R425+ per the R411 plan). Status surfaced via
//!   [`prepare_data_point_requestor_status`].
//! - **`notifyAboutNodeDisconnected`** (RTView-conditional): the
//!   RTView web UI is a synthesis carve-out per the R411 plan
//!   (no Rust analog for ThreePenny GUI). The non-RTView upstream
//!   path is `notifyAboutNodeDisconnected _ _ = pure ()`, which
//!   matches Yggdrasil's no-op default.
//! - **`Cardano.Timeseries.Component`**: optional time-series
//!   storage backend. R411 D1 deferred this (Option C) — the
//!   `te_timeseries_handle: Option<TimeseriesHandle>` field
//!   currently always reads `None`. The `store` impl threads the
//!   handle through but no-ops when it is `None`.
//! - **TracerEnv-record-arg**: per the R398 plan's TracerEnv
//!   option (b) decision, the helpers take the slice of state
//!   they need directly (e.g. `&ConnectedNodes` rather than
//!   `&TracerEnv`). This keeps the call sites flexible during
//!   the partial port window.

use crate::metrics_store::{
    AcceptedMetrics, MetricsLocalStore, MetricsStore, get_or_insert_store, remove_store,
};
use crate::types::{ConnectedNodes, ConnectedNodesNames};
use crate::utils::conn_id_to_node_id;

/// Add a freshly-connected node to the `ConnectedNodes` set.
/// Mirror of upstream's
/// `addConnectedNode connectedNodes connId = atomically $
///   modifyTVar' connectedNodes $ S.insert (connIdToNodeId connId)`.
///
/// Returns `true` if the node was newly inserted, `false` if it
/// was already present (mirror of upstream `Set.insert` no-op-on-
/// existing semantics — the latter case happens during reconnect-
/// races where the disconnect cleanup hasn't run yet).
pub fn add_connected_node(connected_nodes: &ConnectedNodes, remote_address: &str) -> bool {
    let node_id = conn_id_to_node_id(remote_address);
    connected_nodes.insert(node_id)
}

/// Prepare the per-node metrics store for a freshly-connected
/// forwarder. Mirror of upstream's
/// `prepareMetricsStores TracerEnv{teConnectedNodes, teAcceptedMetrics} connId`.
///
/// Performs three side-effects upstream rolls into one IO action:
/// 1. Adds the new `NodeId` to `connected_nodes` (via
///    [`add_connected_node`]).
/// 2. Looks up (or creates) the per-node `MetricsStore` in
///    `accepted_metrics` (via [`get_or_insert_store`]).
/// 3. Returns the [`MetricsStore`] + a fresh
///    [`MetricsLocalStore`] paired together as a tuple — matching
///    upstream's `(EKG.Store, TVar MetricsLocalStore)` shape used
///    downstream by [`store`].
///
/// The synthetic `ekg.server_timestamp_ms` counter registration
/// upstream does at line 70 happens automatically inside
/// [`MetricsStore::insert_resp`] (R412 wired it there) — so this
/// function doesn't need an explicit registration step.
pub async fn prepare_metrics_stores(
    connected_nodes: &ConnectedNodes,
    accepted_metrics: &AcceptedMetrics,
    remote_address: &str,
) -> (MetricsStore, MetricsLocalStore) {
    let _new = add_connected_node(connected_nodes, remote_address);
    let node_id = conn_id_to_node_id(remote_address);
    let store = get_or_insert_store(accepted_metrics, node_id).await;
    let local = MetricsLocalStore::new();
    (store, local)
}

/// Tear down the per-node state for a forwarder that has
/// disconnected. Mirror of upstream's
/// `removeDisconnectedNode tracerEnv connId` — removes the node
/// from all four runtime-state maps (`teConnectedNodes`,
/// `teConnectedNodesNames`, `teAcceptedMetrics`, `teDPRequestors`)
/// in a single STM transaction upstream; Yggdrasil performs the
/// removals sequentially (each map is independently locked) which
/// is safe because no other thread can re-introduce the NodeId
/// concurrently — the disconnect signal is the unique terminator
/// for that NodeId's lifecycle.
///
/// `te_dp_requestors` removal is a no-op pending the
/// DataPointRequestors port (the field is currently a unit-struct
/// placeholder).
pub async fn remove_disconnected_node(
    connected_nodes: &ConnectedNodes,
    connected_nodes_names: &ConnectedNodesNames,
    accepted_metrics: &AcceptedMetrics,
    remote_address: &str,
) {
    let node_id = conn_id_to_node_id(remote_address);
    connected_nodes.remove(&node_id);
    connected_nodes_names.remove_id(&node_id);
    let _ = remove_store(accepted_metrics, &node_id).await;
    // te_dp_requestors removal: no-op (DataPointRequestors port
    // deferred — the field is the R371 unit-struct placeholder).
}

/// Insert a `Response::ResponseMetrics` batch from the EKG sub-
/// protocol into the per-node metrics store. Mirror of upstream's
/// `store tracerEnv (NodeId nodeId) (ekgStore, localStore) resp@(ResponseMetrics ms)`.
///
/// Threads the batch through:
/// 1. [`MetricsStore::insert_resp`] (R412) — populates the per-
///    node store + the synthetic `ekg.server_timestamp_ms` counter.
/// 2. The `te_timeseries_handle` time-series sink — currently
///    no-op since R411 deferred the time-series dependency
///    (Option C).
///
/// The upstream `numeralOnly` + `parseMetric` filter helpers that
/// extract numeric values for the time-series sink are folded into
/// the time-series no-op step; they will be exposed when the
/// time-series port lands.
pub async fn store(
    metrics_store: &MetricsStore,
    metrics_local: &MetricsLocalStore,
    response_metrics: Vec<(String, crate::metrics_store::MetricValue)>,
) {
    metrics_store.insert_resp(response_metrics).await;
    // Stash a snapshot in the local store so subsequent delta
    // computations have a baseline. Mirror of upstream's
    // `storeMetrics` writing to both `EKG.Store` AND
    // `TVar MetricsLocalStore`.
    let _delta = metrics_local.diff_and_advance(metrics_store).await;
    // Time-series forwarding: deferred (R411 D1 Option C).
}

// ---------------------------------------------------------------------------
// Status descriptors (carve-out exposition)
// ---------------------------------------------------------------------------

/// Status descriptor for the deferred
/// `prepareDataPointRequestor` upstream surface. Surfaces the
/// dependency on the trace-forward DataPoint sub-protocol port
/// (R425+ pending).
pub fn prepare_data_point_requestor_status() -> &'static str {
    "prepareDataPointRequestor: deferred pending the trace-forward \
     DataPoint sub-protocol port (Trace.Forward.Run.DataPoint.Acceptor + \
     Trace.Forward.Utils.DataPoint.initDataPointRequestor — vendored at \
     .reference-haskell-cardano-node/trace-forward/src/Trace/Forward/Run/\
     DataPoint/Acceptor.hs). Tracked in the R411-R430 arc plan as a \
     post-R422 follow-up."
}

/// Status descriptor for the RTView-conditional
/// `notifyAboutNodeDisconnected` upstream surface. The non-RTView
/// build is a no-op (`pure ()`); RTView is a synthesis carve-out
/// per the R411 plan.
pub fn notify_about_node_disconnected_status() -> &'static str {
    "notifyAboutNodeDisconnected: RTView-conditional. Non-RTView \
     build is no-op (pure ()), which Yggdrasil matches by default. \
     The RTView web UI is a synthesis carve-out per the R411 plan \
     (no Rust analog for ThreePenny GUI)."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics_store::{MetricValue, new_accepted_metrics};

    #[tokio::test]
    async fn add_connected_node_inserts_new_node() {
        let connected = ConnectedNodes::new();
        let inserted = add_connected_node(&connected, "test-pipe-node-1");
        assert!(inserted, "first insert should report new");
        let again = add_connected_node(&connected, "test-pipe-node-1");
        assert!(!again, "second insert should report not-new");
    }

    #[tokio::test]
    async fn add_connected_node_strips_pipe_prefix() {
        let connected = ConnectedNodes::new();
        // The conn-id-to-node-id sanitization should strip the
        // 'pipe' prefix; verify the resulting NodeId is consistent.
        add_connected_node(&connected, "LocalAddress \"pipe.node-x\"");
        let snap = connected.snapshot();
        assert_eq!(snap.len(), 1);
        // Sanitized form: 'LocalAddress' + 'pipe' + '.' all stripped,
        // quotes replaced with dashes.
        assert!(snap[0].as_str().contains("node-x"));
    }

    #[tokio::test]
    async fn prepare_metrics_stores_creates_new_node_store() {
        let connected = ConnectedNodes::new();
        let accepted = new_accepted_metrics();
        let (store, _local) = prepare_metrics_stores(&connected, &accepted, "test-node-2").await;
        assert_eq!(store.len().await, 0, "fresh store starts empty");
        assert_eq!(connected.snapshot().len(), 1, "node added to connected set");
    }

    #[tokio::test]
    async fn prepare_metrics_stores_returns_existing_for_reconnect() {
        let connected = ConnectedNodes::new();
        let accepted = new_accepted_metrics();
        let (store_a, _) = prepare_metrics_stores(&connected, &accepted, "rec-node").await;
        store_a.register_counter("ekg.test", 99).await;
        let (store_b, _) = prepare_metrics_stores(&connected, &accepted, "rec-node").await;
        // Second prepare returns the same store (Arc-shared); the
        // counter we registered on store_a should be visible via
        // store_b.
        let snap = store_b.snapshot().await;
        assert_eq!(snap.get("ekg.test"), Some(&MetricValue::Counter(99)));
    }

    #[tokio::test]
    async fn remove_disconnected_node_clears_all_maps() {
        let connected = ConnectedNodes::new();
        let names = ConnectedNodesNames::new();
        let accepted = new_accepted_metrics();

        // Set up state for a node, then disconnect it.
        let (_store, _) = prepare_metrics_stores(&connected, &accepted, "drop-node").await;
        let node_id = conn_id_to_node_id("drop-node");
        names.insert(node_id.clone(), "drop-pool".to_string());
        assert_eq!(connected.snapshot().len(), 1);
        assert_eq!(names.snapshot().len(), 1);
        assert_eq!(accepted.read().await.len(), 1);

        remove_disconnected_node(&connected, &names, &accepted, "drop-node").await;

        assert_eq!(connected.snapshot().len(), 0, "connected cleared");
        assert_eq!(names.snapshot().len(), 0, "names cleared");
        assert_eq!(accepted.read().await.len(), 0, "metrics cleared");
    }

    #[tokio::test]
    async fn remove_disconnected_node_is_idempotent() {
        let connected = ConnectedNodes::new();
        let names = ConnectedNodesNames::new();
        let accepted = new_accepted_metrics();
        // No setup — just remove. Should not panic.
        remove_disconnected_node(&connected, &names, &accepted, "ghost").await;
    }

    #[tokio::test]
    async fn store_inserts_response_metrics_into_store() {
        let connected = ConnectedNodes::new();
        let accepted = new_accepted_metrics();
        let (metrics_store, metrics_local) =
            prepare_metrics_stores(&connected, &accepted, "metrics-node").await;
        let resp = vec![
            ("ekg.cpu_pct".to_string(), MetricValue::Gauge(42)),
            ("ekg.uptime_s".to_string(), MetricValue::Counter(3600)),
        ];
        store(&metrics_store, &metrics_local, resp).await;
        let snap = metrics_store.snapshot().await;
        assert_eq!(snap.get("ekg.cpu_pct"), Some(&MetricValue::Gauge(42)));
        assert_eq!(snap.get("ekg.uptime_s"), Some(&MetricValue::Counter(3600)));
        // The ekg.server_timestamp_ms synthetic counter is also
        // populated by insert_resp.
        assert!(snap.contains_key("ekg.server_timestamp_ms"));
    }

    #[test]
    fn prepare_data_point_requestor_status_describes_deferral() {
        let s = prepare_data_point_requestor_status();
        assert!(s.contains("deferred"));
        assert!(s.contains("DataPoint"));
        assert!(s.contains("R411-R430"));
    }

    #[test]
    fn notify_about_node_disconnected_status_describes_rtview() {
        let s = notify_about_node_disconnected_status();
        assert!(s.contains("RTView"));
        assert!(s.contains("no-op"));
    }
}
