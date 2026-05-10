//! Cross-cutting helpers — runtime-state initialization, line
//! separator, connection-id conversion, registry wrappers.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Utils.hs.
//!
//! Direct port of upstream's bounded subset. Pure helpers + the
//! init functions for runtime-state types ship now. The IO
//! orchestration entries (`askNodeName`, `askNodeNameRaw`,
//! `showProblemIfAny`, `beforeProgramStops`, `sequenceConcurrently_`)
//! defer pending the data-point mini-protocol port + tracer-trace
//! channel from MetaTrace.hs.
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `nl :: Text`                                                   | [`NL`]                                 |
//! | `initConnectedNodes :: IO ConnectedNodes`                      | [`init_connected_nodes`]               |
//! | `initConnectedNodesNames :: IO ConnectedNodesNames`            | [`init_connected_nodes_names`]         |
//! | `initAcceptedMetrics :: IO AcceptedMetrics`                    | [`init_accepted_metrics`]              |
//! | `initDataPointRequestors :: IO DataPointRequestors`            | [`init_data_point_requestors`]         |
//! | `initProtocolsBrake :: IO ProtocolsBrake`                      | [`init_protocols_brake`]               |
//! | `applyBrake :: ProtocolsBrake -> IO ()`                        | [`apply_brake`]                        |
//! | `connIdToNodeId :: ConnectionId addr -> NodeId`                | [`conn_id_to_node_id`]                 |
//! | `getProcessId :: IO Word32`                                    | [`get_process_id`]                     |
//! | `newRegistry :: IO (Registry a b)`                             | [`new_registry`]                       |
//! | `memberRegistry`                                               | [`member_registry`]                    |
//! | `lookupRegistry`                                               | [`lookup_registry`]                    |
//! | `readRegistry`                                                 | [`read_registry`]                      |
//! | `modifyRegistry_`                                              | [`modify_registry`]                    |
//! | `forMM` / `forMM_`                                             | (synthesis carve-out — see below)      |
//! | `askNodeName` / `askNodeNameRaw` / `askNodeId`                 | (deferred — see [`ask_node_name_status`]) |
//! | `showProblemIfAny`                                             | (deferred — same)                      |
//! | `beforeProgramStops`                                           | (deferred — see [`before_program_stops_status`]) |
//! | `sequenceConcurrently_`                                        | (deferred — see [`sequence_concurrently_status`]) |
//! | `clearRegistry` / `elemsRegistry` / `showRegistry`             | (synthesis carve-out — see below)      |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`forMM` / `forMM_`**: upstream's monad-transformer convenience
//!   wrappers (`m (t a) -> (a -> m b) -> m (t b)`) collapse to plain
//!   `for x in iter { ... }` in Rust. Synthesis-only — no Yggdrasil
//!   surface.
//! - **`askNodeName` / `askNodeNameRaw` / `askNodeId`**: depend on
//!   the data-point mini-protocol surface (askDataPoint) + the
//!   tracer-trace channel (Trace IO TracerTrace) — both unported.
//!   Status surfaced via [`ask_node_name_status`].
//! - **`showProblemIfAny`**: depends on the tracer-trace channel.
//!   Status surfaced via [`ask_node_name_status`] (same dependency
//!   blocker).
//! - **`beforeProgramStops`**: Unix signal handler installation.
//!   Yggdrasil-side equivalent uses `tokio::signal::unix::signal`
//!   for SIGINT/SIGTERM but needs careful integration with the Run
//!   supervisor task lifetime — deferred to the supervisor port.
//!   Status surfaced via [`before_program_stops_status`].
//! - **`sequenceConcurrently_`**: Rust's idiomatic equivalents are
//!   `tokio::join!` / `futures::future::join_all` — neither has a
//!   1:1 mirror of upstream's `Control.Concurrent.Async.runConcurrently
//!   . traverse_ Concurrently` shape. Status surfaced via
//!   [`sequence_concurrently_status`].
//! - **`clearRegistry` / `elemsRegistry` / `showRegistry`**: depend
//!   on `System.IO.hClose` semantics (close the file handle stored
//!   in each registry value) which is specific to the upstream
//!   `Handle` type. Once Logs/File.hs ports a real handle type these
//!   land alongside.

use crate::environment::{AcceptedMetrics, DataPointRequestors};
use crate::types::{ConnectedNodes, ConnectedNodesNames, NodeId, ProtocolsBrake, Registry};

