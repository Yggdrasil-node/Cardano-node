//! Thin node-side tracing helpers aligned with the Cardano trace dispatcher
//! vocabulary.
//!
//! Yggdrasil emits local trace objects to stdout in human (plain or
//! ANSI-coloured), machine (JSON), or both formats, based on the configured
//! `TraceOptions` backends.  `EKGBackend` backend strings are silently
//! accepted (metrics flow through [`NodeMetrics`]) and `Forwarder` is
//! recognized for forward-compatibility with cardano-tracer socket transport.
//!
//! Each namespace may carry a `detail` level (`DMinimal`, `DNormal`,
//! `DDetailed`, `DMaximum`) matching upstream `DetailLevel`.  Callsites can
//! query the resolved detail via [`NodeTracer::detail_for`] and conditionally
//! include extra data fields.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::config::{NodeConfigFile, TraceNamespaceConfig};
use crate::trace_forwarder::TraceForwarder;

/// Trace output backend corresponding to upstream scribe/backend strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TraceBackend {
    /// `"Stdout HumanFormatUncoloured"` or `"Stdout HumanFormat"`.
    StdoutHuman,
    StdoutHumanColoured,
    StdoutMachine,
    /// `"Forwarder"` — send trace events as CBOR to a Unix socket.
    Forwarder,
}

/// Upstream `DetailLevel` controlling trace object verbosity per namespace.
///
/// Matches `Cardano.Logging.Types.DetailLevel` in trace-dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TraceDetail {
    /// Minimal output — only key identification fields.
    DMinimal,
    /// Normal output — standard operational fields (upstream default).
    DNormal,
    /// Detailed output — additional diagnostic fields.
    DDetailed,
    /// Maximum output — all available fields.
    DMaximum,
}

impl TraceDetail {
    /// Parse from the upstream-style label string.
    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim() {
            "DMinimal" => Some(Self::DMinimal),
            "DNormal" => Some(Self::DNormal),
            "DDetailed" => Some(Self::DDetailed),
            "DMaximum" => Some(Self::DMaximum),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum TraceSeverity {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
    Silence,
}

impl TraceSeverity {
    fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "debug" => Some(Self::Debug),
            "info" | "informational" => Some(Self::Info),
            "notice" => Some(Self::Notice),
            "warning" | "warn" => Some(Self::Warning),
            "error" => Some(Self::Error),
            "critical" | "crit" => Some(Self::Critical),
            "alert" => Some(Self::Alert),
            "emergency" | "emerg" | "fatal" => Some(Self::Emergency),
            "silence" | "silent" | "off" => Some(Self::Silence),
            _ => None,
        }
    }

    fn level(self) -> u8 {
        match self {
            Self::Debug => 10,
            Self::Info => 20,
            Self::Notice => 30,
            Self::Warning => 40,
            Self::Error => 50,
            Self::Critical => 60,
            Self::Alert => 70,
            Self::Emergency => 80,
            Self::Silence => 255,
        }
    }

    /// ANSI escape code prefix for coloured terminal output.
    fn ansi_colour(self) -> &'static str {
        match self {
            Self::Debug => "\x1b[2m",           // dim
            Self::Info => "",                   // default
            Self::Notice => "\x1b[36m",         // cyan
            Self::Warning => "\x1b[33m",        // yellow
            Self::Error => "\x1b[31m",          // red
            Self::Critical => "\x1b[1;31m",     // bold red
            Self::Alert => "\x1b[1;35m",        // bold magenta
            Self::Emergency => "\x1b[1;41;37m", // bold white on red
            Self::Silence => "",
        }
    }

    const ANSI_RESET: &'static str = "\x1b[0m";
}

#[derive(Serialize)]
struct MachineTraceLine<'a> {
    at_ms: u128,
    namespace: &'a str,
    severity: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_name: Option<&'a str>,
    message: &'a str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    data: &'a BTreeMap<String, Value>,
}

/// Lightweight runtime tracer derived from [`NodeConfigFile`] tracing fields.

#[derive(Debug)]
pub struct NodeTracer {
    turn_on_logging: bool,
    use_trace_dispatcher: bool,
    trace_option_node_name: Option<String>,
    trace_options: BTreeMap<String, TraceNamespaceConfig>,
    last_emit_ms: Arc<Mutex<BTreeMap<String, u128>>>,
    forwarder: Option<Arc<TraceForwarder>>,
}

impl Clone for NodeTracer {
    fn clone(&self) -> Self {
        Self {
            turn_on_logging: self.turn_on_logging,
            use_trace_dispatcher: self.use_trace_dispatcher,
            trace_option_node_name: self.trace_option_node_name.clone(),
            trace_options: self.trace_options.clone(),
            // Share emit-rate state across clones so `max_frequency` limits
            // remain global when runtime tasks hold cloned tracers.
            last_emit_ms: Arc::clone(&self.last_emit_ms),
            // Preserve the forwarder transport across runtime task clones so
            // all spawned loops keep emitting Forwarder backend events.
            forwarder: self.forwarder.clone(),
        }
    }
}

impl NodeTracer {
    /// Build a tracer from the effective node configuration.
    pub fn from_config(config: &NodeConfigFile) -> Self {
        let forwarder = if config
            .trace_options
            .values()
            .any(|cfg| cfg.backends.iter().any(|b| b == "Forwarder"))
        {
            Some(Arc::new(TraceForwarder::new(
                config.trace_option_forwarder.socket_path.clone(),
            )))
        } else {
            None
        };
        Self {
            turn_on_logging: config.turn_on_logging,
            use_trace_dispatcher: config.use_trace_dispatcher,
            trace_option_node_name: config.trace_option_node_name.clone(),
            trace_options: config.trace_options.clone(),
            last_emit_ms: Arc::new(Mutex::new(BTreeMap::new())),
            forwarder,
        }
    }

    /// Return a disabled tracer that emits no local trace output.
    pub fn disabled() -> Self {
        Self {
            turn_on_logging: false,
            use_trace_dispatcher: false,
            trace_option_node_name: None,
            trace_options: BTreeMap::new(),
            last_emit_ms: Arc::new(Mutex::new(BTreeMap::new())),
            forwarder: None,
        }
    }

    /// Emit a runtime trace event if the current tracing config enables it.
    pub fn trace_runtime(
        &self,
        namespace: &str,
        default_severity: &str,
        message: impl Into<String>,
        data: BTreeMap<String, Value>,
    ) {
        let message = message.into();
        let Some(severity) = self.resolve_severity(namespace, default_severity) else {
            return;
        };

        let now_ms = current_unix_millis();
        if !self.should_emit(namespace, now_ms) {
            return;
        }

        for backend in self.backends_for(namespace) {
            match backend {
                TraceBackend::StdoutHuman => {
                    println!(
                        "{}",
                        self.format_human_line(namespace, severity, &message, &data, false)
                    );
                }
                TraceBackend::StdoutHumanColoured => {
                    println!(
                        "{}",
                        self.format_human_line(namespace, severity, &message, &data, true)
                    );
                }
                TraceBackend::StdoutMachine => {
                    println!(
                        "{}",
                        self.format_machine_line(namespace, severity, &message, &data)
                    );
                }
                TraceBackend::Forwarder => {
                    if let Some(forwarder) = &self.forwarder {
                        let event = serde_json::json!({
                            "namespace": namespace,
                            "severity": severity,
                            "message": message,
                            "data": data,
                            "timestamp": now_ms
                        });
                        forwarder.send(&event);
                    }
                }
            }
        }
    }

    /// Emit a runtime trace event only if the configured detail level for
    /// `namespace` is at least `min_detail`.
    ///
    /// This allows callsites to emit verbose trace events that operators can
    /// enable per-namespace via the `detail` field in `TraceOptions`.
    pub fn trace_runtime_detailed(
        &self,
        namespace: &str,
        default_severity: &str,
        min_detail: TraceDetail,
        message: impl Into<String>,
        data: BTreeMap<String, Value>,
    ) {
        if self.detail_for(namespace) < min_detail {
            return;
        }
        self.trace_runtime(namespace, default_severity, message, data);
    }

    /// Resolve the effective [`TraceDetail`] for a namespace using
    /// longest-prefix matching, falling back to the root config and then
    /// `DNormal` (the upstream default).
    pub fn detail_for(&self, namespace: &str) -> TraceDetail {
        self.namespace_config(namespace)
            .and_then(|cfg| cfg.detail.as_deref().and_then(TraceDetail::from_label))
            .or_else(|| {
                self.trace_options
                    .get("")
                    .and_then(|cfg| cfg.detail.as_deref().and_then(TraceDetail::from_label))
            })
            .unwrap_or(TraceDetail::DNormal)
    }

