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

use std::sync::Arc;

use crate::configuration::{LogMode, LoggingParams};
use crate::logging::TraceObject;
use crate::types::{HandleRegistry, NodeName};

use super::file::{prepare_lines, write_trace_objects_to_file};
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
    /// Routed to the file sink — bytes successfully appended.
    /// R462 closure of the previously-deferred file-write IO
    /// orchestration. The handle was either looked up from the
    /// shared [`HandleRegistry`] or freshly minted via
    /// [`super::utils::create_or_update_empty_log`] (and registered)
    /// before the append.
    FileWritten {
        /// The logging-params slice that matched.
        params: LoggingParams,
        /// Number of trace objects routed.
        count: usize,
        /// Number of bytes appended to the file (including the
        /// leading newline). Matches the return value of
        /// [`super::file::write_trace_objects_to_file`].
        written_bytes: usize,
    },
    /// Routed to the file sink, but the file-write failed. The
    /// caller should log + carry on (matching upstream's
    /// `showProblemIfAny` swallow-and-continue semantics).
    FileError {
        /// The logging-params slice that matched.
        params: LoggingParams,
        /// Human-readable error.
        message: String,
    },
    /// Routed to the file sink using the pure line-encoder only —
    /// no registry was supplied, so the IO orchestration was
    /// skipped. Used by the registry-less
    /// [`trace_objects_handler`] entry-point for backward
    /// compatibility with existing call sites.
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
///
/// **Registry-less variant**: this entry point does not have access
/// to the supervisor's [`HandleRegistry`] and so cannot actually
/// write file-mode entries to disk — it produces
/// [`DispatchOutcome::FilePending`] outcomes for them. Production
/// call sites should use
/// [`trace_objects_handler_with_registry`] (R462 closure), which
/// writes + registers handles.
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
                // Registry-less: produce the line-encoded bytes for
                // sites that just want the byte-count without doing
                // the actual write.
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