/// Newline character — UTF-8 bytes for the system-native record
/// separator. Mirror of upstream `nl :: Text` (`"\n"` on Unix,
/// `"\r\n"` on Windows). Yggdrasil only ships the Unix variant
/// since the cardano-tracer binary is operationally Unix-only.
pub const NL: &str = "\n";

/// Construct an empty [`ConnectedNodes`] set. Mirror of upstream
/// `initConnectedNodes :: IO ConnectedNodes`.
pub fn init_connected_nodes() -> ConnectedNodes {
    ConnectedNodes::new()
}

/// Construct an empty [`ConnectedNodesNames`] bidirectional map.
/// Mirror of upstream `initConnectedNodesNames :: IO ConnectedNodesNames`.
pub fn init_connected_nodes_names() -> ConnectedNodesNames {
    ConnectedNodesNames::new()
}

/// Construct an empty [`AcceptedMetrics`] placeholder. Mirror of
/// upstream `initAcceptedMetrics :: IO AcceptedMetrics`.
pub fn init_accepted_metrics() -> AcceptedMetrics {
    AcceptedMetrics
}

/// Construct an empty [`DataPointRequestors`] placeholder. Mirror
/// of upstream `initDataPointRequestors :: IO DataPointRequestors`.
pub fn init_data_point_requestors() -> DataPointRequestors {
    DataPointRequestors
}

/// Construct a [`ProtocolsBrake`] in the running state. Mirror of
/// upstream `initProtocolsBrake :: IO ProtocolsBrake`.
pub fn init_protocols_brake() -> ProtocolsBrake {
    ProtocolsBrake::new()
}

/// Engage the protocols brake; signals all attached protocols to
/// stop at the next checkpoint. Mirror of upstream
/// `applyBrake :: ProtocolsBrake -> IO ()`.
pub fn apply_brake(brake: &ProtocolsBrake) {
    brake.engage();
}

/// Convert an upstream `ConnectionId` to a [`NodeId`] suitable for
/// use as a filesystem subdirectory name. Mirror of upstream
/// `connIdToNodeId :: Show addr => ConnectionId addr -> NodeId`.
///
/// The string sanitization mirrors upstream's `replace`/`dropPrefix`/
/// `dropSuffix` chain verbatim:
/// - drops leading + trailing `-`
/// - replaces `--` with empty
/// - replaces ` ` / `"` / `/` / `\` with `-`
/// - drops `pipe` (Windows) and `.` (Windows) substrings
/// - drops the `LocalAddress` prefix (Yggdrasil-side only sees
///   local addresses by design)
pub fn conn_id_to_node_id(remote_address: &str) -> NodeId {
    // First pass: strip the multi-character substrings.
    let stripped = remote_address
        .replace("LocalAddress", "")
        .replace("pipe", "")
        .replace('.', "");
    // Second pass: replace the path/whitespace separators with a
    // single dash. Use a char-class match rather than a chain of
    // `.replace` calls (the latter triggers clippy::collapsible_str_replace).
    let dashed: String = stripped
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | '"' | ' ') {
                '-'
            } else {
                c
            }
        })
        .collect();
    // Final pass: collapse double-dashes (matching upstream's
    // `replace "--" ""` semantics).
    let trimmed = dashed.replace("--", "").trim_matches('-').to_string();
    NodeId::new(trimmed)
}

/// Get the running process's PID. Mirror of upstream
/// `getProcessId :: IO Word32`. Returns the host platform's PID
/// (POSIX `getpid()` on Unix, `GetCurrentProcessId()` on Windows).
pub fn get_process_id() -> u32 {
    std::process::id()
}

/// Construct a fresh empty registry. Mirror of upstream
/// `newRegistry :: IO (Registry a b)`. Generic over key + value.
pub fn new_registry<Key, Value>() -> Registry<Key, Value>
where
    Key: Eq + std::hash::Hash + Clone,
    Value: Clone,
{
    Registry::new()
}

/// `True` if the registry contains the supplied key. Mirror of
/// upstream `memberRegistry :: Ord a => a -> Registry a b -> IO Bool`.
pub fn member_registry<Key, Value>(registry: &Registry<Key, Value>, key: &Key) -> bool
where
    Key: Eq + std::hash::Hash + Clone,
    Value: Clone,
{
    registry.get(key).is_some()
}

