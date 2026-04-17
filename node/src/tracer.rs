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
            start_time_ms: current_unix_millis(),
        }
    }

    /// Add `n` to the blocks-synced counter.
    pub fn add_blocks_synced(&self, n: u64) {
        self.blocks_synced.fetch_add(n, Ordering::Relaxed);
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
        format!(
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
yggdrasil_inbound_connections_rejected {}\n",
            self.blocks_synced,
            self.rollbacks,
            self.batches_completed,
            self.stable_blocks_promoted,
            self.reconnects,
            self.current_slot,
            self.current_block_number,
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
        )
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
