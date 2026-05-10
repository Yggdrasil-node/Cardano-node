//! Trace-object dispatcher — routes incoming objects to the
//! appropriate per-LoggingParams sink (file or journal).
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Handlers/Logs/TraceObjects.hs.
//!
//! Direct port of upstream's bounded subset. The dispatcher routing
//! decisions ship now (call into the journal sink at
//! [`super::journal::write_trace_objects_to_journal`] from R382 +
//! the file-sink line-encoder at [`super::file::prepare_lines`]
//! from R400). Two pieces defer:
//!
//! - File-mode write actually happens — depends on the
//!   [`super::utils::log_rotation_status`] carve-out which is itself
//!   pending the modifyRegistry_ port at R402.
//! - `deregisterNodeId` — depends on modifyRegistry_ + System.IO.hClose
//!   on the registry-stored handles (placeholder at R371).
//!
//! Mapping summary:
//!
//! | Upstream                                                       | Yggdrasil                              |
//! |----------------------------------------------------------------|----------------------------------------|
//! | `traceObjectsHandler :: TracerEnv -> TracerEnvRTView -> NodeId -> [TraceObject] -> IO ()` | [`trace_objects_handler`] (returns dispatch outcomes) |
//! | `deregisterNodeId :: TracerEnv -> NodeId -> IO ()`             | (deferred — see [`deregister_node_id_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`TracerEnv`-record-arg + `askNodeName` lookup**: per the R398
//!   plan's TracerEnv option (b) decision, the dispatcher takes a
//!   resolved [`NodeName`] directly rather than looking it up from
//!   `TracerEnv` via the deferred `askNodeName` chain.
//! - **`forConcurrently_` parallel fan-out**: replaced with
//!   sequential per-LoggingParams iteration. The upstream
//!   `Control.Concurrent.Async.forConcurrently_` improves throughput
//!   when log writes contend on disk; sequential dispatch is
//!   acceptable until the file-mode write actually happens (R402+).
//!   Once R402 lands, swap to `tokio::task::join_all` if the soak
//!   shows contention.
//! - **`teReforwardTraceObjects` callback**: invoked at the tail of
//!   upstream's `traceObjectsHandler`. Deferred — depends on the
//!   trace-forwarder mini-protocol acceptors (R411+) being wired.
//! - **`#if RTVIEW saveTraceObjects` arm**: the entire RTView UI
//!   carve-out per the workspace plan; never ported.

use crate::configuration::{LogMode, LoggingParams};
use crate::logging::TraceObject;
use crate::types::NodeName;

use super::file::prepare_lines;
use super::journal::write_trace_objects_to_journal;

/// Outcome of dispatching one batch of trace objects to a single
/// [`LoggingParams`] sink. Each variant corresponds to one of
/// upstream's `forMode` arms.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    /// Routed to the journal sink. On Yggdrasil this is currently a
    /// no-op per the no-FFI policy (see
    /// [`super::journal::no_systemd::write_trace_objects_to_journal`]);
    /// the variant is kept so call sites can track upstream-parity
    /// dispatch counts.
    Journal {
        /// The logging-params slice that matched.
        params: LoggingParams,
        /// Number of trace objects routed.
        count: usize,
    },
    /// Routed to the file sink. The line-encoded bytes are computed
    /// (via [`super::file::prepare_lines`]) but the actual write
    /// operation is **pending R402** — the file orchestration
    /// depends on `createOrUpdateEmptyLog` which is itself blocked
    /// on the modifyRegistry_ port. Sites should track this as a
    /// pending IO commitment.
    FilePending {
        /// The logging-params slice that matched.
        params: LoggingParams,
        /// Number of trace objects routed.
        count: usize,
        /// Number of bytes that would have been appended to the file.
        prepared_bytes: usize,
    },
    /// No dispatch — empty trace-object input. Mirror of upstream's
    /// `traceObjectsHandler _ _ _ [] = return ()` short-circuit.
    Skipped,
}

