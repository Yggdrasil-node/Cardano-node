//! Thin node-side tracing helpers aligned with the Cardano trace dispatcher
//! vocabulary.
//!
//! Yggdrasil currently emits local trace objects to stdout in either machine
//! or human format, based on the configured `TraceOptions` backends. This keeps
//! runtime tracing aligned with the official node's producer role while the
//! dedicated tracer transport remains a future milestone.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::config::{NodeConfigFile, TraceNamespaceConfig};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TraceBackend {
    StdoutHuman,
    StdoutMachine,
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
#[derive(Clone, Debug)]
pub struct NodeTracer {
    turn_on_logging: bool,
    use_trace_dispatcher: bool,
    trace_option_node_name: Option<String>,
    trace_options: BTreeMap<String, TraceNamespaceConfig>,
    last_emit_ms: Arc<Mutex<BTreeMap<String, u128>>>,
}

impl NodeTracer {
    /// Build a tracer from the effective node configuration.
    pub fn from_config(config: &NodeConfigFile) -> Self {
        Self {
            turn_on_logging: config.turn_on_logging,
            use_trace_dispatcher: config.use_trace_dispatcher,
            trace_option_node_name: config.trace_option_node_name.clone(),
            trace_options: config.trace_options.clone(),
            last_emit_ms: Arc::new(Mutex::new(BTreeMap::new())),
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
                        self.format_human_line(namespace, severity, &message, &data)
                    );
                }
                TraceBackend::StdoutMachine => {
                    println!(
                        "{}",
                        self.format_machine_line(namespace, severity, &message, &data)
                    );
                }
            }
        }
    }

    fn resolve_severity<'a>(&'a self, namespace: &str, default_severity: &'a str) -> Option<&'a str> {
        if !(self.turn_on_logging && self.use_trace_dispatcher) {
            return None;
        }

        let namespace_severity = self
            .trace_options
            .get(namespace)
            .and_then(|cfg| cfg.severity.as_deref());
        let root_severity = self
            .trace_options
            .get("")
            .and_then(|cfg| cfg.severity.as_deref());
        let severity = namespace_severity.or(root_severity).unwrap_or(default_severity);

        if severity.eq_ignore_ascii_case("Silence") {
            None
        } else {
            Some(severity)
        }
    }

    fn backends_for(&self, namespace: &str) -> Vec<TraceBackend> {
        let configured = self
            .trace_options
            .get(namespace)
            .filter(|cfg| !cfg.backends.is_empty())
            .or_else(|| self.trace_options.get(""));

        configured
            .map(|cfg| {
                cfg.backends
                    .iter()
                    .filter_map(|backend| match backend.as_str() {
                        s if s.starts_with("Stdout HumanFormat") => Some(TraceBackend::StdoutHuman),
                        s if s.starts_with("Stdout MachineFormat") => Some(TraceBackend::StdoutMachine),
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
            .trace_options
            .get(namespace)
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
    ) -> String {
        let mut line = format!(
            "[{}] {} {}",
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
        self.current_block_number.store(block_number, Ordering::Relaxed);
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
            established_big_ledger_peers: self
                .established_big_ledger_peers
                .load(Ordering::Relaxed),
            active_big_ledger_peers: self.active_big_ledger_peers.load(Ordering::Relaxed),
            known_local_root_peers: self.known_local_root_peers.load(Ordering::Relaxed),
            established_local_root_peers: self
                .established_local_root_peers
                .load(Ordering::Relaxed),
            active_local_root_peers: self.active_local_root_peers.load(Ordering::Relaxed),
            warm_local_root_peers: self.established_local_root_peers.load(Ordering::Relaxed),
            hot_local_root_peers: self.active_local_root_peers.load(Ordering::Relaxed),
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
yggdrasil_uptime_seconds {}\n",
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
            Some("Info")
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
}