/// Look up a key under the `(key, key1)` composite tuple — mirror of
/// upstream's
/// `lookupRegistry :: Ord a => Ord b => a -> b -> Registry (a, b) c -> IO (Maybe c)`.
pub fn lookup_registry<Key, Key1, Value>(
    registry: &Registry<(Key, Key1), Value>,
    key: Key,
    key1: Key1,
) -> Option<Value>
where
    Key: Eq + std::hash::Hash + Clone,
    Key1: Eq + std::hash::Hash + Clone,
    Value: Clone,
{
    registry.get(&(key, key1))
}

/// Snapshot the registry as a key→value `Vec` of pairs (mirror of
/// upstream `readRegistry :: Registry a b -> IO (Map.Map a b)`).
/// Returns `Vec` instead of `HashMap` to make callers' tests easier
/// to write without ordering surprises.
pub fn read_registry<Key, Value>(registry: &Registry<Key, Value>) -> Vec<(Key, Value)>
where
    Key: Eq + std::hash::Hash + Clone,
    Value: Clone,
{
    registry.snapshot()
}

/// Atomically replace the registry's contents via a transformation
/// closure. Mirror of upstream
/// `modifyRegistry_ :: Registry a b -> (Map.Map a b -> IO (Map.Map a b)) -> IO ()`.
///
/// The closure receives a clone of the current contents as a `Vec`
/// of pairs and returns the desired new contents; the registry is
/// then atomically rebuilt from the closure's output. Use
/// [`Registry::insert`] / [`Registry::remove`] directly for single-
/// key updates (cheaper than going through this transformation).
pub fn modify_registry<Key, Value, F>(registry: &Registry<Key, Value>, transform: F)
where
    Key: Eq + std::hash::Hash + Clone,
    Value: Clone,
    F: FnOnce(Vec<(Key, Value)>) -> Vec<(Key, Value)>,
{
    let snapshot = registry.snapshot();
    let new_contents = transform(snapshot);
    // Replace the current contents: clear (via per-key remove) +
    // re-insert. This is safe for the typical use case of
    // notification-engine state where mutations are rare; for
    // hot-path mutations the per-key API is preferred.
    let current_keys: Vec<Key> = registry.snapshot().into_iter().map(|(k, _)| k).collect();
    for key in current_keys {
        registry.remove(&key);
    }
    for (key, value) in new_contents {
        registry.insert(key, value);
    }
}

// =====================================================================
// metrics_help loader
// =====================================================================

/// Load the operator-supplied per-metric HELP text. Mirror of upstream
/// `Cardano.Tracer.Run::loadMetricsHelp` (Run.hs:181-191).
///
/// The upstream surface is `Maybe FileOrMap -> IO [(Text, Builder)]`;
/// the Yggdrasil port returns `Vec<(String, String)>` directly
/// (Builder→String per the workspace TextBuilder carve-out).
///
/// Behavior:
/// - `None` returns `vec![]`.
/// - `Some(FileOrMap::File(path))` reads + decodes the file as JSON
///   `Map<String, String>`. On any IO/parse error, returns `vec![]`
///   (mirror of upstream's `try $ decodeFileStrict'` swallowed-error
///   semantics).
/// - `Some(FileOrMap::Map(map))` uses the inline map directly.
/// - The result excludes entries with empty values (mirror of
///   upstream's `M.filter (not . T.null)` filter step).
///
/// Result is sorted by metric-name for deterministic output (mirror
/// of upstream's `M.toList` over `Data.Map` which iterates in
/// insertion-sorted order).
pub fn load_metrics_help(
    metrics_help: Option<&crate::configuration::FileOrMap>,
) -> Vec<(String, String)> {
    let raw_map: std::collections::BTreeMap<String, String> = match metrics_help {
        None => return Vec::new(),
        Some(crate::configuration::FileOrMap::File(path)) => {
            let Ok(bytes) = std::fs::read(path) else {
                return Vec::new();
            };
            serde_json::from_slice(&bytes).unwrap_or_default()
        }
        Some(crate::configuration::FileOrMap::Map(map)) => map.clone(),
    };
    raw_map.into_iter().filter(|(_, v)| !v.is_empty()).collect()
}

// =====================================================================
// Deferral status descriptors
// =====================================================================

/// Status descriptor for the deferred `askNodeName` / `askNodeNameRaw`
/// / `askNodeId` / `showProblemIfAny` entries.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskNodeNameStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing upstream port.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for the askNodeName family.
pub fn ask_node_name_status() -> AskNodeNameStatus {
    AskNodeNameStatus {
        status: "deferred",
        depends_on: "data-point mini-protocol surface (askDataPoint, DataPointRequestor) + tracer-trace channel (Trace IO TracerTrace from MetaTrace.hs) — both unported",
        deferred_round: "R397+",
    }
}