/// Route incoming trace objects to the appropriate per-LoggingParams
/// sink. Mirror of upstream `traceObjectsHandler`.
///
/// `node_name` is the pre-resolved name (caller is expected to
/// resolve it from a NodeId via the deferred `askNodeName` chain;
/// the R398 plan's TracerEnv option (b) keeps this explicit rather
/// than coupling to TracerEnv).
///
/// Returns one [`DispatchOutcome`] per LoggingParams entry, in
/// upstream's iteration order. Empty trace-object input returns a
/// single [`DispatchOutcome::Skipped`].
pub async fn trace_objects_handler(
    node_name: &NodeName,
    logging_params: &[LoggingParams],
    trace_objects: &[TraceObject],
) -> Vec<DispatchOutcome> {
    if trace_objects.is_empty() {
        return vec![DispatchOutcome::Skipped];
    }

    let mut outcomes = Vec::with_capacity(logging_params.len());
    for params in logging_params {
        match params.mode {
            LogMode::JournalMode => {
                // R382 wired the no-op journal sink; we still call
                // through so the parity-call-graph stays intact when
                // the systemd-binding port lands.
                let _ = write_trace_objects_to_journal(params.format, node_name, trace_objects);
                outcomes.push(DispatchOutcome::Journal {
                    params: params.clone(),
                    count: trace_objects.len(),
                });
            }
            LogMode::FileMode => {
                // R400 wired the line-encoder; the actual write
                // path defers to R402.
                let prepared = prepare_lines(params.format, trace_objects);
                outcomes.push(DispatchOutcome::FilePending {
                    params: params.clone(),
                    count: trace_objects.len(),
                    prepared_bytes: prepared.len(),
                });
            }
        }
    }
    outcomes
}

/// Status descriptor for the deferred `deregisterNodeId` entry.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DeregisterNodeIdStatus {
    /// One-line summary of the deferral.
    pub status: &'static str,
    /// Reason — references the missing upstream port.
    pub depends_on: &'static str,
    /// Round-number marker for tracking the deferred work.
    pub deferred_round: &'static str,
}

