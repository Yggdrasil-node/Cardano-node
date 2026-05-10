//! Log-file writer — appends [`TraceObject`]s to per-node log files
//! managed by the rotator.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/File.hs.
//!
//! Direct port of upstream's bounded subset. The pure converters +
//! line-encoders ship now. The IO orchestration entry-point
//! `writeTraceObjectsToFile` defers pending the
//! [`super::utils::log_rotation_status`] carve-out's resolution at
//! R402 (createOrUpdateEmptyLog).
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `traceTextForHuman :: TraceObject -> Text`                     | [`trace_text_for_human`]               |
//! | `traceTextForMachine :: TraceObject -> Text`                   | [`trace_text_for_machine`]             |
//! | `writeTraceObjectsToFile` line-encoding subset                 | [`prepare_lines`]                      |
//! | `writeTraceObjectsToFile` IO orchestration                     | (deferred — see [`write_trace_objects_to_file_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`writeTraceObjectsToFile` IO orchestration**: depends on
//!   [`super::utils::log_rotation_status`] (createOrUpdateEmptyLog)
//!   which is itself deferred pending the modifyRegistry_ port. The
//!   pure subset (line-encoding) ships now so downstream sites can
//!   test the conversion logic; the actual file-writing wires up
//!   when R402 lands.
//! - **`Cardano.Tracer.Utils.nl`**: replaced with [`crate::utils::NL`]
//!   (`"\n"` Unix; matches upstream Unix-only operational
//!   convention).

use crate::configuration::LogFormat;
use crate::logging::TraceObject;
use crate::utils::NL;

/// Render a single [`TraceObject`] for human-friendly output.
/// Mirror of upstream `traceTextForHuman`. Uses
/// [`TraceObject::render_for_human`] which falls back to
/// `to_machine` when `to_human` is `None`.
pub fn trace_text_for_human(trace_object: &TraceObject) -> &str {
    trace_object.render_for_human()
}

/// Render a single [`TraceObject`] for machine-readable output.
/// Mirror of upstream `traceTextForMachine`. Always returns
/// `to_machine`.
pub fn trace_text_for_machine(trace_object: &TraceObject) -> &str {
    trace_object.render_for_machine()
}

/// Build the byte payload that would be appended to the log file
/// for a list of trace objects, given the format. Mirror of
/// upstream's
/// `preparedLines = TE.encodeUtf8 (nl `T.append` T.intercalate nl itemsToWrite)`.
///
/// The payload starts with a leading newline (matches upstream's
/// `nl `T.append` ...` semantics — preserves separation from any
/// previously-written line in the file).
///
/// Returns an empty `Vec<u8>` for an empty input slice (mirror of
/// upstream's `unless (null itemsToWrite) do { ... }` guard — sites
/// can use the empty result as a "nothing to write" signal).
pub fn prepare_lines(format: LogFormat, trace_objects: &[TraceObject]) -> Vec<u8> {
    if trace_objects.is_empty() {
        return Vec::new();
    }
    let converter: fn(&TraceObject) -> &str = match format {
        LogFormat::ForHuman => trace_text_for_human,
        LogFormat::ForMachine => trace_text_for_machine,
    };
    let lines: Vec<&str> = trace_objects.iter().map(converter).collect();
    let joined = lines.join(NL);
    let mut out = String::with_capacity(NL.len() + joined.len());
    out.push_str(NL);
    out.push_str(&joined);
    out.into_bytes()
}