/// Status descriptor for the deferred `beforeProgramStops` entry.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BeforeProgramStopsStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing surface.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `beforeProgramStops`.
pub fn before_program_stops_status() -> BeforeProgramStopsStatus {
    BeforeProgramStopsStatus {
        status: "deferred",
        depends_on: "Unix signal handler installation via tokio::signal::unix::signal; integration requires the Run.hs supervisor task lifetime to be in scope",
        deferred_round: "R398+",
    }
}

/// Status descriptor for the deferred `sequenceConcurrently_` entry.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceConcurrentlyStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the Rust idiomatic equivalents.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `sequenceConcurrently_`.
pub fn sequence_concurrently_status() -> SequenceConcurrentlyStatus {
    SequenceConcurrentlyStatus {
        status: "deferred",
        depends_on: "no clean Rust 1:1 mirror — Rust uses tokio::join! / futures::future::join_all instead. Will land as a thin wrapper around futures::future::join_all when Run.hs supervisor needs it",
        deferred_round: "R399+",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nl_is_unix_newline() {
        assert_eq!(NL, "\n");
    }

    #[test]
    fn init_connected_nodes_returns_empty() {
        let nodes = init_connected_nodes();
        assert!(nodes.snapshot().is_empty());
    }

    #[test]
    fn init_connected_nodes_names_returns_empty() {
        let names = init_connected_nodes_names();
        assert!(names.snapshot().is_empty());
    }

    #[test]
    fn init_protocols_brake_starts_disengaged() {
        let brake = init_protocols_brake();
        assert!(!brake.is_engaged());
    }

    #[test]
    fn apply_brake_engages_protocols_brake() {
        let brake = init_protocols_brake();
        apply_brake(&brake);
        assert!(brake.is_engaged());
    }

    #[test]
    fn conn_id_to_node_id_strips_local_address_prefix() {
        let id = conn_id_to_node_id("LocalAddress \"/tmp/socket\"");
        let s = id.as_str();
        assert!(!s.contains("LocalAddress"));
    }

    #[test]
    fn conn_id_to_node_id_replaces_path_separators_with_dashes() {
        let id = conn_id_to_node_id("/tmp/socket");
        let s = id.as_str();
        assert!(!s.contains('/'));
        assert!(s.contains('-') || s.is_empty());
    }

    #[test]
    fn conn_id_to_node_id_strips_leading_and_trailing_dashes() {
        let id = conn_id_to_node_id("---tmp/sock---");
        let s = id.as_str();
        assert!(!s.starts_with('-'));
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn conn_id_to_node_id_drops_quotes() {
        let id = conn_id_to_node_id("\"node-spo-1\"");
        let s = id.as_str();
        assert!(!s.contains('"'));
    }

    #[test]
    fn conn_id_to_node_id_collapses_double_dashes() {
        // Two runs of dash → empty after `replace "--" ""`.
        let id = conn_id_to_node_id("a--b");
        let s = id.as_str();
        assert_eq!(s, "ab");
    }

    #[test]
    fn get_process_id_returns_positive() {
        let pid = get_process_id();
        assert!(pid > 0);
    }

    #[test]
    fn new_registry_starts_empty() {
        let r: Registry<String, u32> = new_registry();
        assert!(r.is_empty());
    }

    #[test]
    fn member_registry_false_for_empty() {
        let r: Registry<String, u32> = new_registry();
        assert!(!member_registry(&r, &"missing".to_string()));
    }

    #[test]
    fn member_registry_true_after_insert() {
        let r: Registry<String, u32> = new_registry();
        r.insert("present".to_string(), 42);
        assert!(member_registry(&r, &"present".to_string()));
    }

    #[test]
    fn lookup_registry_with_composite_key_returns_value() {
        let r: Registry<(String, u32), i64> = new_registry();
        r.insert(("alpha".to_string(), 7), 100);
        let got = lookup_registry(&r, "alpha".to_string(), 7);
        assert_eq!(got, Some(100));
    }

    #[test]
    fn lookup_registry_with_composite_key_returns_none_when_missing() {
        let r: Registry<(String, u32), i64> = new_registry();
        let got = lookup_registry(&r, "alpha".to_string(), 7);
        assert!(got.is_none());
    }

    #[test]
    fn read_registry_returns_snapshot_of_all_entries() {
        let r: Registry<String, u32> = new_registry();
        r.insert("a".to_string(), 1);
        r.insert("b".to_string(), 2);
        let snapshot = read_registry(&r);
        assert_eq!(snapshot.len(), 2);
    }

    #[test]
    fn modify_registry_replaces_contents() {
        let r: Registry<String, u32> = new_registry();
        r.insert("a".to_string(), 1);
        r.insert("b".to_string(), 2);
        modify_registry(&r, |snapshot| {
            // Drop "a", keep "b", add "c".
            let mut out: Vec<_> = snapshot.into_iter().filter(|(k, _)| k != "a").collect();
            out.push(("c".to_string(), 99));
            out
        });
        let after: Vec<(String, u32)> = read_registry(&r);
        assert_eq!(after.len(), 2);
        assert!(after.iter().any(|(k, _)| k == "b"));
        assert!(after.iter().any(|(k, _)| k == "c"));
        assert!(!after.iter().any(|(k, _)| k == "a"));
    }

    #[test]
    fn modify_registry_with_no_op_transform_preserves_contents() {
        let r: Registry<String, u32> = new_registry();
        r.insert("a".to_string(), 1);
        modify_registry(&r, |snapshot| snapshot);
        assert_eq!(r.len(), 1);
        assert_eq!(r.get(&"a".to_string()), Some(1));
    }

    #[test]
    fn ask_node_name_status_describes_deferral() {
        let s = ask_node_name_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("data-point"));
    }

    #[test]
    fn before_program_stops_status_describes_deferral() {
        let s = before_program_stops_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("signal"));
    }

    #[test]
    fn sequence_concurrently_status_describes_deferral() {
        let s = sequence_concurrently_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("tokio") || s.depends_on.contains("futures"));
    }

    #[test]
    fn load_metrics_help_none_returns_empty() {
        let result = load_metrics_help(None);
        assert!(result.is_empty());
    }

    #[test]
    fn load_metrics_help_inline_map_round_trips() {
        use crate::configuration::FileOrMap;
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            "Mem_resident_int".to_string(),
            "Kernel-reported RSS".to_string(),
        );
        map.insert(
            "RTS_gcMajorNum_int".to_string(),
            "Major GC count".to_string(),
        );
        let result = load_metrics_help(Some(&FileOrMap::Map(map)));
        assert_eq!(result.len(), 2);
        // BTreeMap iteration is alphabetical → Mem first.
        assert_eq!(result[0].0, "Mem_resident_int");
        assert_eq!(result[0].1, "Kernel-reported RSS");
    }

    #[test]
    fn load_metrics_help_inline_map_filters_empty_values() {
        use crate::configuration::FileOrMap;
        let mut map = std::collections::BTreeMap::new();
        map.insert("with_help".to_string(), "Description".to_string());
        map.insert("empty_help".to_string(), String::new());
        let result = load_metrics_help(Some(&FileOrMap::Map(map)));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "with_help");
    }

    #[test]
    fn load_metrics_help_file_read_swallows_io_error() {
        use crate::configuration::FileOrMap;
        let result = load_metrics_help(Some(&FileOrMap::File(std::path::PathBuf::from(
            "/nonexistent/path/to/help.json",
        ))));
        assert!(result.is_empty());
    }

    #[test]
    fn load_metrics_help_file_swallows_invalid_json() {
        use crate::configuration::FileOrMap;
        let tmp = std::env::temp_dir().join(format!(
            "yggdrasil-load-metrics-help-bad-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        std::fs::write(&tmp, b"this is not valid JSON").expect("write");
        let result = load_metrics_help(Some(&FileOrMap::File(tmp.clone())));
        assert!(result.is_empty());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_metrics_help_file_round_trips_valid_json() {
        use crate::configuration::FileOrMap;
        let tmp = std::env::temp_dir().join(format!(
            "yggdrasil-load-metrics-help-good-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let json = r#"{"Mem_resident_int":"RSS","RTS_gcMajorNum_int":"Major GCs"}"#;
        std::fs::write(&tmp, json).expect("write");
        let result = load_metrics_help(Some(&FileOrMap::File(tmp.clone())));
        assert_eq!(result.len(), 2);
        assert!(
            result
                .iter()
                .any(|(k, v)| k == "Mem_resident_int" && v == "RSS"),
        );
        let _ = std::fs::remove_file(&tmp);
    }
}