/// Production trace-objects dispatcher: routes incoming objects to
/// the appropriate per-LoggingParams sink, **writing file-mode
/// entries to disk** by looking up (or minting) handles in the
/// supervisor's shared [`HandleRegistry`]. Mirror of upstream
/// `traceObjectsHandler` with the full IO orchestration wired.
///
/// R462 closure: the previously-deferred file-write path now ships.
/// The supervisor's [`HandleRegistry`] is the source of truth for
/// open log file descriptors — the rotator (R461) inspects the
/// same registry to roll over full / aged-out files.
///
/// Returns one [`DispatchOutcome`] per LoggingParams entry, in
/// upstream's iteration order. File-mode writes return
/// [`DispatchOutcome::FileWritten`] on success or
/// [`DispatchOutcome::FileError`] on a transport failure
/// (matching upstream's `showProblemIfAny` swallow-and-continue).
pub async fn trace_objects_handler_with_registry(
    node_name: &NodeName,
    logging_params: &[LoggingParams],
    trace_objects: &[TraceObject],
    registry: &HandleRegistry,
    current_log_lock: &Arc<tokio::sync::Mutex<()>>,
) -> Vec<DispatchOutcome> {
    if trace_objects.is_empty() {
        return vec![DispatchOutcome::Skipped];
    }
    let mut outcomes = Vec::with_capacity(logging_params.len());
    for params in logging_params {
        match params.mode {
            LogMode::JournalMode => {
                let _ = write_trace_objects_to_journal(params.format, node_name, trace_objects);
                outcomes.push(DispatchOutcome::Journal {
                    params: params.clone(),
                    count: trace_objects.len(),
                });
            }
            LogMode::FileMode => {
                match write_trace_objects_to_file(
                    registry,
                    params,
                    node_name,
                    current_log_lock,
                    trace_objects,
                )
                .await
                {
                    Ok(written_bytes) => {
                        outcomes.push(DispatchOutcome::FileWritten {
                            params: params.clone(),
                            count: trace_objects.len(),
                            written_bytes,
                        });
                    }
                    Err(e) => {
                        outcomes.push(DispatchOutcome::FileError {
                            params: params.clone(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
    }
    outcomes
}

/// Status descriptor for `deregisterNodeId`.
///
/// R465 closure: the per-connection HandleRegistry teardown hook
/// is now wired into the Acceptors `remove_disconnected_node_with_registry`
/// finalizer. On forwarder disconnect (graceful MsgDone or
/// transport error), the supervisor's shared HandleRegistry is
/// scanned for any (node_name, LoggingParams) keys matching the
/// disconnecting forwarder, and the entries are removed —
/// dropping the SharedLogFile Arcs which closes the underlying
/// file descriptors. No more leaked entries between disconnect +
/// reconnect.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DeregisterNodeIdStatus {
    /// One-line summary.
    pub status: &'static str,
    /// Reason — describes the closed state.
    pub depends_on: &'static str,
    /// Round-number marker.
    pub deferred_round: &'static str,
}

/// Get the status descriptor for `deregisterNodeId`. R465 closed
/// the per-connection HandleRegistry teardown hook; the function
/// now ships via `acceptors::utils::remove_disconnected_node_with_registry`.
pub fn deregister_node_id_status() -> DeregisterNodeIdStatus {
    DeregisterNodeIdStatus {
        status: "closed at R465",
        depends_on: "acceptors::utils::remove_disconnected_node_with_registry scans the supervisor-shared HandleRegistry for matching (node_name, LoggingParams) keys and removes them on disconnect. AcceptorsServerState gained a handle_registry field plumbing the registry through the per-connection spawn body. SharedLogFile Arcs drop on Registry::remove which closes the underlying FDs.",
        deferred_round: "(closed)",
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
    fn deregister_node_id_status_describes_closure() {
        let s = deregister_node_id_status();
        assert_eq!(s.status, "closed at R465");
        assert!(
            s.depends_on
                .contains("remove_disconnected_node_with_registry")
        );
        assert_eq!(s.deferred_round, "(closed)");
    }

    #[test]
    fn dispatch_outcome_skipped_equals_skipped() {
        assert_eq!(DispatchOutcome::Skipped, DispatchOutcome::Skipped);
    }

    // ----- Registry-aware dispatcher tests (R462) -------------------------

    #[tokio::test]
    async fn handler_with_registry_writes_file_mode_to_disk() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let registry = HandleRegistry::new();
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let params = lp(
            LogMode::FileMode,
            LogFormat::ForMachine,
            dir.path().to_str().expect("path"),
        );
        let event = sample_event();
        let outcomes = trace_objects_handler_with_registry(
            &"node-rt".to_string(),
            std::slice::from_ref(&params),
            std::slice::from_ref(&event),
            &registry,
            &lock,
        )
        .await;
        assert_eq!(outcomes.len(), 1);
        let written_bytes = match &outcomes[0] {
            DispatchOutcome::FileWritten {
                written_bytes,
                count,
                ..
            } => {
                assert_eq!(*count, 1);
                *written_bytes
            }
            other => panic!("expected FileWritten, got {other:?}"),
        };
        // Registry now has one entry for (node-rt, params).
        assert_eq!(registry.len(), 1);
        // The minted log file exists on disk.
        let node_dir = dir.path().join("node-rt");
        let mut entries = tokio::fs::read_dir(&node_dir).await.expect("read_dir");
        let mut found_log = false;
        while let Some(e) = entries.next_entry().await.expect("next_entry") {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("node-") && name.ends_with(".json") && !name.contains(".sym") {
                found_log = true;
                let metadata = e.metadata().await.expect("metadata");
                assert_eq!(metadata.len(), written_bytes as u64);
            }
        }
        assert!(found_log, "expected a node-*.json log to exist");
    }

    #[tokio::test]
    async fn handler_with_registry_appends_on_second_call() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let registry = HandleRegistry::new();
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let params = lp(
            LogMode::FileMode,
            LogFormat::ForMachine,
            dir.path().to_str().expect("path"),
        );
        let event = sample_event();
        // First batch: mints the handle + writes 1 line.
        let outcomes1 = trace_objects_handler_with_registry(
            &"node-rt2".to_string(),
            std::slice::from_ref(&params),
            std::slice::from_ref(&event),
            &registry,
            &lock,
        )
        .await;
        let first_bytes = match outcomes1[0] {
            DispatchOutcome::FileWritten { written_bytes, .. } => written_bytes,
            _ => panic!("expected FileWritten"),
        };
        // Second batch: reuses the handle + writes 2 more lines.
        let outcomes2 = trace_objects_handler_with_registry(
            &"node-rt2".to_string(),
            std::slice::from_ref(&params),
            &[sample_event(), sample_event()],
            &registry,
            &lock,
        )
        .await;
        let second_bytes = match outcomes2[0] {
            DispatchOutcome::FileWritten {
                written_bytes,
                count,
                ..
            } => {
                assert_eq!(count, 2);
                written_bytes
            }
            _ => panic!("expected FileWritten"),
        };
        // Registry still has exactly 1 entry (handle was reused).
        assert_eq!(registry.len(), 1);
        // The file's total size is first + second (handle appended,
        // didn't truncate).
        let node_dir = dir.path().join("node-rt2");
        let mut entries = tokio::fs::read_dir(&node_dir).await.expect("read_dir");
        while let Some(e) = entries.next_entry().await.expect("next_entry") {
            if e.file_name().to_string_lossy().starts_with("node-")
                && e.file_name().to_string_lossy().ends_with(".json")
            {
                let metadata = e.metadata().await.expect("metadata");
                assert_eq!(metadata.len() as usize, first_bytes + second_bytes);
                return;
            }
        }
        panic!("expected node-*.json log");
    }

    #[tokio::test]
    async fn handler_with_registry_journal_outcome_unchanged() {
        let registry = HandleRegistry::new();
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let outcomes = trace_objects_handler_with_registry(
            &"node-rt3".to_string(),
            &[lp(LogMode::JournalMode, LogFormat::ForMachine, "/tmp")],
            &[sample_event()],
            &registry,
            &lock,
        )
        .await;
        match &outcomes[0] {
            DispatchOutcome::Journal { count, .. } => assert_eq!(*count, 1),
            other => panic!("expected Journal, got {other:?}"),
        }
        // No registry entry for journal mode (only file mode writes
        // register handles).
        assert!(registry.is_empty());
    }
}