    fn resolve_severity<'a>(
        &'a self,
        namespace: &str,
        default_severity: &'a str,
    ) -> Option<&'a str> {
        if !(self.turn_on_logging && self.use_trace_dispatcher) {
            return None;
        }

        if matches!(
            TraceSeverity::from_label(default_severity),
            Some(TraceSeverity::Silence)
        ) {
            return None;
        }

        let namespace_severity = self
            .namespace_config(namespace)
            .and_then(|cfg| cfg.severity.as_deref());
        let root_severity = self
            .trace_options
            .get("")
            .and_then(|cfg| cfg.severity.as_deref());
        let configured_threshold = namespace_severity.or(root_severity);

        if let Some(threshold) = configured_threshold {
            if !passes_severity_threshold(default_severity, threshold) {
                return None;
            }
        }

        Some(default_severity)
    }

    fn namespace_config(&self, namespace: &str) -> Option<&TraceNamespaceConfig> {
        let mut best_match: Option<(&TraceNamespaceConfig, usize)> = None;

        for (selector, cfg) in &self.trace_options {
            if selector.is_empty() {
                continue;
            }

            let prefix = selector.trim_end_matches('.');
            if prefix.is_empty() {
                continue;
            }

            let is_match = namespace == prefix
                || (namespace.starts_with(prefix)
                    && namespace.as_bytes().get(prefix.len()) == Some(&b'.'));
            if !is_match {
                continue;
            }

            let candidate_len = prefix.len();
            if best_match.is_none_or(|(_, len)| candidate_len > len) {
                best_match = Some((cfg, candidate_len));
            }
        }

        best_match.map(|(cfg, _)| cfg)
    }

    fn backends_for(&self, namespace: &str) -> Vec<TraceBackend> {
        let configured = self
            .namespace_config(namespace)
            .filter(|cfg| !cfg.backends.is_empty())
            .or_else(|| {
                self.trace_options
                    .get("")
                    .filter(|cfg| !cfg.backends.is_empty())
            });

        configured
            .map(|cfg| {
                cfg.backends
                    .iter()
                    .filter_map(|backend| match backend.as_str() {
                        s if s.starts_with("Stdout HumanFormatColoured") => {
                            Some(TraceBackend::StdoutHumanColoured)
                        }
                        s if s.starts_with("Stdout HumanFormat") => Some(TraceBackend::StdoutHuman),
                        s if s.starts_with("Stdout MachineFormat") => {
                            Some(TraceBackend::StdoutMachine)
                        }
                        "Forwarder" => Some(TraceBackend::Forwarder),
                        // EKGBackend flows through NodeMetrics — no trace-line output.
                        "EKGBackend" => None,
                        // PrometheusSimple recognised — metrics served via /metrics endpoint.
                        s if s.starts_with("PrometheusSimple") => None,
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn should_emit(&self, namespace: &str, now_ms: u128) -> bool {
        let Some(min_interval_ms) = self.min_emit_interval_ms(namespace) else {
            return true;
        };

        let mut last_emit_ms = self
            .last_emit_ms
            .lock()
            .expect("trace limiter mutex should not be poisoned");
        let should_emit = last_emit_ms
            .get(namespace)
            .is_none_or(|last_ms| now_ms.saturating_sub(*last_ms) >= min_interval_ms);

        if should_emit {
            last_emit_ms.insert(namespace.to_owned(), now_ms);
        }

        should_emit
    }

    fn min_emit_interval_ms(&self, namespace: &str) -> Option<u128> {
        let frequency = self
            .namespace_config(namespace)
            .and_then(|cfg| cfg.max_frequency)
            .or_else(|| self.trace_options.get("").and_then(|cfg| cfg.max_frequency));

        frequency.and_then(|hz| {
            if hz.is_finite() && hz > 0.0 {
                Some((1000.0 / hz).ceil() as u128)
            } else {
                None
            }
        })
    }

    fn format_human_line(
        &self,
        namespace: &str,
        severity: &str,
        message: &str,
        data: &BTreeMap<String, Value>,
        coloured: bool,
    ) -> String {
        let (colour_start, colour_end) = if coloured {
            let sev = TraceSeverity::from_label(severity).unwrap_or(TraceSeverity::Info);
            let start = sev.ansi_colour();
            let end = if start.is_empty() {
                ""
            } else {
                TraceSeverity::ANSI_RESET
            };
            (start, end)
        } else {
            ("", "")
        };

        let mut line = format!(
            "{colour_start}[{}] {} {}",
            current_unix_millis(),
            severity,
            namespace
        );

        if let Some(node_name) = self.trace_option_node_name.as_deref() {
            line.push_str(&format!(" node={node_name}"));
        }

        line.push(' ');
        line.push_str(message);

        for (key, value) in data {
            line.push(' ');
            line.push_str(key);
            line.push('=');
            line.push_str(&value_to_human(value));
        }

        line.push_str(colour_end);
        line
    }

    fn format_machine_line(
        &self,
        namespace: &str,
        severity: &str,
        message: &str,
        data: &BTreeMap<String, Value>,
    ) -> String {
        serde_json::to_string(&MachineTraceLine {
            at_ms: current_unix_millis(),
            namespace,
            severity,
            node_name: self.trace_option_node_name.as_deref(),
            message,
            data,
        })
        .expect("trace line serialization should succeed")
    }
}

/// Build a deterministic field map for runtime trace events.
pub fn trace_fields<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis()
}

fn value_to_human(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn passes_severity_threshold(event_severity: &str, threshold: &str) -> bool {
    let Some(threshold) = TraceSeverity::from_label(threshold) else {
        return true;
    };

    if threshold == TraceSeverity::Silence {
        return false;
    }

    let Some(event) = TraceSeverity::from_label(event_severity) else {
        return true;
    };

    if event == TraceSeverity::Silence {
        return false;
    }

    event.level() >= threshold.level()
}

// ---------------------------------------------------------------------------
// Operational metrics
// ---------------------------------------------------------------------------

/// Atomic operational metrics updated by the runtime during sync.
///
/// All counters use relaxed ordering for low-overhead lock-free updates.
/// A [`MetricsSnapshot`] can be read at any time via [`NodeMetrics::snapshot`]
/// for status queries or future metric export endpoints.
#[derive(Debug)]
pub struct NodeMetrics {
    blocks_synced: AtomicU64,
    rollbacks: AtomicU64,
    batches_completed: AtomicU64,
    stable_blocks_promoted: AtomicU64,
    reconnects: AtomicU64,
    current_slot: AtomicU64,
    current_block_number: AtomicU64,
    /// Round 169 — current ledger era as a Prometheus gauge.
    ///
    /// Encoded as the wire era ordinal: `0=Byron, 1=Shelley, 2=Allegra,
    /// 3=Mary, 4=Alonzo, 5=Babbage, 6=Conway`.  Tracks the snapshot's
    /// applied-block era; the PV-aware promotion that cardano-cli sees
    /// (R160) is not reflected here — operators consult this gauge for
    /// raw on-disk era progression.
    ///
    /// Reference: `Cardano.Ledger.Core.Era` ordering.
    current_era: AtomicU64,
    /// Round 170 — per-era applied-block counters.
    ///
    /// Indexed parallel to `Era::era_ordinal()`: `[0]=Byron, [1]=Shelley,
    /// [2]=Allegra, [3]=Mary, [4]=Alonzo, [5]=Babbage, [6]=Conway`.
    /// Combined with R169's `current_era` gauge, dashboards can graph
    /// the share of blocks applied per era during a long sync without
    /// scraping `cardano-cli query tip` history.
    ///
    /// Reference: `Cardano.Ledger.Core.Era` ordering.
    blocks_per_era: [AtomicU64; 7],
    checkpoint_slot: AtomicU64,
    target_known_peers: AtomicU64,
    target_established_peers: AtomicU64,
    target_active_peers: AtomicU64,
    target_known_big_ledger_peers: AtomicU64,
    target_established_big_ledger_peers: AtomicU64,
    target_active_big_ledger_peers: AtomicU64,
    known_peers: AtomicU64,
    established_peers: AtomicU64,
    active_peers: AtomicU64,
    known_big_ledger_peers: AtomicU64,
    established_big_ledger_peers: AtomicU64,
    active_big_ledger_peers: AtomicU64,
    known_local_root_peers: AtomicU64,
    established_local_root_peers: AtomicU64,
    active_local_root_peers: AtomicU64,
    // Mempool gauges (upstream `cardano.node.metrics.txsInMempool` etc.)
    mempool_tx_count: AtomicU64,
    mempool_bytes: AtomicU64,
    mempool_tx_added: AtomicU64,
    mempool_tx_rejected: AtomicU64,
    // Connection manager counters (upstream `ConnectionManagerCounters`)
    cm_full_duplex_conns: AtomicU64,
    cm_duplex_conns: AtomicU64,
    cm_unidirectional_conns: AtomicU64,
    cm_inbound_conns: AtomicU64,
    cm_outbound_conns: AtomicU64,
    // Inbound server counters
    inbound_connections_accepted: AtomicU64,
    inbound_connections_rejected: AtomicU64,
    // NtC (local Unix socket) server counters. Kept distinct from the NtN
    // `inbound_connections_*` pair above because NtC connections are wallet
    // / tooling handshakes driven by a different accept path and a wrong
    // signal here would mask the wrong class of issue (e.g. wallet
    // network-magic mismatch vs. NtN rate-limit rejection).
    ntc_connections_accepted: AtomicU64,
    ntc_connections_rejected: AtomicU64,
    // BlockFetch worker pool gauges (Phase 6 multi-peer dispatch).
    // `blockfetch_workers_registered`: number of per-peer
    // `FetchWorkerHandle` entries currently in the shared
    // `FetchWorkerPool`.  Equals 0 in legacy single-peer mode (knob =
    // 1).  Equal to the number of warm peers when knob > 1 and the
    // governor has migrated their `BlockFetchClient`s to workers.
    // `blockfetch_workers_migrated_total`: lifetime count of
    // promote-time migrations.
    blockfetch_workers_registered: AtomicU64,
    blockfetch_workers_migrated_total: AtomicU64,
    // ChainSync worker pool gauges (Round 151 multi-peer dispatch).
    // `chainsync_workers_registered`: number of per-peer
    // `ChainSyncWorkerHandle` entries currently in the shared
    // `ChainSyncWorkerPool`.  Auto-grown as RollForward observations
    // arrive, so >0 implies candidate-fragment partitioning is feeding
    // real header hashes into BlockFetch dispatch.
    chainsync_workers_registered: AtomicU64,
    /// Round 200 — apply-batch duration histogram.  Bucket
    /// boundaries are [`NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS`];
    /// each observation increments cumulative buckets `<=` the
    /// observed duration.  Pairs with `_sum` (microseconds) and
    /// `_count` to produce the standard Prometheus histogram
    /// rendering in [`MetricsSnapshot::to_prometheus_text`].
    apply_batch_duration_buckets: [AtomicU64; 10],
    apply_batch_duration_sum_micros: AtomicU64,
    apply_batch_duration_count: AtomicU64,
    /// R217 — fetch-batch duration histogram.  Mirrors the R200
    /// apply-batch histogram (same bucket boundaries) so an operator
    /// can compare fetch time vs apply time per batch.  Baseline
    /// observability for Phase C.2 (pipelined fetch+apply): the
    /// gap between `fetch_batch_duration_sum / count` and
    /// `apply_batch_duration_sum / count` quantifies the headroom
    /// available for overlap.  Instrumented around the
    /// `fetch_range_blocks_multi_era_raw_decoded` call site in the
    /// legacy single-peer path (which is the dominant production
    /// code path on mainnet, where multi-peer dispatch is gated
    /// behind `--max-concurrent-block-fetch-peers > 1`).
    fetch_batch_duration_buckets: [AtomicU64; 10],
    fetch_batch_duration_sum_micros: AtomicU64,
    fetch_batch_duration_count: AtomicU64,
    /// R223 — Phase D.2: aggregate of `PeerLifetimeStats.sessions`
    /// across all peers in the governor's `lifetime_stats` map.
    /// Monotonic across reconnects; lets dashboards alert on
    /// peer-churn rate (`rate(yggdrasil_peer_lifetime_sessions_total[5m])`)
    /// distinct from the live `known/active/established_peers`
    /// gauges which reflect the current session count only.
    peer_lifetime_sessions_total: AtomicU64,
    /// R223 — Phase D.2: aggregate of `PeerLifetimeStats.failures_total`
    /// across all peers.  Monotonic; pairs with the sessions counter
    /// to compute peer reliability (`failures_total /
    /// sessions_total`).
    peer_lifetime_failures_total: AtomicU64,
    /// R224 — Phase D.2: aggregate of `PeerLifetimeStats.bytes_in`
    /// across all peers (cumulative bytes received).  Refreshed at
    /// each governor tick from per-peer
    /// `BlockFetchInstrumentation::bytes_delivered`.
    peer_lifetime_bytes_in_total: AtomicU64,
    /// R234 — Phase D.2 bytes-out (initial slice): cumulative
    /// bytes served by the BlockFetch SERVER (yggdrasil-as-peer
    /// egress).  Counterpart to `peer_lifetime_bytes_in_total`
    /// (yggdrasil-as-client ingress).  Aggregate-only (not
    /// per-peer); per-peer attribution requires threading remote
    /// `SocketAddr` through the BlockFetch server run-loop, which
    /// is a larger refactor deferred to a follow-up that also
    /// covers ChainSync and TxSubmission2 egress.
    blockfetch_server_bytes_served_total: AtomicU64,
    /// R226 — Phase D.2: count of distinct peers this node has
    /// ever connected to (cardinality of `lifetime_stats` map).
    /// Distinct from the live `known_peers` gauge which counts the
    /// current peer registry; this is monotonic across restarts
    /// (within the lifetime of a single process; the
    /// `lifetime_stats` map is in-memory and starts empty on each
    /// `yggdrasil-node run` invocation).
    peer_lifetime_unique_peers: AtomicU64,
    /// R226 — Phase D.2: aggregate of
    /// `PeerLifetimeStats.successful_handshakes` across all peers.
    /// Tracks every successful handshake completion; useful for
    /// computing `(handshakes_total / sessions_total) > 1`
    /// scenarios where some sessions don't progress past handshake.
    peer_lifetime_handshakes_total: AtomicU64,
    /// R225 — Phase D.1 first slice: rollback-depth histogram
    /// bucket counters.  Each rollback event observed during sync
    /// increments the bucket whose `le` (rollback depth in blocks)
    /// is `>=` the observed depth.  Pairs with
    /// `_count` total to compute mean rollback depth and tail
    /// percentiles via the standard Prometheus histogram queries.
    /// Lets operators distinguish frequent shallow rollbacks (1-2
    /// blocks, normal chain reorgs) from rare deep cross-epoch
    /// rollbacks (>k blocks, the Phase D.1 problematic case).
    rollback_depth_buckets: [AtomicU64; 7],
    /// R225 — sum of all observed rollback depths.  Standard
    /// Prometheus histogram `_sum` field.
    rollback_depth_sum_blocks: AtomicU64,
    /// R225 — total number of rollback observations.  Standard
    /// Prometheus histogram `_count` field.
    rollback_depth_count: AtomicU64,
    start_time_ms: u128,
}

/// Point-in-time snapshot of runtime metrics.
#[derive(Clone, Debug, Serialize)]
pub struct MetricsSnapshot {
    /// Total blocks fetched and applied during sync.
    pub blocks_synced: u64,
    /// Total rollback events.
    pub rollbacks: u64,
    /// Total sync batches completed.
    pub batches_completed: u64,
    /// Blocks promoted from volatile to immutable storage.
    pub stable_blocks_promoted: u64,
    /// Peer reconnection count.
    pub reconnects: u64,
    /// Latest slot seen by the sync pipeline.
    pub current_slot: u64,
    /// Latest block number seen by the sync pipeline.
    pub current_block_number: u64,
    /// Round 169 — wire era ordinal of the latest applied block
    /// (`0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo, 5=Babbage,
    /// 6=Conway`).
    pub current_era: u64,
    /// Round 170 — per-era applied-block counters, indexed parallel
    /// to `Era::era_ordinal()`.
    pub blocks_per_era: [u64; 7],
    /// Slot of the latest persisted ledger checkpoint.
    pub checkpoint_slot: u64,
    /// Governor target known peers.
    pub target_known_peers: u64,
    /// Governor target established peers.
    pub target_established_peers: u64,
    /// Governor target active peers.
    pub target_active_peers: u64,
    /// Governor target known big-ledger peers.
    pub target_known_big_ledger_peers: u64,
    /// Governor target established big-ledger peers.
    pub target_established_big_ledger_peers: u64,
    /// Governor target active big-ledger peers.
    pub target_active_big_ledger_peers: u64,
    /// Current known non-big-ledger peers in the registry.
    pub known_peers: u64,
    /// Current established non-big-ledger peers in the registry.
    pub established_peers: u64,
    /// Current active non-big-ledger peers in the registry.
    pub active_peers: u64,
    /// Current known big-ledger peers in the registry.
    pub known_big_ledger_peers: u64,
    /// Current established big-ledger peers in the registry.
    pub established_big_ledger_peers: u64,
    /// Current active big-ledger peers in the registry.
    pub active_big_ledger_peers: u64,
    /// Current known local-root peers in the registry.
    pub known_local_root_peers: u64,
    /// Current established local-root peers in the registry.
    pub established_local_root_peers: u64,
    /// Current active local-root peers in the registry.
    pub active_local_root_peers: u64,
    /// Alias of `established_local_root_peers` for backward compatibility.
    pub warm_local_root_peers: u64,
    /// Alias of `active_local_root_peers` for backward compatibility.
    pub hot_local_root_peers: u64,
    /// Current number of transactions in the mempool.
    pub mempool_tx_count: u64,
    /// Approximate total bytes of transactions in the mempool.
    pub mempool_bytes: u64,
    /// Cumulative count of transactions accepted into the mempool.
    pub mempool_tx_added: u64,
    /// Cumulative count of transactions rejected from the mempool.
    pub mempool_tx_rejected: u64,
    /// Connection manager: full-duplex connections.
    pub cm_full_duplex_conns: u64,
    /// Connection manager: duplex connections.
    pub cm_duplex_conns: u64,
    /// Connection manager: unidirectional connections.
    pub cm_unidirectional_conns: u64,
    /// Connection manager: inbound connections.
    pub cm_inbound_conns: u64,
    /// Connection manager: outbound connections.
    pub cm_outbound_conns: u64,
    /// Total inbound connections accepted.
    pub inbound_connections_accepted: u64,
    /// Total inbound connections rejected (rate-limited or CM-refused).
    pub inbound_connections_rejected: u64,
    /// Total NtC (local Unix socket) connections that completed the
    /// handshake and entered protocol dispatch.
    pub ntc_connections_accepted: u64,
    /// Total NtC connections whose handshake failed (e.g. network-magic
    /// mismatch, unsupported protocol version, early disconnect).
    pub ntc_connections_rejected: u64,
    /// Number of per-peer BlockFetch workers currently registered in
    /// the shared `FetchWorkerPool` (Phase 6).  `0` in legacy
    /// single-peer mode; equal to the number of warm peers when
    /// `max_concurrent_block_fetch_peers > 1`.
    pub blockfetch_workers_registered: u64,
    /// Lifetime total of per-peer BlockFetch worker migrations
    /// (each `migrate_session_to_worker` call that takes the
    /// `BlockFetchClient` out of a session and spawns a worker).
    pub blockfetch_workers_migrated_total: u64,
    /// Number of per-peer ChainSync workers currently registered in
    /// the shared `ChainSyncWorkerPool` (Round 151).  Grows
    /// monotonically as RollForward observations arrive from each
    /// peer; 0 implies candidate-fragment partitioning is inactive
    /// and dispatch falls back to placeholder-hash collapse.
    pub chainsync_workers_registered: u64,
    /// Round 200 — apply-batch duration histogram bucket counters.
    /// Each entry is the cumulative count of observations whose
    /// duration was `<=` the corresponding bucket boundary in
    /// [`NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS`].
    pub apply_batch_duration_buckets: [u64; 10],
    /// Round 200 — sum of all observed apply-batch durations
    /// (microseconds).  Rendered as the Prometheus histogram
    /// `_sum` field after dividing by 1e6 for seconds.
    pub apply_batch_duration_sum_micros: u64,
    /// Round 200 — total number of apply-batch duration
    /// observations.  Rendered as the histogram `_count` field.
    pub apply_batch_duration_count: u64,
    /// R217 — fetch-batch duration histogram bucket counters.
    /// Mirrors `apply_batch_duration_buckets`; same bucket
    /// boundaries.  See `NodeMetrics::fetch_batch_duration_buckets`
    /// rustdoc for the Phase C.2 (pipelined fetch+apply)
    /// rationale.
    pub fetch_batch_duration_buckets: [u64; 10],
    /// R217 — sum of all observed fetch-batch durations
    /// (microseconds).
    pub fetch_batch_duration_sum_micros: u64,
    /// R217 — total number of fetch-batch duration observations.
    pub fetch_batch_duration_count: u64,
    /// R223 — Phase D.2: cumulative sessions across all peers in
    /// the governor's `lifetime_stats` map (sum of
    /// `PeerLifetimeStats::sessions`).
    pub peer_lifetime_sessions_total: u64,
    /// R223 — Phase D.2: cumulative session failures across all
    /// peers (sum of `PeerLifetimeStats::failures_total`).
    pub peer_lifetime_failures_total: u64,
    /// R224 — Phase D.2: cumulative bytes received across all
    /// peers (sum of `PeerLifetimeStats::bytes_in`, sourced from
    /// per-peer `BlockFetchInstrumentation::bytes_delivered`).
    pub peer_lifetime_bytes_in_total: u64,
    /// R234 — Phase D.2: cumulative bytes served by the BlockFetch
    /// server (egress; aggregate, not per-peer).
    pub blockfetch_server_bytes_served_total: u64,
    /// R226 — Phase D.2: count of distinct peers ever connected
    /// (cardinality of the governor's `lifetime_stats` map).
    pub peer_lifetime_unique_peers: u64,
    /// R226 — Phase D.2: cumulative successful-handshake count
    /// across all peers (sum of
    /// `PeerLifetimeStats::successful_handshakes`).
    pub peer_lifetime_handshakes_total: u64,
    /// R225 — Phase D.1: rollback-depth histogram bucket counts.
    /// Each entry is the cumulative count of rollback observations
    /// whose depth (in blocks) was `<=` the corresponding bucket
    /// boundary in [`NodeMetrics::ROLLBACK_DEPTH_BUCKETS`].
    pub rollback_depth_buckets: [u64; 7],
    /// R225 — sum of all observed rollback depths in blocks.
    pub rollback_depth_sum_blocks: u64,
    /// R225 — total rollback-depth observations.
    pub rollback_depth_count: u64,
    /// Milliseconds since the metrics tracker was created.
    pub uptime_ms: u128,
}

impl NodeMetrics {
    /// Create a new metrics tracker. Records the creation time for uptime.
    pub fn new() -> Self {
        Self {
            blocks_synced: AtomicU64::new(0),
            rollbacks: AtomicU64::new(0),
            batches_completed: AtomicU64::new(0),
            stable_blocks_promoted: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            current_slot: AtomicU64::new(0),
            current_block_number: AtomicU64::new(0),
            current_era: AtomicU64::new(0),
            blocks_per_era: std::array::from_fn(|_| AtomicU64::new(0)),
            checkpoint_slot: AtomicU64::new(0),
            target_known_peers: AtomicU64::new(0),
            target_established_peers: AtomicU64::new(0),
            target_active_peers: AtomicU64::new(0),
            target_known_big_ledger_peers: AtomicU64::new(0),
            target_established_big_ledger_peers: AtomicU64::new(0),
            target_active_big_ledger_peers: AtomicU64::new(0),
            known_peers: AtomicU64::new(0),
            established_peers: AtomicU64::new(0),
            active_peers: AtomicU64::new(0),
            known_big_ledger_peers: AtomicU64::new(0),
            established_big_ledger_peers: AtomicU64::new(0),
            active_big_ledger_peers: AtomicU64::new(0),
            known_local_root_peers: AtomicU64::new(0),
            established_local_root_peers: AtomicU64::new(0),
            active_local_root_peers: AtomicU64::new(0),
            mempool_tx_count: AtomicU64::new(0),
            mempool_bytes: AtomicU64::new(0),
            mempool_tx_added: AtomicU64::new(0),
            mempool_tx_rejected: AtomicU64::new(0),
            cm_full_duplex_conns: AtomicU64::new(0),
            cm_duplex_conns: AtomicU64::new(0),
            cm_unidirectional_conns: AtomicU64::new(0),
            cm_inbound_conns: AtomicU64::new(0),
            cm_outbound_conns: AtomicU64::new(0),
            inbound_connections_accepted: AtomicU64::new(0),
            inbound_connections_rejected: AtomicU64::new(0),
            ntc_connections_accepted: AtomicU64::new(0),
            ntc_connections_rejected: AtomicU64::new(0),
            blockfetch_workers_registered: AtomicU64::new(0),
            blockfetch_workers_migrated_total: AtomicU64::new(0),
            chainsync_workers_registered: AtomicU64::new(0),
            apply_batch_duration_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            apply_batch_duration_sum_micros: AtomicU64::new(0),
            apply_batch_duration_count: AtomicU64::new(0),
            fetch_batch_duration_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            fetch_batch_duration_sum_micros: AtomicU64::new(0),
            fetch_batch_duration_count: AtomicU64::new(0),
            peer_lifetime_sessions_total: AtomicU64::new(0),
            peer_lifetime_failures_total: AtomicU64::new(0),
            peer_lifetime_bytes_in_total: AtomicU64::new(0),
            blockfetch_server_bytes_served_total: AtomicU64::new(0),
            peer_lifetime_unique_peers: AtomicU64::new(0),
            peer_lifetime_handshakes_total: AtomicU64::new(0),
            rollback_depth_buckets: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
            rollback_depth_sum_blocks: AtomicU64::new(0),
            rollback_depth_count: AtomicU64::new(0),
            start_time_ms: current_unix_millis(),
        }
    }

    /// R225 — Phase D.1 first slice: bucket boundaries (in blocks)
    /// for the rollback-depth histogram.  Covers the spectrum from
    /// shallow chain-reorg rollbacks (1, 2, 5 blocks) through the
    /// stability-window edge (k=2160) to cross-epoch rollbacks
    /// (>10k blocks) and full-resync (`+Inf`).
    pub const ROLLBACK_DEPTH_BUCKETS: [u64; 7] = [1, 2, 5, 50, 2160, 10_000, u64::MAX];

    /// R225 — Phase D.1 first slice: record a rollback observation
    /// of `depth_blocks` blocks into the histogram.  Cumulative
    /// buckets: each observation increments every bucket whose
    /// `le` is ≥ the observed depth.
    pub fn record_rollback_depth(&self, depth_blocks: u64) {
        for (i, le) in Self::ROLLBACK_DEPTH_BUCKETS.iter().enumerate() {
            if depth_blocks <= *le {
                self.rollback_depth_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.rollback_depth_sum_blocks
            .fetch_add(depth_blocks, Ordering::Relaxed);
        self.rollback_depth_count.fetch_add(1, Ordering::Relaxed);
    }

    /// R200 — Bucket boundaries (in seconds) for the apply-batch
    /// duration histogram, ascending.  Mirrors typical Prometheus
    /// HTTP latency bucket conventions and covers ~1ms to ~10s
    /// (with `+Inf` implicit as the final cumulative bucket).
    pub const APPLY_BATCH_BUCKETS_SECONDS: [f64; 10] = [
        0.001,
        0.005,
        0.01,
        0.05,
        0.1,
        0.5,
        1.0,
        5.0,
        10.0,
        f64::INFINITY,
    ];

    /// R200 — Record an apply-batch duration into the histogram.
    /// Cumulative buckets: each observation increments every bucket
    /// whose `le` is ≥ the observed duration.
    pub fn record_apply_batch_duration(&self, duration: std::time::Duration) {
        let secs = duration.as_secs_f64();
        let micros = duration.as_micros() as u64;
        for (i, le) in Self::APPLY_BATCH_BUCKETS_SECONDS.iter().enumerate() {
            if secs <= *le {
                self.apply_batch_duration_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.apply_batch_duration_sum_micros
            .fetch_add(micros, Ordering::Relaxed);
        self.apply_batch_duration_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// R217 — Record a fetch-batch duration into the histogram.
    /// Same cumulative-bucket semantics as the apply histogram.
    /// Reuses [`Self::APPLY_BATCH_BUCKETS_SECONDS`] so operators can
    /// compare fetch vs apply on identical bucket boundaries —
    /// baseline observability for Phase C.2 (pipelined fetch+apply).
    pub fn record_fetch_batch_duration(&self, duration: std::time::Duration) {
        let secs = duration.as_secs_f64();
        let micros = duration.as_micros() as u64;
        for (i, le) in Self::APPLY_BATCH_BUCKETS_SECONDS.iter().enumerate() {
            if secs <= *le {
                self.fetch_batch_duration_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        self.fetch_batch_duration_sum_micros
            .fetch_add(micros, Ordering::Relaxed);
        self.fetch_batch_duration_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Add `n` to the blocks-synced counter.
    pub fn add_blocks_synced(&self, n: u64) {
        self.blocks_synced.fetch_add(n, Ordering::Relaxed);
    }

    /// R223 — Phase D.2: set the cumulative peer-lifetime sessions
    /// counter from the governor's aggregated state.  Caller computes
    /// `sum(peer.sessions for peer in lifetime_stats.values())` and
    /// passes the total here on each governor tick.  Distinct from
    /// the live `known_peers` / `active_peers` gauges which reflect
    /// the current session count only.
    pub fn set_peer_lifetime_sessions_total(&self, total: u64) {
        self.peer_lifetime_sessions_total
            .store(total, Ordering::Relaxed);
    }

    /// R223 — Phase D.2: set the cumulative peer-lifetime failures
    /// counter from the governor's aggregated state.
    pub fn set_peer_lifetime_failures_total(&self, total: u64) {
        self.peer_lifetime_failures_total
            .store(total, Ordering::Relaxed);
    }

    /// R224 — Phase D.2: set the cumulative peer-lifetime bytes-in
    /// counter from the governor's aggregated state.  Caller folds
    /// `peer.bytes_in` across `lifetime_stats.values()` after
    /// refreshing each per-peer entry from the BlockFetch pool's
    /// `bytes_delivered` counter.
    pub fn set_peer_lifetime_bytes_in_total(&self, total: u64) {
        self.peer_lifetime_bytes_in_total
            .store(total, Ordering::Relaxed);
    }

    /// R234 — Phase D.2 bytes-out: add `n` bytes to the BlockFetch
    /// server cumulative bytes-served counter.  Called by
    /// `run_blockfetch_server` after each successful
    /// `serve_batch`.  Aggregate only (not per-peer).
    pub fn add_blockfetch_server_bytes_served(&self, n: u64) {
        self.blockfetch_server_bytes_served_total
            .fetch_add(n, Ordering::Relaxed);
    }

    /// R226 — Phase D.2: set the unique-peer cardinality counter
    /// (`governor_state.lifetime_stats.len()`).
    pub fn set_peer_lifetime_unique_peers(&self, total: u64) {
        self.peer_lifetime_unique_peers
            .store(total, Ordering::Relaxed);
    }

    /// R226 — Phase D.2: set the cumulative successful-handshakes
    /// counter from the governor's aggregated state.
    pub fn set_peer_lifetime_handshakes_total(&self, total: u64) {
        self.peer_lifetime_handshakes_total
            .store(total, Ordering::Relaxed);
    }

    /// Add `n` to the rollback counter.
    pub fn add_rollbacks(&self, n: u64) {
        self.rollbacks.fetch_add(n, Ordering::Relaxed);
    }

    /// Increment the batches-completed counter.
    pub fn inc_batches_completed(&self) {
        self.batches_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Add `n` to the stable-blocks-promoted counter.
    pub fn add_stable_blocks_promoted(&self, n: u64) {
        self.stable_blocks_promoted.fetch_add(n, Ordering::Relaxed);
    }

    /// Increment the reconnection counter.
    pub fn inc_reconnects(&self) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
    }

    /// Update the latest observed slot.
    pub fn set_current_slot(&self, slot: u64) {
        self.current_slot.store(slot, Ordering::Relaxed);
    }

    /// Update the latest observed block number.
    pub fn set_current_block_number(&self, block_number: u64) {
        self.current_block_number
            .store(block_number, Ordering::Relaxed);
    }

    /// Update the latest persisted checkpoint slot.
    pub fn set_checkpoint_slot(&self, slot: u64) {
        self.checkpoint_slot.store(slot, Ordering::Relaxed);
    }

    /// Round 169 — update the wire era ordinal of the latest applied block.
    ///
    /// Encoded as `0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo,
    /// 5=Babbage, 6=Conway` to match `Era::era_ordinal()`.
    pub fn set_current_era(&self, era_ordinal: u64) {
        self.current_era.store(era_ordinal, Ordering::Relaxed);
    }

    /// Round 170 — increment the applied-block count for the given era.
    ///
    /// Out-of-range ordinals (≥ 7) silently no-op so future era additions
    /// upstream don't crash this metric path; the `current_era` gauge
    /// will still reflect the actual era ordinal.
    pub fn add_blocks_for_era(&self, era_ordinal: u8, n: u64) {
        if (era_ordinal as usize) < self.blocks_per_era.len() {
            self.blocks_per_era[era_ordinal as usize].fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Update current governor peer-selection targets and registry counters.
    #[allow(clippy::too_many_arguments)]
    pub fn set_peer_selection_counters(
        &self,
        target_known_peers: u64,
        target_established_peers: u64,
        target_active_peers: u64,
        target_known_big_ledger_peers: u64,
        target_established_big_ledger_peers: u64,
        target_active_big_ledger_peers: u64,
        known_peers: u64,
        established_peers: u64,
        active_peers: u64,
        known_big_ledger_peers: u64,
        established_big_ledger_peers: u64,
        active_big_ledger_peers: u64,
        known_local_root_peers: u64,
        established_local_root_peers: u64,
        active_local_root_peers: u64,
    ) {
        self.target_known_peers
            .store(target_known_peers, Ordering::Relaxed);
        self.target_established_peers
            .store(target_established_peers, Ordering::Relaxed);
        self.target_active_peers
            .store(target_active_peers, Ordering::Relaxed);
        self.target_known_big_ledger_peers
            .store(target_known_big_ledger_peers, Ordering::Relaxed);
        self.target_established_big_ledger_peers
            .store(target_established_big_ledger_peers, Ordering::Relaxed);
        self.target_active_big_ledger_peers
            .store(target_active_big_ledger_peers, Ordering::Relaxed);
        self.known_peers.store(known_peers, Ordering::Relaxed);
        self.established_peers
            .store(established_peers, Ordering::Relaxed);
        self.active_peers.store(active_peers, Ordering::Relaxed);
        self.known_big_ledger_peers
            .store(known_big_ledger_peers, Ordering::Relaxed);
        self.established_big_ledger_peers
            .store(established_big_ledger_peers, Ordering::Relaxed);
        self.active_big_ledger_peers
            .store(active_big_ledger_peers, Ordering::Relaxed);
        self.known_local_root_peers
            .store(known_local_root_peers, Ordering::Relaxed);
        self.established_local_root_peers
            .store(established_local_root_peers, Ordering::Relaxed);
        self.active_local_root_peers
            .store(active_local_root_peers, Ordering::Relaxed);
    }

    /// Update mempool gauges: current count and byte size.
    pub fn set_mempool_gauges(&self, tx_count: u64, bytes: u64) {
        self.mempool_tx_count.store(tx_count, Ordering::Relaxed);
        self.mempool_bytes.store(bytes, Ordering::Relaxed);
    }

    /// Increment the mempool-transactions-added counter.
    pub fn inc_mempool_tx_added(&self) {
        self.mempool_tx_added.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the mempool-transactions-rejected counter.
    pub fn inc_mempool_tx_rejected(&self) {
        self.mempool_tx_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Update connection manager counters.
    pub fn set_connection_manager_counters(
        &self,
        full_duplex: u64,
        duplex: u64,
        unidirectional: u64,
        inbound: u64,
        outbound: u64,
    ) {
        self.cm_full_duplex_conns
            .store(full_duplex, Ordering::Relaxed);
        self.cm_duplex_conns.store(duplex, Ordering::Relaxed);
        self.cm_unidirectional_conns
            .store(unidirectional, Ordering::Relaxed);
        self.cm_inbound_conns.store(inbound, Ordering::Relaxed);
        self.cm_outbound_conns.store(outbound, Ordering::Relaxed);
    }

    /// Increment the inbound-connections-accepted counter.
    pub fn inc_inbound_accepted(&self) {
        self.inbound_connections_accepted
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the inbound-connections-rejected counter.
    pub fn inc_inbound_rejected(&self) {
        self.inbound_connections_rejected
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the NtC-connections-accepted counter.
    pub fn inc_ntc_accepted(&self) {
        self.ntc_connections_accepted
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the NtC-connections-rejected counter (handshake failure).
    pub fn inc_ntc_rejected(&self) {
        self.ntc_connections_rejected
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Set the current count of registered BlockFetch workers in the
    /// shared `FetchWorkerPool`.  Called by the runtime after each
    /// register/unregister so the operator can observe activation in
    /// `/metrics`.  `0` indicates legacy single-peer mode is active.
    pub fn set_blockfetch_workers_registered(&self, count: u64) {
        self.blockfetch_workers_registered
            .store(count, Ordering::Relaxed);
    }

    /// Increment the lifetime BlockFetch worker migration count
    /// (incremented once per successful
    /// `OutboundPeerManager::migrate_session_to_worker` call).
    pub fn inc_blockfetch_workers_migrated(&self) {
        self.blockfetch_workers_migrated_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Set the current count of registered ChainSync workers in the
    /// shared `ChainSyncWorkerPool`.  Called by the runtime tick so
    /// the operator can observe candidate-fragment partitioning in
    /// `/metrics`.  `0` implies dispatch is falling back to
    /// placeholder-hash collapse.
    pub fn set_chainsync_workers_registered(&self, count: u64) {
        self.chainsync_workers_registered
            .store(count, Ordering::Relaxed);
    }

    /// Read a consistent snapshot of all current metric values.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            blocks_synced: self.blocks_synced.load(Ordering::Relaxed),
            rollbacks: self.rollbacks.load(Ordering::Relaxed),
            batches_completed: self.batches_completed.load(Ordering::Relaxed),
            stable_blocks_promoted: self.stable_blocks_promoted.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
            current_slot: self.current_slot.load(Ordering::Relaxed),
            current_block_number: self.current_block_number.load(Ordering::Relaxed),
            current_era: self.current_era.load(Ordering::Relaxed),
            blocks_per_era: std::array::from_fn(|i| self.blocks_per_era[i].load(Ordering::Relaxed)),
            checkpoint_slot: self.checkpoint_slot.load(Ordering::Relaxed),
            target_known_peers: self.target_known_peers.load(Ordering::Relaxed),
            target_established_peers: self.target_established_peers.load(Ordering::Relaxed),
            target_active_peers: self.target_active_peers.load(Ordering::Relaxed),
            target_known_big_ledger_peers: self
                .target_known_big_ledger_peers
                .load(Ordering::Relaxed),
            target_established_big_ledger_peers: self
                .target_established_big_ledger_peers
                .load(Ordering::Relaxed),
            target_active_big_ledger_peers: self
                .target_active_big_ledger_peers
                .load(Ordering::Relaxed),
            known_peers: self.known_peers.load(Ordering::Relaxed),
            established_peers: self.established_peers.load(Ordering::Relaxed),
            active_peers: self.active_peers.load(Ordering::Relaxed),
            known_big_ledger_peers: self.known_big_ledger_peers.load(Ordering::Relaxed),
            established_big_ledger_peers: self.established_big_ledger_peers.load(Ordering::Relaxed),
            active_big_ledger_peers: self.active_big_ledger_peers.load(Ordering::Relaxed),
            known_local_root_peers: self.known_local_root_peers.load(Ordering::Relaxed),
            established_local_root_peers: self.established_local_root_peers.load(Ordering::Relaxed),
            active_local_root_peers: self.active_local_root_peers.load(Ordering::Relaxed),
            warm_local_root_peers: self.established_local_root_peers.load(Ordering::Relaxed),
            hot_local_root_peers: self.active_local_root_peers.load(Ordering::Relaxed),
            mempool_tx_count: self.mempool_tx_count.load(Ordering::Relaxed),
            mempool_bytes: self.mempool_bytes.load(Ordering::Relaxed),
            mempool_tx_added: self.mempool_tx_added.load(Ordering::Relaxed),
            mempool_tx_rejected: self.mempool_tx_rejected.load(Ordering::Relaxed),
            cm_full_duplex_conns: self.cm_full_duplex_conns.load(Ordering::Relaxed),
            cm_duplex_conns: self.cm_duplex_conns.load(Ordering::Relaxed),
            cm_unidirectional_conns: self.cm_unidirectional_conns.load(Ordering::Relaxed),
            cm_inbound_conns: self.cm_inbound_conns.load(Ordering::Relaxed),
            cm_outbound_conns: self.cm_outbound_conns.load(Ordering::Relaxed),
            inbound_connections_accepted: self.inbound_connections_accepted.load(Ordering::Relaxed),
            inbound_connections_rejected: self.inbound_connections_rejected.load(Ordering::Relaxed),
            ntc_connections_accepted: self.ntc_connections_accepted.load(Ordering::Relaxed),
            ntc_connections_rejected: self.ntc_connections_rejected.load(Ordering::Relaxed),
            blockfetch_workers_registered: self
                .blockfetch_workers_registered
                .load(Ordering::Relaxed),
            blockfetch_workers_migrated_total: self
                .blockfetch_workers_migrated_total
                .load(Ordering::Relaxed),
            chainsync_workers_registered: self.chainsync_workers_registered.load(Ordering::Relaxed),
            apply_batch_duration_buckets: {
                let mut buckets = [0u64; 10];
                for (i, b) in self.apply_batch_duration_buckets.iter().enumerate() {
                    buckets[i] = b.load(Ordering::Relaxed);
                }
                buckets
            },
            apply_batch_duration_sum_micros: self
                .apply_batch_duration_sum_micros
                .load(Ordering::Relaxed),
            apply_batch_duration_count: self.apply_batch_duration_count.load(Ordering::Relaxed),
            fetch_batch_duration_buckets: {
                let mut buckets = [0u64; 10];
                for (i, b) in self.fetch_batch_duration_buckets.iter().enumerate() {
                    buckets[i] = b.load(Ordering::Relaxed);
                }
                buckets
            },
            fetch_batch_duration_sum_micros: self
                .fetch_batch_duration_sum_micros
                .load(Ordering::Relaxed),
            fetch_batch_duration_count: self.fetch_batch_duration_count.load(Ordering::Relaxed),
            peer_lifetime_sessions_total: self.peer_lifetime_sessions_total.load(Ordering::Relaxed),
            peer_lifetime_failures_total: self.peer_lifetime_failures_total.load(Ordering::Relaxed),
            peer_lifetime_bytes_in_total: self.peer_lifetime_bytes_in_total.load(Ordering::Relaxed),
            blockfetch_server_bytes_served_total: self
                .blockfetch_server_bytes_served_total
                .load(Ordering::Relaxed),
            peer_lifetime_unique_peers: self.peer_lifetime_unique_peers.load(Ordering::Relaxed),
            peer_lifetime_handshakes_total: self
                .peer_lifetime_handshakes_total
                .load(Ordering::Relaxed),
            rollback_depth_buckets: {
                let mut buckets = [0u64; 7];
                for (i, b) in self.rollback_depth_buckets.iter().enumerate() {
                    buckets[i] = b.load(Ordering::Relaxed);
                }
                buckets
            },
            rollback_depth_sum_blocks: self.rollback_depth_sum_blocks.load(Ordering::Relaxed),
            rollback_depth_count: self.rollback_depth_count.load(Ordering::Relaxed),
            uptime_ms: current_unix_millis().saturating_sub(self.start_time_ms),
        }
    }
}

impl Default for NodeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsSnapshot {
    /// Render the snapshot in Prometheus text exposition format.
    pub fn to_prometheus_text(&self) -> String {
        let mut out = format!(
            "\
# HELP yggdrasil_blocks_synced Total blocks fetched and applied.\n\
# TYPE yggdrasil_blocks_synced counter\n\
yggdrasil_blocks_synced {}\n\
# HELP yggdrasil_rollbacks Total rollback events.\n\
# TYPE yggdrasil_rollbacks counter\n\
yggdrasil_rollbacks {}\n\
# HELP yggdrasil_batches_completed Total sync batches completed.\n\
# TYPE yggdrasil_batches_completed counter\n\
yggdrasil_batches_completed {}\n\
# HELP yggdrasil_stable_blocks_promoted Blocks promoted from volatile to immutable.\n\
# TYPE yggdrasil_stable_blocks_promoted counter\n\
yggdrasil_stable_blocks_promoted {}\n\
# HELP yggdrasil_reconnects Peer reconnection count.\n\
# TYPE yggdrasil_reconnects counter\n\
yggdrasil_reconnects {}\n\
# HELP yggdrasil_current_slot Latest slot seen by the sync pipeline.\n\
# TYPE yggdrasil_current_slot gauge\n\
yggdrasil_current_slot {}\n\
# HELP yggdrasil_current_block_number Latest block number.\n\
# TYPE yggdrasil_current_block_number gauge\n\
yggdrasil_current_block_number {}\n\
# HELP yggdrasil_current_era Wire era ordinal of the latest applied block (0=Byron, 1=Shelley, 2=Allegra, 3=Mary, 4=Alonzo, 5=Babbage, 6=Conway).\n\
# TYPE yggdrasil_current_era gauge\n\
yggdrasil_current_era {}\n\
# HELP yggdrasil_checkpoint_slot Slot of latest persisted ledger checkpoint.\n\
# TYPE yggdrasil_checkpoint_slot gauge\n\
yggdrasil_checkpoint_slot {}\n\
# HELP yggdrasil_target_known_peers Governor target known peers.\n\
# TYPE yggdrasil_target_known_peers gauge\n\
yggdrasil_target_known_peers {}\n\
# HELP yggdrasil_target_established_peers Governor target established peers.\n\
# TYPE yggdrasil_target_established_peers gauge\n\
yggdrasil_target_established_peers {}\n\
# HELP yggdrasil_target_active_peers Governor target active peers.\n\
# TYPE yggdrasil_target_active_peers gauge\n\
yggdrasil_target_active_peers {}\n\
# HELP yggdrasil_target_known_big_ledger_peers Governor target known big-ledger peers.\n\
# TYPE yggdrasil_target_known_big_ledger_peers gauge\n\
yggdrasil_target_known_big_ledger_peers {}\n\
# HELP yggdrasil_target_established_big_ledger_peers Governor target established big-ledger peers.\n\
# TYPE yggdrasil_target_established_big_ledger_peers gauge\n\
yggdrasil_target_established_big_ledger_peers {}\n\
# HELP yggdrasil_target_active_big_ledger_peers Governor target active big-ledger peers.\n\
# TYPE yggdrasil_target_active_big_ledger_peers gauge\n\
yggdrasil_target_active_big_ledger_peers {}\n\
# HELP yggdrasil_known_peers Current known non-big-ledger peers.\n\
# TYPE yggdrasil_known_peers gauge\n\
yggdrasil_known_peers {}\n\
# HELP yggdrasil_established_peers Current established non-big-ledger peers.\n\
# TYPE yggdrasil_established_peers gauge\n\
yggdrasil_established_peers {}\n\
# HELP yggdrasil_active_peers Current active non-big-ledger peers.\n\
# TYPE yggdrasil_active_peers gauge\n\
yggdrasil_active_peers {}\n\
# HELP yggdrasil_known_big_ledger_peers Current known big-ledger peers.\n\
# TYPE yggdrasil_known_big_ledger_peers gauge\n\
yggdrasil_known_big_ledger_peers {}\n\
# HELP yggdrasil_established_big_ledger_peers Current established big-ledger peers.\n\
# TYPE yggdrasil_established_big_ledger_peers gauge\n\
yggdrasil_established_big_ledger_peers {}\n\
# HELP yggdrasil_active_big_ledger_peers Current active big-ledger peers.\n\
# TYPE yggdrasil_active_big_ledger_peers gauge\n\
yggdrasil_active_big_ledger_peers {}\n\
# HELP yggdrasil_known_local_root_peers Current known local-root peers.\n\
# TYPE yggdrasil_known_local_root_peers gauge\n\
yggdrasil_known_local_root_peers {}\n\
# HELP yggdrasil_established_local_root_peers Current established local-root peers.\n\
# TYPE yggdrasil_established_local_root_peers gauge\n\
yggdrasil_established_local_root_peers {}\n\
# HELP yggdrasil_active_local_root_peers Current active local-root peers.\n\
# TYPE yggdrasil_active_local_root_peers gauge\n\
yggdrasil_active_local_root_peers {}\n\
# HELP yggdrasil_warm_local_root_peers Current warm local-root peers.\n\
# TYPE yggdrasil_warm_local_root_peers gauge\n\
yggdrasil_warm_local_root_peers {}\n\
# HELP yggdrasil_hot_local_root_peers Current hot local-root peers.\n\
# TYPE yggdrasil_hot_local_root_peers gauge\n\
yggdrasil_hot_local_root_peers {}\n\
# HELP yggdrasil_uptime_seconds Seconds since node start.\n\
# TYPE yggdrasil_uptime_seconds gauge\n\
yggdrasil_uptime_seconds {}\n\
# HELP yggdrasil_mempool_tx_count Transactions currently in the mempool.\n\
# TYPE yggdrasil_mempool_tx_count gauge\n\
yggdrasil_mempool_tx_count {}\n\
# HELP yggdrasil_mempool_bytes Approximate total bytes of transactions in the mempool.\n\
# TYPE yggdrasil_mempool_bytes gauge\n\
yggdrasil_mempool_bytes {}\n\
# HELP yggdrasil_mempool_tx_added Total transactions accepted into the mempool.\n\
# TYPE yggdrasil_mempool_tx_added counter\n\
yggdrasil_mempool_tx_added {}\n\
# HELP yggdrasil_mempool_tx_rejected Total transactions rejected from the mempool.\n\
# TYPE yggdrasil_mempool_tx_rejected counter\n\
yggdrasil_mempool_tx_rejected {}\n\
# HELP yggdrasil_cm_full_duplex_conns Connection manager full-duplex connections.\n\
# TYPE yggdrasil_cm_full_duplex_conns gauge\n\
yggdrasil_cm_full_duplex_conns {}\n\
# HELP yggdrasil_cm_duplex_conns Connection manager duplex connections.\n\
# TYPE yggdrasil_cm_duplex_conns gauge\n\
yggdrasil_cm_duplex_conns {}\n\
# HELP yggdrasil_cm_unidirectional_conns Connection manager unidirectional connections.\n\
# TYPE yggdrasil_cm_unidirectional_conns gauge\n\
yggdrasil_cm_unidirectional_conns {}\n\
# HELP yggdrasil_cm_inbound_conns Connection manager inbound connections.\n\
# TYPE yggdrasil_cm_inbound_conns gauge\n\
yggdrasil_cm_inbound_conns {}\n\
# HELP yggdrasil_cm_outbound_conns Connection manager outbound connections.\n\
# TYPE yggdrasil_cm_outbound_conns gauge\n\
yggdrasil_cm_outbound_conns {}\n\
# HELP yggdrasil_inbound_connections_accepted Total inbound connections accepted.\n\
# TYPE yggdrasil_inbound_connections_accepted counter\n\
yggdrasil_inbound_connections_accepted {}\n\
# HELP yggdrasil_inbound_connections_rejected Total inbound connections rejected.\n\
# TYPE yggdrasil_inbound_connections_rejected counter\n\
yggdrasil_inbound_connections_rejected {}\n\
# HELP yggdrasil_ntc_connections_accepted Total NtC (local socket) connections that completed the handshake.\n\
# TYPE yggdrasil_ntc_connections_accepted counter\n\
yggdrasil_ntc_connections_accepted {}\n\
# HELP yggdrasil_ntc_connections_rejected Total NtC (local socket) handshake failures (magic mismatch, unsupported version, early disconnect).\n\
# TYPE yggdrasil_ntc_connections_rejected counter\n\
yggdrasil_ntc_connections_rejected {}\n\
# HELP yggdrasil_blockfetch_workers_registered Number of per-peer BlockFetch workers in the shared pool (Phase 6 multi-peer dispatch). 0 in legacy single-peer mode.\n\
# TYPE yggdrasil_blockfetch_workers_registered gauge\n\
yggdrasil_blockfetch_workers_registered {}\n\
# HELP yggdrasil_blockfetch_workers_migrated_total Lifetime BlockFetch worker migrations (per successful promote-time migrate_session_to_worker call).\n\
# TYPE yggdrasil_blockfetch_workers_migrated_total counter\n\
yggdrasil_blockfetch_workers_migrated_total {}\n\
# HELP yggdrasil_chainsync_workers_registered Number of per-peer ChainSync workers in the shared pool (Round 151 candidate-fragment dispatch). 0 implies dispatch falls back to placeholder-hash collapse.\n\
# TYPE yggdrasil_chainsync_workers_registered gauge\n\
yggdrasil_chainsync_workers_registered {}\n\
# HELP yggdrasil_blocks_byron Total Byron-era blocks applied.\n\
# TYPE yggdrasil_blocks_byron counter\n\
yggdrasil_blocks_byron {}\n\
# HELP yggdrasil_blocks_shelley Total Shelley-era blocks applied.\n\
# TYPE yggdrasil_blocks_shelley counter\n\
yggdrasil_blocks_shelley {}\n\
# HELP yggdrasil_blocks_allegra Total Allegra-era blocks applied.\n\
# TYPE yggdrasil_blocks_allegra counter\n\
yggdrasil_blocks_allegra {}\n\
# HELP yggdrasil_blocks_mary Total Mary-era blocks applied.\n\
# TYPE yggdrasil_blocks_mary counter\n\
yggdrasil_blocks_mary {}\n\
# HELP yggdrasil_blocks_alonzo Total Alonzo-era blocks applied.\n\
# TYPE yggdrasil_blocks_alonzo counter\n\
yggdrasil_blocks_alonzo {}\n\
# HELP yggdrasil_blocks_babbage Total Babbage-era blocks applied.\n\
# TYPE yggdrasil_blocks_babbage counter\n\
yggdrasil_blocks_babbage {}\n\
# HELP yggdrasil_blocks_conway Total Conway-era blocks applied.\n\
# TYPE yggdrasil_blocks_conway counter\n\
yggdrasil_blocks_conway {}\n",
            self.blocks_synced,
            self.rollbacks,
            self.batches_completed,
            self.stable_blocks_promoted,
            self.reconnects,
            self.current_slot,
            self.current_block_number,
            self.current_era,
            self.checkpoint_slot,
            self.target_known_peers,
            self.target_established_peers,
            self.target_active_peers,
            self.target_known_big_ledger_peers,
            self.target_established_big_ledger_peers,
            self.target_active_big_ledger_peers,
            self.known_peers,
            self.established_peers,
            self.active_peers,
            self.known_big_ledger_peers,
            self.established_big_ledger_peers,
            self.active_big_ledger_peers,
            self.known_local_root_peers,
            self.established_local_root_peers,
            self.active_local_root_peers,
            self.warm_local_root_peers,
            self.hot_local_root_peers,
            self.uptime_ms / 1000,
            self.mempool_tx_count,
            self.mempool_bytes,
            self.mempool_tx_added,
            self.mempool_tx_rejected,
            self.cm_full_duplex_conns,
            self.cm_duplex_conns,
            self.cm_unidirectional_conns,
            self.cm_inbound_conns,
            self.cm_outbound_conns,
            self.inbound_connections_accepted,
            self.inbound_connections_rejected,
            self.ntc_connections_accepted,
            self.ntc_connections_rejected,
            self.blockfetch_workers_registered,
            self.blockfetch_workers_migrated_total,
            self.chainsync_workers_registered,
            self.blocks_per_era[0],
            self.blocks_per_era[1],
            self.blocks_per_era[2],
            self.blocks_per_era[3],
            self.blocks_per_era[4],
            self.blocks_per_era[5],
            self.blocks_per_era[6],
        );

        // R200 — Append apply-batch duration histogram in the
        // standard Prometheus histogram exposition format.
        out.push_str(
            "# HELP yggdrasil_apply_batch_duration_seconds Time spent applying a batch of fetched blocks to ledger state.\n",
        );
        out.push_str("# TYPE yggdrasil_apply_batch_duration_seconds histogram\n");
        for (i, le) in NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS.iter().enumerate() {
            let le_str = if le.is_infinite() {
                "+Inf".to_string()
            } else {
                format!("{le}")
            };
            out.push_str(&format!(
                "yggdrasil_apply_batch_duration_seconds_bucket{{le=\"{le_str}\"}} {}\n",
                self.apply_batch_duration_buckets[i]
            ));
        }
        let sum_secs = (self.apply_batch_duration_sum_micros as f64) / 1_000_000.0;
        out.push_str(&format!(
            "yggdrasil_apply_batch_duration_seconds_sum {}\n",
            sum_secs
        ));
        out.push_str(&format!(
            "yggdrasil_apply_batch_duration_seconds_count {}\n",
            self.apply_batch_duration_count
        ));

        // R217 — Append fetch-batch duration histogram with the same
        // shape as apply-batch.  Operators can compare `*_apply_*` and
        // `*_fetch_*` rates side-by-side to baseline how much overlap
        // a Phase C.2 pipelined fetch+apply implementation could
        // recover.
        out.push_str(
            "# HELP yggdrasil_fetch_batch_duration_seconds Time spent fetching a batch of blocks from the upstream peer (legacy single-peer path).\n",
        );
        out.push_str("# TYPE yggdrasil_fetch_batch_duration_seconds histogram\n");
        for (i, le) in NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS.iter().enumerate() {
            let le_str = if le.is_infinite() {
                "+Inf".to_string()
            } else {
                format!("{le}")
            };
            out.push_str(&format!(
                "yggdrasil_fetch_batch_duration_seconds_bucket{{le=\"{le_str}\"}} {}\n",
                self.fetch_batch_duration_buckets[i]
            ));
        }
        let fetch_sum_secs = (self.fetch_batch_duration_sum_micros as f64) / 1_000_000.0;
        out.push_str(&format!(
            "yggdrasil_fetch_batch_duration_seconds_sum {}\n",
            fetch_sum_secs
        ));
        out.push_str(&format!(
            "yggdrasil_fetch_batch_duration_seconds_count {}\n",
            self.fetch_batch_duration_count
        ));

        // R223 — Phase D.2 lifetime peer-stats aggregates.  Distinct
        // from the live `known/active/established_peers` gauges
        // which track current session counts; these accumulate
        // monotonically across reconnects so dashboards can alert
        // on real peer churn rate
        // (`rate(yggdrasil_peer_lifetime_sessions_total[5m])`).
        out.push_str(
            "# HELP yggdrasil_peer_lifetime_sessions_total Cumulative count of successful peer sessions across all peers (lifetime, monotonic across reconnects).\n",
        );
        out.push_str("# TYPE yggdrasil_peer_lifetime_sessions_total counter\n");
        out.push_str(&format!(
            "yggdrasil_peer_lifetime_sessions_total {}\n",
            self.peer_lifetime_sessions_total
        ));
        out.push_str(
            "# HELP yggdrasil_peer_lifetime_failures_total Cumulative count of peer session failures (lifetime, monotonic across reconnects).\n",
        );
        out.push_str("# TYPE yggdrasil_peer_lifetime_failures_total counter\n");
        out.push_str(&format!(
            "yggdrasil_peer_lifetime_failures_total {}\n",
            self.peer_lifetime_failures_total
        ));
        out.push_str(
            "# HELP yggdrasil_peer_lifetime_bytes_in_total Cumulative bytes received from peers (lifetime, monotonic across reconnects; sourced from BlockFetch per-peer bytes_delivered).\n",
        );
        out.push_str("# TYPE yggdrasil_peer_lifetime_bytes_in_total counter\n");
        out.push_str(&format!(
            "yggdrasil_peer_lifetime_bytes_in_total {}\n",
            self.peer_lifetime_bytes_in_total
        ));
        out.push_str(
            "# HELP yggdrasil_blockfetch_server_bytes_served_total Cumulative bytes served by the BlockFetch server (yggdrasil-as-peer egress).\n",
        );
        out.push_str("# TYPE yggdrasil_blockfetch_server_bytes_served_total counter\n");
        out.push_str(&format!(
            "yggdrasil_blockfetch_server_bytes_served_total {}\n",
            self.blockfetch_server_bytes_served_total
        ));
        out.push_str(
            "# HELP yggdrasil_peer_lifetime_unique_peers Count of distinct peers ever connected during this process lifetime.\n",
        );
        out.push_str("# TYPE yggdrasil_peer_lifetime_unique_peers gauge\n");
        out.push_str(&format!(
            "yggdrasil_peer_lifetime_unique_peers {}\n",
            self.peer_lifetime_unique_peers
        ));
        out.push_str(
            "# HELP yggdrasil_peer_lifetime_handshakes_total Cumulative successful NtN handshake completions across all peers.\n",
        );
        out.push_str("# TYPE yggdrasil_peer_lifetime_handshakes_total counter\n");
        out.push_str(&format!(
            "yggdrasil_peer_lifetime_handshakes_total {}\n",
            self.peer_lifetime_handshakes_total
        ));

        // R225 — Phase D.1 rollback-depth histogram.  Bucket
        // boundaries (in blocks): 1, 2, 5, 50, 2160 (k), 10 000,
        // +Inf.  Lets operators distinguish shallow chain-reorg
        // rollbacks from rare deep cross-epoch rollbacks (the
        // Phase D.1 problematic case where current behaviour
        // forces a re-sync from origin).
        out.push_str(
            "# HELP yggdrasil_rollback_depth_blocks Distribution of rollback depths (in blocks) observed during sync.\n",
        );
        out.push_str("# TYPE yggdrasil_rollback_depth_blocks histogram\n");
        for (i, le) in NodeMetrics::ROLLBACK_DEPTH_BUCKETS.iter().enumerate() {
            let le_str = if *le == u64::MAX {
                "+Inf".to_string()
            } else {
                format!("{le}")
            };
            out.push_str(&format!(
                "yggdrasil_rollback_depth_blocks_bucket{{le=\"{le_str}\"}} {}\n",
                self.rollback_depth_buckets[i]
            ));
        }
        out.push_str(&format!(
            "yggdrasil_rollback_depth_blocks_sum {}\n",
            self.rollback_depth_sum_blocks
        ));
        out.push_str(&format!(
            "yggdrasil_rollback_depth_blocks_count {}\n",
            self.rollback_depth_count
        ));

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{NodeConfigFile, TraceNamespaceConfig, default_config};

    #[test]
    fn machine_trace_line_is_json() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_option_node_name = Some("yggdrasil-test".to_owned());
        cfg.trace_options = BTreeMap::from([(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec!["Stdout MachineFormat".to_owned()],
                max_frequency: None,
            },
        )]);

        let tracer = NodeTracer::from_config(&cfg);
        let rendered = tracer.format_machine_line(
            "Startup.DiffusionInit",
            "Notice",
            "starting node runtime",
            &trace_fields([
                ("peerCount", Value::from(3)),
                ("networkMagic", Value::from(764824073u64)),
            ]),
        );
        let parsed: Value = serde_json::from_str(&rendered).expect("valid json");

        assert_eq!(parsed["namespace"], Value::from("Startup.DiffusionInit"));
        assert_eq!(parsed["severity"], Value::from("Notice"));
        assert_eq!(parsed["node_name"], Value::from("yggdrasil-test"));
        assert_eq!(parsed["message"], Value::from("starting node runtime"));
        assert_eq!(parsed["data"]["peerCount"], Value::from(3));
    }

    #[test]
    fn namespace_silence_suppresses_event() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "ChainSync.Client".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Silence".to_owned()),
                detail: None,
                backends: vec!["Stdout HumanFormatColoured".to_owned()],
                max_frequency: None,
            },
        );

        let tracer = NodeTracer::from_config(&cfg);
        assert_eq!(tracer.resolve_severity("ChainSync.Client", "Info"), None);
    }

    #[test]
    fn human_trace_line_includes_fields() {
        let tracer = NodeTracer::from_config(&default_config());
        let line = tracer.format_human_line(
            "Net.PeerSelection",
            "Info",
            "bootstrap peer connected",
            &trace_fields([
                ("peer", Value::from("127.0.0.1:3001")),
                ("attempt", Value::from(1)),
            ]),
            false,
        );

        assert!(line.contains("Net.PeerSelection"));
        assert!(line.contains("bootstrap peer connected"));
        assert!(line.contains("peer=127.0.0.1:3001"));
        assert!(line.contains("attempt=1"));
    }

    #[test]
    fn default_config_exposes_checkpoint_namespace_override() {
        let tracer = NodeTracer::from_config(&default_config());

        assert_eq!(
            tracer.resolve_severity("Node.Recovery.Checkpoint", "Notice"),
            Some("Notice")
        );
    }

    #[test]
    fn root_severity_threshold_filters_lower_events() {
        let tracer = NodeTracer::from_config(&default_config());

        // Root threshold is Notice in default config.
        assert_eq!(tracer.resolve_severity("Node.Runtime", "Info"), None);
        assert_eq!(
            tracer.resolve_severity("Node.Runtime", "Warning"),
            Some("Warning")
        );
    }

    #[test]
    fn prefix_namespace_severity_is_applied() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "Net".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Warning".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);

        assert_eq!(tracer.resolve_severity("Net.Handshake", "Info"), None);
        assert_eq!(
            tracer.resolve_severity("Net.Handshake", "Warning"),
            Some("Warning")
        );
    }

    #[test]
    fn exact_namespace_overrides_prefix_threshold() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "Net".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Warning".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: None,
            },
        );
        cfg.trace_options.insert(
            "Net.PeerSelection".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Info".to_owned()),
                detail: None,
                backends: Vec::new(),
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);

        assert_eq!(
            tracer.resolve_severity("Net.PeerSelection", "Info"),
            Some("Info")
        );
    }

    #[test]
    fn prefix_namespace_frequency_is_applied() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "Node.Recovery".to_owned(),
            TraceNamespaceConfig {
                severity: None,
                detail: None,
                backends: Vec::new(),
                max_frequency: Some(2.0),
            },
        );
        let tracer = NodeTracer::from_config(&cfg);

        assert_eq!(
            tracer.min_emit_interval_ms("Node.Recovery.Custom"),
            Some(500)
        );
    }

    #[test]
    fn namespace_frequency_override_maps_to_interval() {
        let tracer = NodeTracer::from_config(&default_config());

        assert_eq!(
            tracer.min_emit_interval_ms("Node.Recovery.Checkpoint"),
            Some(1000)
        );
    }

    #[test]
    fn rate_limiter_blocks_repeated_namespace_events_inside_interval() {
        let tracer = NodeTracer::from_config(&default_config());

        assert!(tracer.should_emit("Node.Recovery.Checkpoint", 1_000));
        assert!(!tracer.should_emit("Node.Recovery.Checkpoint", 1_500));
        assert!(tracer.should_emit("Node.Recovery.Checkpoint", 2_000));
    }

    #[test]
    fn node_metrics_accumulates_counters() {
        let metrics = NodeMetrics::new();

        metrics.add_blocks_synced(10);
        metrics.add_blocks_synced(5);
        metrics.add_rollbacks(1);
        metrics.inc_batches_completed();
        metrics.inc_batches_completed();
        metrics.add_stable_blocks_promoted(3);
        metrics.inc_reconnects();

        let snap = metrics.snapshot();
        assert_eq!(snap.blocks_synced, 15);
        assert_eq!(snap.rollbacks, 1);
        assert_eq!(snap.batches_completed, 2);
        assert_eq!(snap.stable_blocks_promoted, 3);
        assert_eq!(snap.reconnects, 1);
    }

    #[test]
    fn node_metrics_tracks_slot_and_block_number() {
        let metrics = NodeMetrics::new();

        metrics.set_current_slot(42_000);
        metrics.set_current_block_number(1_234);
        metrics.set_checkpoint_slot(41_000);

        let snap = metrics.snapshot();
        assert_eq!(snap.current_slot, 42_000);
        assert_eq!(snap.current_block_number, 1_234);
        assert_eq!(snap.checkpoint_slot, 41_000);
    }

    #[test]
    fn node_metrics_uptime_grows() {
        let metrics = NodeMetrics::new();
        let snap = metrics.snapshot();
        // Uptime should be zero or very small immediately after creation.
        assert!(snap.uptime_ms < 1000);
    }

    #[test]
    fn node_metrics_snapshot_is_serializable() {
        let metrics = NodeMetrics::new();
        metrics.add_blocks_synced(7);
        let snap = metrics.snapshot();
        let json = serde_json::to_string(&snap).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["blocks_synced"], Value::from(7));
    }

    /// Invariant: every numeric field of [`MetricsSnapshot`] must appear in
    /// the Prometheus text emission as `yggdrasil_<field>`. Enforces the
    /// "added an AtomicU64 but forgot to emit it" drift case — without
    /// this test, a new counter slips into the struct + snapshot + JSON
    /// surface silently invisible to Prometheus scrapers.
    ///
    /// Iteration is done via `serde_json::to_value` reflection over the
    /// snapshot so the check stays automatic as fields are added.
    /// `uptime_ms` is the only snapshot field NOT emitted verbatim —
    /// it's published as `yggdrasil_uptime_seconds` (divided by 1000) so
    /// the check accepts either spelling.
    #[test]
    fn every_metrics_snapshot_field_is_exported_in_prometheus_text() {
        let metrics = NodeMetrics::new();
        let snapshot = metrics.snapshot();
        let text = snapshot.to_prometheus_text();

        let json = serde_json::to_value(&snapshot).expect("snapshot is serializable");
        let fields = json
            .as_object()
            .expect("snapshot serialises as a JSON object");

        let mut missing: Vec<&str> = Vec::new();
        for field_name in fields.keys() {
            // Only numeric counter/gauge fields are expected to surface;
            // every current field is u64 or u128.
            let metric_canonical = format!("yggdrasil_{field_name} ");
            // Accept the documented rename for the one non-verbatim field.
            // Round 170 — `blocks_per_era` is exploded into seven
            // explicitly-named counters (`yggdrasil_blocks_byron` …
            // `yggdrasil_blocks_conway`) per Prometheus convention; check
            // each named counter is present.
            let accepts = text.contains(&metric_canonical)
                || (field_name == "uptime_ms" && text.contains("yggdrasil_uptime_seconds"))
                || (field_name == "blocks_per_era"
                    && [
                        "yggdrasil_blocks_byron ",
                        "yggdrasil_blocks_shelley ",
                        "yggdrasil_blocks_allegra ",
                        "yggdrasil_blocks_mary ",
                        "yggdrasil_blocks_alonzo ",
                        "yggdrasil_blocks_babbage ",
                        "yggdrasil_blocks_conway ",
                    ]
                    .iter()
                    .all(|name| text.contains(name)))
                // Round 200 — apply-batch histogram is rendered with
                // standard Prometheus histogram suffixes (`_bucket`,
                // `_sum`, `_count`) under one shared metric name.
                || (field_name == "apply_batch_duration_buckets"
                    && text.contains("yggdrasil_apply_batch_duration_seconds_bucket"))
                || (field_name == "apply_batch_duration_sum_micros"
                    && text.contains("yggdrasil_apply_batch_duration_seconds_sum "))
                || (field_name == "apply_batch_duration_count"
                    && text.contains("yggdrasil_apply_batch_duration_seconds_count "))
                // R217 — fetch-batch histogram (same shape as apply).
                || (field_name == "fetch_batch_duration_buckets"
                    && text.contains("yggdrasil_fetch_batch_duration_seconds_bucket"))
                || (field_name == "fetch_batch_duration_sum_micros"
                    && text.contains("yggdrasil_fetch_batch_duration_seconds_sum "))
                || (field_name == "fetch_batch_duration_count"
                    && text.contains("yggdrasil_fetch_batch_duration_seconds_count "))
                // R225 — rollback-depth histogram.
                || (field_name == "rollback_depth_buckets"
                    && text.contains("yggdrasil_rollback_depth_blocks_bucket"))
                || (field_name == "rollback_depth_sum_blocks"
                    && text.contains("yggdrasil_rollback_depth_blocks_sum "))
                || (field_name == "rollback_depth_count"
                    && text.contains("yggdrasil_rollback_depth_blocks_count "));
            if !accepts {
                missing.push(field_name);
            }
        }
        assert!(
            missing.is_empty(),
            "MetricsSnapshot fields with no Prometheus export line: {missing:?}\n\
             Every new counter must be mirrored in `MetricsSnapshot::to_prometheus_text`."
        );
    }

    #[test]
    fn node_metrics_snapshot_renders_prometheus_text() {
        let metrics = NodeMetrics::new();
        metrics.add_blocks_synced(42);
        metrics.set_current_slot(100);
        metrics.set_peer_selection_counters(20, 10, 5, 6, 3, 1, 18, 7, 4, 4, 2, 1, 3, 2, 1);
        let text = metrics.snapshot().to_prometheus_text();

        assert!(text.contains("yggdrasil_blocks_synced 42\n"));
        assert!(text.contains("yggdrasil_current_slot 100\n"));
        assert!(text.contains("yggdrasil_target_known_peers 20\n"));
        assert!(text.contains("yggdrasil_known_big_ledger_peers 4\n"));
        assert!(text.contains("# TYPE yggdrasil_blocks_synced counter\n"));
        assert!(text.contains("# TYPE yggdrasil_current_slot gauge\n"));
        assert!(text.contains("# TYPE yggdrasil_target_known_peers gauge\n"));
        assert!(text.contains("yggdrasil_uptime_seconds"));
    }

    /// R231 — pin the R200 apply-batch + R217 fetch-batch
    /// duration histogram contracts.  Both share
    /// [`NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS`] bucket
    /// boundaries so dashboards can render fetch-vs-apply
    /// side-by-side comparisons (R217+R218 multi-peer sync-rate
    /// quantification depends on this).  Pins:
    /// (1) bucket boundaries `[1ms, 5ms, 10ms, 50ms, 100ms, 500ms,
    /// 1s, 5s, 10s, +Inf]` — drift means dashboards misclassify
    /// latency tier;
    /// (2) cumulative-bucket semantic (observation `d` increments
    /// every bucket whose `le_secs` is ≥ `d`);
    /// (3) Prometheus exposition shape for both metrics.
    #[test]
    fn node_metrics_tracks_fetch_and_apply_batch_histograms() {
        use std::time::Duration;

        // Bucket-boundary pin.  Each numeric value is load-bearing
        // for operator alerting.
        assert_eq!(
            NodeMetrics::APPLY_BATCH_BUCKETS_SECONDS,
            [
                0.001,
                0.005,
                0.01,
                0.05,
                0.1,
                0.5,
                1.0,
                5.0,
                10.0,
                f64::INFINITY
            ],
        );

        let metrics = NodeMetrics::new();

        // Default: no observations.
        let snap = metrics.snapshot();
        assert_eq!(snap.apply_batch_duration_count, 0);
        assert_eq!(snap.fetch_batch_duration_count, 0);

        // Apply observation: 200ms (a typical mainnet apply per
        // R218).  Falls into le=0.5 and higher (5 buckets).
        metrics.record_apply_batch_duration(Duration::from_millis(200));
        let snap = metrics.snapshot();
        assert_eq!(snap.apply_batch_duration_count, 1);
        assert_eq!(snap.apply_batch_duration_buckets[0], 0, "le=0.001 < 0.2s");
        assert_eq!(snap.apply_batch_duration_buckets[4], 0, "le=0.1 < 0.2s");
        assert_eq!(
            snap.apply_batch_duration_buckets[5], 1,
            "le=0.5 includes 0.2s"
        );
        assert_eq!(
            snap.apply_batch_duration_buckets[9], 1,
            "+Inf includes everything"
        );

        // Fetch observation: 12.85s (R217 mainnet single-peer
        // baseline).  Falls into +Inf only (>10s).
        metrics.record_fetch_batch_duration(Duration::from_millis(12_850));
        let snap = metrics.snapshot();
        assert_eq!(snap.fetch_batch_duration_count, 1);
        assert_eq!(snap.fetch_batch_duration_buckets[8], 0, "le=10.0 < 12.85s");
        assert_eq!(
            snap.fetch_batch_duration_buckets[9], 1,
            "+Inf includes 12.85s"
        );

        // Fetch observation: 8.56s (R218 multi-peer, 2 active
        // workers).  Falls into le=10.0 and +Inf.
        metrics.record_fetch_batch_duration(Duration::from_millis(8_560));
        let snap = metrics.snapshot();
        assert_eq!(snap.fetch_batch_duration_count, 2);
        assert_eq!(
            snap.fetch_batch_duration_buckets[8], 1,
            "le=10.0 includes 8.56s"
        );
        assert_eq!(
            snap.fetch_batch_duration_buckets[9], 2,
            "+Inf includes both"
        );

        // Prometheus text format pin.
        let text = snap.to_prometheus_text();
        assert!(text.contains("# TYPE yggdrasil_apply_batch_duration_seconds histogram\n"));
        assert!(text.contains("# TYPE yggdrasil_fetch_batch_duration_seconds histogram\n"));
        assert!(
            text.contains("yggdrasil_apply_batch_duration_seconds_bucket{le=\"0.5\"} 1\n"),
            "apply le=0.5 not exposed"
        );
        assert!(
            text.contains("yggdrasil_fetch_batch_duration_seconds_bucket{le=\"+Inf\"} 2\n"),
            "fetch +Inf not exposed"
        );
        assert!(text.contains("yggdrasil_apply_batch_duration_seconds_count 1\n"));
        assert!(text.contains("yggdrasil_fetch_batch_duration_seconds_count 2\n"));
    }

    /// R230 — pin the Phase D.1 rollback-depth histogram contract
    /// from R225.  Bucket boundaries `[1, 2, 5, 50, 2160 (k),
    /// 10_000, +Inf]` are load-bearing — operator dashboards and
    /// `histogram_quantile(0.99, …)` alerts depend on them.
    /// Also pins the cumulative-bucket semantic: an observation of
    /// depth `d` increments every bucket whose `le` is ≥ `d` (so
    /// the +Inf bucket is the total observation count).
    #[test]
    fn node_metrics_tracks_phase_d1_rollback_depth_histogram() {
        let metrics = NodeMetrics::new();

        // Default: zero observations.
        let snap = metrics.snapshot();
        assert_eq!(snap.rollback_depth_count, 0);
        assert_eq!(snap.rollback_depth_sum_blocks, 0);
        for bucket in &snap.rollback_depth_buckets {
            assert_eq!(*bucket, 0);
        }

        // Observation 1: depth=0 (session-start confirm rollback,
        // common case).  Falls into every bucket including le=1.
        metrics.record_rollback_depth(0);
        let snap = metrics.snapshot();
        assert_eq!(snap.rollback_depth_count, 1);
        assert_eq!(snap.rollback_depth_sum_blocks, 0);
        for (i, bucket) in snap.rollback_depth_buckets.iter().enumerate() {
            assert_eq!(*bucket, 1, "depth=0 must increment every bucket (i={i})");
        }

        // Observation 2: depth=3 (small chain reorg).  Falls into
        // le=5, le=50, le=2160, le=10_000, le=+Inf (5 buckets).
        // Does NOT fall into le=1 or le=2.
        metrics.record_rollback_depth(3);
        let snap = metrics.snapshot();
        assert_eq!(snap.rollback_depth_count, 2);
        assert_eq!(snap.rollback_depth_sum_blocks, 3);
        assert_eq!(
            snap.rollback_depth_buckets[0], 1,
            "le=1 unchanged for depth=3"
        );
        assert_eq!(
            snap.rollback_depth_buckets[1], 1,
            "le=2 unchanged for depth=3"
        );
        assert_eq!(snap.rollback_depth_buckets[2], 2, "le=5 includes depth=3");
        assert_eq!(
            snap.rollback_depth_buckets[6], 2,
            "+Inf includes everything"
        );

        // Observation 3: depth=5000 (cross-epoch range).  Falls into
        // le=10_000 and le=+Inf only.
        metrics.record_rollback_depth(5000);
        let snap = metrics.snapshot();
        assert_eq!(snap.rollback_depth_count, 3);
        assert_eq!(snap.rollback_depth_sum_blocks, 3 + 5000);
        assert_eq!(
            snap.rollback_depth_buckets[5], 3,
            "le=10_000 includes depth=5000"
        );
        assert_eq!(
            snap.rollback_depth_buckets[6], 3,
            "+Inf still includes everything"
        );
        assert_eq!(
            snap.rollback_depth_buckets[4], 2,
            "le=2160 (k) does NOT include 5000"
        );

        // Bucket boundaries pin: drift here means operator dashboards
        // misclassify rollback severity.
        assert_eq!(
            NodeMetrics::ROLLBACK_DEPTH_BUCKETS,
            [1, 2, 5, 50, 2160, 10_000, u64::MAX]
        );

        // Prometheus text format pin.
        let text = snap.to_prometheus_text();
        assert!(text.contains("# TYPE yggdrasil_rollback_depth_blocks histogram\n"));
        assert!(
            text.contains("yggdrasil_rollback_depth_blocks_bucket{le=\"1\"} 1\n"),
            "le=1 bucket value not exposed correctly"
        );
        assert!(
            text.contains("yggdrasil_rollback_depth_blocks_bucket{le=\"+Inf\"} 3\n"),
            "+Inf bucket value not exposed correctly"
        );
        assert!(text.contains("yggdrasil_rollback_depth_blocks_sum 5003\n"));
        assert!(text.contains("yggdrasil_rollback_depth_blocks_count 3\n"));
    }

    /// R229 — pin the Phase D.2 5-counter lifetime peer-stats
    /// Prometheus output contract.  The 4 counters
    /// (`*_total`) MUST emit `# TYPE …_total counter`; the 1
    /// gauge (`unique_peers`) MUST emit `# TYPE … gauge`.  Drift
    /// in the contract (e.g. accidentally emitting a counter as a
    /// gauge) silently breaks operator alerts that depend on
    /// `rate(...)` semantics.
    ///
    /// References R222–R226 (the lifetime peer-stats deliverable).
    #[test]
    fn node_metrics_tracks_phase_d2_lifetime_peer_stats() {
        let metrics = NodeMetrics::new();

        // Default state: all five lifetime counters at zero.
        let snap = metrics.snapshot();
        assert_eq!(snap.peer_lifetime_sessions_total, 0);
        assert_eq!(snap.peer_lifetime_failures_total, 0);
        assert_eq!(snap.peer_lifetime_bytes_in_total, 0);
        assert_eq!(snap.peer_lifetime_unique_peers, 0);
        assert_eq!(snap.peer_lifetime_handshakes_total, 0);

        // Simulate governor-tick aggregate updates.
        metrics.set_peer_lifetime_sessions_total(7);
        metrics.set_peer_lifetime_failures_total(2);
        metrics.set_peer_lifetime_bytes_in_total(1_500_000);
        metrics.set_peer_lifetime_unique_peers(9);
        metrics.set_peer_lifetime_handshakes_total(7);

        let snap = metrics.snapshot();
        assert_eq!(snap.peer_lifetime_sessions_total, 7);
        assert_eq!(snap.peer_lifetime_failures_total, 2);
        assert_eq!(snap.peer_lifetime_bytes_in_total, 1_500_000);
        assert_eq!(snap.peer_lifetime_unique_peers, 9);
        assert_eq!(snap.peer_lifetime_handshakes_total, 7);

        // Prometheus text contract — TYPE lines + value lines for
        // each of the 5 metrics, with correct counter / gauge
        // discrimination.
        let text = snap.to_prometheus_text();

        // 4 counters.
        for counter in [
            "yggdrasil_peer_lifetime_sessions_total",
            "yggdrasil_peer_lifetime_failures_total",
            "yggdrasil_peer_lifetime_bytes_in_total",
            "yggdrasil_peer_lifetime_handshakes_total",
        ] {
            assert!(
                text.contains(&format!("# TYPE {counter} counter\n")),
                "missing counter TYPE for {counter}"
            );
        }
        // 1 gauge.
        assert!(
            text.contains("# TYPE yggdrasil_peer_lifetime_unique_peers gauge\n"),
            "unique_peers must be a gauge (cardinality of map)"
        );

        // Value lines.
        assert!(text.contains("yggdrasil_peer_lifetime_sessions_total 7\n"));
        assert!(text.contains("yggdrasil_peer_lifetime_failures_total 2\n"));
        assert!(text.contains("yggdrasil_peer_lifetime_bytes_in_total 1500000\n"));
        assert!(text.contains("yggdrasil_peer_lifetime_unique_peers 9\n"));
        assert!(text.contains("yggdrasil_peer_lifetime_handshakes_total 7\n"));
    }

    #[test]
    fn node_metrics_tracks_blockfetch_worker_pool_size() {
        // Phase 6 multi-peer dispatch observability: operators must
        // be able to verify activation of the multi-peer path via
        // `/metrics`.  `blockfetch_workers_registered` reports the
        // current pool size; `blockfetch_workers_migrated_total`
        // counts lifetime migrations.
        let metrics = NodeMetrics::new();
        // Default state: no workers, no migrations.
        let snap = metrics.snapshot();
        assert_eq!(snap.blockfetch_workers_registered, 0);
        assert_eq!(snap.blockfetch_workers_migrated_total, 0);

        // Simulate 2 peers being migrated.
        metrics.inc_blockfetch_workers_migrated();
        metrics.inc_blockfetch_workers_migrated();
        metrics.set_blockfetch_workers_registered(2);
        let snap = metrics.snapshot();
        assert_eq!(snap.blockfetch_workers_registered, 2);
        assert_eq!(snap.blockfetch_workers_migrated_total, 2);

        // One peer disconnects; pool size shrinks but lifetime count
        // is monotonic.
        metrics.set_blockfetch_workers_registered(1);
        let snap = metrics.snapshot();
        assert_eq!(snap.blockfetch_workers_registered, 1);
        assert_eq!(snap.blockfetch_workers_migrated_total, 2);

        // Prometheus text contains both lines for scrape parity.
        let text = metrics.snapshot().to_prometheus_text();
        assert!(text.contains("yggdrasil_blockfetch_workers_registered 1\n"));
        assert!(text.contains("yggdrasil_blockfetch_workers_migrated_total 2\n"));
        assert!(text.contains("# TYPE yggdrasil_blockfetch_workers_registered gauge\n"));
        assert!(text.contains("# TYPE yggdrasil_blockfetch_workers_migrated_total counter\n"));
    }

    #[test]
    fn node_metrics_tracks_peer_selection_counters() {
        let metrics = NodeMetrics::new();

        metrics.set_peer_selection_counters(30, 18, 7, 9, 4, 2, 22, 11, 6, 8, 3, 1, 5, 3, 2);

        let snap = metrics.snapshot();
        assert_eq!(snap.target_known_peers, 30);
        assert_eq!(snap.target_established_peers, 18);
        assert_eq!(snap.target_active_peers, 7);
        assert_eq!(snap.target_known_big_ledger_peers, 9);
        assert_eq!(snap.target_established_big_ledger_peers, 4);
        assert_eq!(snap.target_active_big_ledger_peers, 2);
        assert_eq!(snap.known_peers, 22);
        assert_eq!(snap.established_peers, 11);
        assert_eq!(snap.active_peers, 6);
        assert_eq!(snap.known_big_ledger_peers, 8);
        assert_eq!(snap.established_big_ledger_peers, 3);
        assert_eq!(snap.active_big_ledger_peers, 1);
        assert_eq!(snap.known_local_root_peers, 5);
        assert_eq!(snap.established_local_root_peers, 3);
        assert_eq!(snap.active_local_root_peers, 2);
        assert_eq!(snap.warm_local_root_peers, 3);
        assert_eq!(snap.hot_local_root_peers, 2);
    }

    // -----------------------------------------------------------------------
    // Coloured stdout backend tests
    // -----------------------------------------------------------------------

    #[test]
    fn coloured_human_line_contains_ansi_codes_for_warning() {
        let tracer = NodeTracer::from_config(&default_config());
        let line = tracer.format_human_line(
            "Net.PeerSelection",
            "Warning",
            "peer timed out",
            &BTreeMap::new(),
            true,
        );

        // Yellow ANSI start + reset at end.
        assert!(line.starts_with("\x1b[33m"));
        assert!(line.ends_with("\x1b[0m"));
        assert!(line.contains("Warning"));
    }

    #[test]
    fn coloured_human_line_no_ansi_for_info() {
        let tracer = NodeTracer::from_config(&default_config());
        let line = tracer.format_human_line("Startup", "Info", "starting", &BTreeMap::new(), true);

        // Info has no colour code, so no ANSI escape and no reset.
        assert!(!line.contains("\x1b["));
    }

    #[test]
    fn uncoloured_human_line_has_no_ansi() {
        let tracer = NodeTracer::from_config(&default_config());
        let line = tracer.format_human_line(
            "Net.PeerSelection",
            "Error",
            "connection failed",
            &BTreeMap::new(),
            false,
        );

        assert!(!line.contains("\x1b["));
    }

    #[test]
    fn coloured_backend_recognised_from_config_string() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec!["Stdout HumanFormatColoured".to_owned()],
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        let backends = tracer.backends_for("Net.Handshake");
        assert_eq!(backends, vec![TraceBackend::StdoutHumanColoured]);
    }

    // -----------------------------------------------------------------------
    // Upstream backend string recognition tests
    // -----------------------------------------------------------------------

    #[test]
    fn ekg_backend_string_yields_no_trace_backend() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec!["EKGBackend".to_owned()],
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        assert!(tracer.backends_for("Net").is_empty());
    }

    #[test]
    fn forwarder_backend_string_yields_forwarder_trace_backend() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec!["Forwarder".to_owned()],
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        assert_eq!(
            tracer.backends_for("Startup"),
            vec![TraceBackend::Forwarder]
        );
    }

    #[test]
    fn prometheus_simple_backend_string_yields_no_trace_backend() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec!["PrometheusSimple suffix 127.0.0.1 12798".to_owned()],
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        assert!(tracer.backends_for("ChainDB").is_empty());
    }

    #[test]
    fn mixed_upstream_backends_resolve_correctly() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec![
                    "EKGBackend".to_owned(),
                    "Forwarder".to_owned(),
                    "PrometheusSimple suffix 127.0.0.1 12798".to_owned(),
                    "Stdout HumanFormatColoured".to_owned(),
                ],
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        let backends = tracer.backends_for("Net");
        // Forwarder and stdout coloured backends both resolve.
        assert_eq!(
            backends,
            vec![TraceBackend::Forwarder, TraceBackend::StdoutHumanColoured]
        );
    }

    #[test]
    fn clone_preserves_forwarder_transport_when_enabled() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: None,
                backends: vec!["Forwarder".to_owned()],
                max_frequency: None,
            },
        );

        let tracer = NodeTracer::from_config(&cfg);
        let cloned = tracer.clone();

        let original = tracer
            .forwarder
            .as_ref()
            .expect("forwarder should be configured on original tracer");
        let cloned_forwarder = cloned
            .forwarder
            .as_ref()
            .expect("forwarder should be configured on cloned tracer");

        assert!(Arc::ptr_eq(original, cloned_forwarder));
    }

    #[test]
    fn clone_shares_rate_limiter_state() {
        let tracer = NodeTracer::from_config(&default_config());
        let cloned = tracer.clone();

        assert!(tracer.should_emit("Node.Recovery.Checkpoint", 1_000));
        assert!(!cloned.should_emit("Node.Recovery.Checkpoint", 1_500));
        assert!(cloned.should_emit("Node.Recovery.Checkpoint", 2_000));
    }

    // -----------------------------------------------------------------------
    // Detail level tests
    // -----------------------------------------------------------------------

    #[test]
    fn detail_for_returns_dnormal_when_unconfigured() {
        let tracer = NodeTracer::from_config(&default_config());
        assert_eq!(tracer.detail_for("Net.PeerSelection"), TraceDetail::DNormal);
    }

    #[test]
    fn detail_for_respects_root_config() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: Some("DDetailed".to_owned()),
                backends: vec!["Stdout HumanFormat".to_owned()],
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        assert_eq!(tracer.detail_for("Any.Namespace"), TraceDetail::DDetailed);
    }

    #[test]
    fn detail_for_respects_namespace_override() {
        let mut cfg: NodeConfigFile = default_config();
        cfg.trace_options.insert(
            "".to_owned(),
            TraceNamespaceConfig {
                severity: Some("Notice".to_owned()),
                detail: Some("DNormal".to_owned()),
                backends: vec!["Stdout HumanFormat".to_owned()],
                max_frequency: None,
            },
        );
        cfg.trace_options.insert(
            "Net.PeerSelection".to_owned(),
            TraceNamespaceConfig {
                severity: None,
                detail: Some("DMaximum".to_owned()),
                backends: Vec::new(),
                max_frequency: None,
            },
        );
        let tracer = NodeTracer::from_config(&cfg);
        assert_eq!(
            tracer.detail_for("Net.PeerSelection"),
            TraceDetail::DMaximum
        );
        assert_eq!(tracer.detail_for("Net.Handshake"), TraceDetail::DNormal);
    }

    #[test]
    fn detail_from_label_parses_upstream_strings() {
        assert_eq!(
            TraceDetail::from_label("DMinimal"),
            Some(TraceDetail::DMinimal)
        );
        assert_eq!(
            TraceDetail::from_label("DNormal"),
            Some(TraceDetail::DNormal)
        );
        assert_eq!(
            TraceDetail::from_label("DDetailed"),
            Some(TraceDetail::DDetailed)
        );
        assert_eq!(
            TraceDetail::from_label("DMaximum"),
            Some(TraceDetail::DMaximum)
        );
        assert_eq!(TraceDetail::from_label("invalid"), None);
    }

    #[test]
    fn trace_detail_ordering() {
        assert!(TraceDetail::DMinimal < TraceDetail::DNormal);
        assert!(TraceDetail::DNormal < TraceDetail::DDetailed);
        assert!(TraceDetail::DDetailed < TraceDetail::DMaximum);
    }
}
