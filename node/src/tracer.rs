//! Thin node-side tracing helpers aligned with the Cardano trace dispatcher
//! vocabulary.
//!
//! Yggdrasil currently emits local trace objects to stdout in either machine
//! or human format, based on the configured `TraceOptions` backends. This keeps
//! runtime tracing aligned with the official node's producer role while the
//! dedicated tracer transport remains a future milestone.

use std::collections::BTreeMap;
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
}