/// Status descriptor for the deferred `writeTraceObjectsToFile`
/// orchestration entry-point.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct WriteTraceObjectsToFileStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing dependency.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `writeTraceObjectsToFile`.
pub fn write_trace_objects_to_file_status() -> WriteTraceObjectsToFileStatus {
    WriteTraceObjectsToFileStatus {
        status: "deferred",
        depends_on: "super::utils::log_rotation_status carve-out (createOrUpdateEmptyLog) — itself blocked on Cardano.Tracer.Utils.modifyRegistry_; resolves at R402",
        deferred_round: "R402",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::severity::SeverityS;

    fn sample_with_human() -> TraceObject {
        TraceObject::new(
            Some("BlockFetch acquired block".to_string()),
            r#"{"event":"BlockFetchAcquired"}"#.to_string(),
            SeverityS::Info,
            vec!["BlockFetch".to_string()],
            "tokio-task-1".to_string(),
            1_700_000_000_000,
        )
    }

    fn sample_machine_only() -> TraceObject {
        TraceObject::new(
            None,
            r#"{"event":"raw"}"#.to_string(),
            SeverityS::Debug,
            vec!["Raw".to_string()],
            "tokio-task-2".to_string(),
            1_700_000_001_000,
        )
    }

    #[test]
    fn trace_text_for_human_uses_to_human_when_present() {
        assert_eq!(
            trace_text_for_human(&sample_with_human()),
            "BlockFetch acquired block",
        );
    }

    #[test]
    fn trace_text_for_human_falls_back_to_machine_when_none() {
        assert_eq!(
            trace_text_for_human(&sample_machine_only()),
            r#"{"event":"raw"}"#
        );
    }

    #[test]
    fn trace_text_for_machine_always_returns_machine() {
        assert_eq!(
            trace_text_for_machine(&sample_with_human()),
            r#"{"event":"BlockFetchAcquired"}"#,
        );
        assert_eq!(
            trace_text_for_machine(&sample_machine_only()),
            r#"{"event":"raw"}"#
        );
    }

    #[test]
    fn prepare_lines_returns_empty_for_empty_input() {
        let payload = prepare_lines(LogFormat::ForMachine, &[]);
        assert!(payload.is_empty());
    }

    #[test]
    fn prepare_lines_human_starts_with_newline() {
        let payload = prepare_lines(LogFormat::ForHuman, &[sample_with_human()]);
        assert_eq!(&payload[0..1], b"\n");
    }

    #[test]
    fn prepare_lines_human_renders_to_human_text() {
        let payload = prepare_lines(LogFormat::ForHuman, &[sample_with_human()]);
        let s = String::from_utf8(payload).expect("utf8");
        assert!(s.contains("BlockFetch acquired block"));
        assert!(!s.contains("BlockFetchAcquired"));
    }

    #[test]
    fn prepare_lines_machine_renders_to_machine_text() {
        let payload = prepare_lines(LogFormat::ForMachine, &[sample_with_human()]);
        let s = String::from_utf8(payload).expect("utf8");
        assert!(s.contains("BlockFetchAcquired"));
        // human-readable text should NOT appear in machine output
        assert!(!s.contains("BlockFetch acquired block"));
    }

    #[test]
    fn prepare_lines_intercalates_with_newline() {
        let payload = prepare_lines(
            LogFormat::ForMachine,
            &[sample_with_human(), sample_machine_only()],
        );
        let s = String::from_utf8(payload).expect("utf8");
        // Expected: "\n" + "machine1" + "\n" + "machine2"
        assert!(s.starts_with('\n'));
        let inner = &s[1..]; // drop leading \n
        let parts: Vec<&str> = inner.split('\n').collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("BlockFetchAcquired"));
        assert!(parts[1].contains("raw"));
    }

    #[test]
    fn prepare_lines_human_falls_back_per_object() {
        // Mix: one with human text, one without.
        let payload = prepare_lines(
            LogFormat::ForHuman,
            &[sample_with_human(), sample_machine_only()],
        );
        let s = String::from_utf8(payload).expect("utf8");
        // First entry has human text.
        assert!(s.contains("BlockFetch acquired block"));
        // Second entry falls back to its machine text.
        assert!(s.contains("raw"));
    }

    #[test]
    fn prepare_lines_single_event_round_trip() {
        let payload = prepare_lines(LogFormat::ForMachine, &[sample_machine_only()]);
        let s = String::from_utf8(payload).expect("utf8");
        // \n + "{"event":"raw"}" — exactly 2 + 16 chars (1 \n + 15 char body).
        assert_eq!(s, "\n{\"event\":\"raw\"}");
    }

    #[test]
    fn prepare_lines_handles_empty_to_machine() {
        let event = TraceObject {
            to_machine: String::new(),
            ..TraceObject::default()
        };
        let payload = prepare_lines(LogFormat::ForMachine, &[event]);
        // Should still produce a leading newline + empty body.
        assert_eq!(payload, b"\n");
    }

    #[test]
    fn write_trace_objects_to_file_status_describes_deferral() {
        let s = write_trace_objects_to_file_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("createOrUpdateEmptyLog"));
        assert_eq!(s.deferred_round, "R402");
    }
}