/// Get the deferral-status descriptor for `deregisterNodeId`.
pub fn deregister_node_id_status() -> DeregisterNodeIdStatus {
    DeregisterNodeIdStatus {
        status: "deferred",
        depends_on: "Cardano.Tracer.Utils.modifyRegistry_ + System.IO.hClose semantics on registry-stored handles (placeholder at R371); resolves alongside R402's createOrUpdateEmptyLog",
        deferred_round: "R402",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{LogFormat, LogMode, LoggingParams};
    use crate::logging::TraceObject;
    use crate::severity::SeverityS;
    use std::path::PathBuf;

    fn lp(mode: LogMode, format: LogFormat, root: &str) -> LoggingParams {
        LoggingParams {
            root: PathBuf::from(root),
            mode,
            format,
        }
    }

    fn sample_event() -> TraceObject {
        TraceObject::new(
            Some("BlockFetch acquired".to_string()),
            r#"{"e":"acquired"}"#.to_string(),
            SeverityS::Info,
            vec!["BlockFetch".to_string()],
            "tokio-1".to_string(),
            1_700_000_000_000,
        )
    }

    #[tokio::test]
    async fn empty_trace_objects_returns_skipped() {
        let outcomes = trace_objects_handler(
            &"node-1".to_string(),
            &[lp(LogMode::FileMode, LogFormat::ForMachine, "/tmp")],
            &[],
        )
        .await;
        assert_eq!(outcomes, vec![DispatchOutcome::Skipped]);
    }

    #[tokio::test]
    async fn empty_logging_params_with_events_returns_no_outcomes() {
        let outcomes = trace_objects_handler(&"node-1".to_string(), &[], &[sample_event()]).await;
        // No params → empty outcome list (not Skipped, since
        // upstream's null-event guard runs first).
        assert!(outcomes.is_empty());
    }

    #[tokio::test]
    async fn journal_mode_dispatches_to_journal_outcome() {
        let outcomes = trace_objects_handler(
            &"node-1".to_string(),
            &[lp(LogMode::JournalMode, LogFormat::ForMachine, "/tmp")],
            &[sample_event()],
        )
        .await;
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            DispatchOutcome::Journal { count, .. } => assert_eq!(*count, 1),
            other => panic!("expected Journal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_mode_dispatches_to_file_pending_with_prepared_bytes() {
        let event = sample_event();
        let outcomes = trace_objects_handler(
            &"node-1".to_string(),
            &[lp(LogMode::FileMode, LogFormat::ForMachine, "/tmp")],
            std::slice::from_ref(&event),
        )
        .await;
        assert_eq!(outcomes.len(), 1);
        match &outcomes[0] {
            DispatchOutcome::FilePending {
                count,
                prepared_bytes,
                ..
            } => {
                assert_eq!(*count, 1);
                // For ForMachine, the prepared bytes match
                // prepare_lines' output exactly.
                let expected = prepare_lines(LogFormat::ForMachine, std::slice::from_ref(&event));
                assert_eq!(*prepared_bytes, expected.len());
            }
            other => panic!("expected FilePending, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mixed_logging_params_routes_each_independently() {
        let event = sample_event();
        let outcomes = trace_objects_handler(
            &"node-1".to_string(),
            &[
                lp(LogMode::FileMode, LogFormat::ForMachine, "/tmp/m"),
                lp(LogMode::JournalMode, LogFormat::ForHuman, "/tmp/j"),
                lp(LogMode::FileMode, LogFormat::ForHuman, "/tmp/h"),
            ],
            &[event],
        )
        .await;
        assert_eq!(outcomes.len(), 3);
        assert!(matches!(outcomes[0], DispatchOutcome::FilePending { .. }));
        assert!(matches!(outcomes[1], DispatchOutcome::Journal { .. }));
        assert!(matches!(outcomes[2], DispatchOutcome::FilePending { .. }));
    }

    #[tokio::test]
    async fn dispatcher_handles_multi_event_batches() {
        let outcomes = trace_objects_handler(
            &"node-1".to_string(),
            &[lp(LogMode::JournalMode, LogFormat::ForMachine, "/tmp")],
            &[sample_event(), sample_event(), sample_event()],
        )
        .await;
        match &outcomes[0] {
            DispatchOutcome::Journal { count, .. } => assert_eq!(*count, 3),
            other => panic!("expected Journal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_mode_for_machine_vs_for_human_produce_different_byte_counts() {
        let event = TraceObject::new(
            Some("human-text".to_string()),
            r#"{"machine":"longer-payload-than-human-text"}"#.to_string(),
            SeverityS::Info,
            vec!["x".to_string()],
            "t-1".to_string(),
            0,
        );
        let outcomes_machine = trace_objects_handler(
            &"node-1".to_string(),
            &[lp(LogMode::FileMode, LogFormat::ForMachine, "/tmp")],
            std::slice::from_ref(&event),
        )
        .await;
        let outcomes_human = trace_objects_handler(
            &"node-1".to_string(),
            &[lp(LogMode::FileMode, LogFormat::ForHuman, "/tmp")],
            std::slice::from_ref(&event),
        )
        .await;
        let machine_bytes = match &outcomes_machine[0] {
            DispatchOutcome::FilePending { prepared_bytes, .. } => *prepared_bytes,
            _ => panic!("expected FilePending"),
        };
        let human_bytes = match &outcomes_human[0] {
            DispatchOutcome::FilePending { prepared_bytes, .. } => *prepared_bytes,
            _ => panic!("expected FilePending"),
        };
        // Machine payload is longer than human text → bigger.
        assert!(machine_bytes > human_bytes);
    }

    #[test]
    fn deregister_node_id_status_describes_deferral() {
        let s = deregister_node_id_status();
        assert_eq!(s.status, "deferred");
        assert!(s.depends_on.contains("modifyRegistry_"));
        assert_eq!(s.deferred_round, "R402");
    }

    #[test]
    fn dispatch_outcome_skipped_equals_skipped() {
        assert_eq!(DispatchOutcome::Skipped, DispatchOutcome::Skipped);
    }
}
