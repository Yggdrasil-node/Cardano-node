//! Trace-field builders for runtime sync events.
//!
//! Mirrors upstream `Cardano.Node.Tracing.Tracers.NodeToNode.*` —
//! the data side of operational tracing for ChainSync sessions, batch
//! application progress, and reconnect events. Each builder returns
//! a `BTreeMap<String, Value>` of structured key-value fields that
//! the runtime's `NodeTracer` emits via `trace_runtime(namespace,
//! severity, message, fields)`.
//!
//! Four builders covering the runtime's sync-event taxonomy:
//! - `peer_point_trace_fields(peer_addr, current_point)` — base
//!   bundle (peer + currentPoint) reused by most sync events.
//! - `session_established_trace_fields(peer_addr, reconnect_count, from_point)`
//!   — emitted on the first `MsgFindIntersect` of a fresh session.
//! - `sync_error_trace_fields(peer_addr, error, current_point)` —
//!   error-side bundle that adds an `error` string field.
//! - `verified_sync_batch_trace_fields(peer_addr, current_point,
//!   progress, run_state, extras)` — the largest bundle, captures
//!   batch-application progress (slot, blocks fetched, rollbacks, era
//!   distribution, GD density).
//!
//! Extracted from `runtime.rs` in R271i (revised) as the prelude for
//! subsequent extractions of `ReconnectingRunState` and the
//! orchestration async fns.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side trace-field builders
//! aggregating the data side of upstream
//! `Cardano.Node.Tracing.Tracers.NodeToNode.*` (ChainSync session,
//! batch application progress, reconnect events). Upstream
//! spreads field-builder logic across multiple `Tracers/*.hs`
//! files; Yggdrasil unifies the sync-event taxonomy into one
//! module of `BTreeMap<String, Value>` builders consumed by
//! `NodeTracer::trace_runtime`.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use serde_json::{Value, json};
use yggdrasil_ledger::Point;

use yggdrasil_node_sync::MultiEraSyncProgress;
use yggdrasil_node_tracer::trace_fields;

use super::{BatchTraceExtras, ReconnectingRunState};

pub(super) fn peer_point_trace_fields(
    peer_addr: SocketAddr,
    current_point: Point,
) -> BTreeMap<String, Value> {
    trace_fields([
        ("peer", json!(peer_addr.to_string())),
        ("currentPoint", json!(format!("{:?}", current_point))),
    ])
}

pub(super) fn session_established_trace_fields(
    peer_addr: SocketAddr,
    reconnect_count: usize,
    from_point: Point,
) -> BTreeMap<String, Value> {
    trace_fields([
        ("peer", json!(peer_addr.to_string())),
        ("reconnectCount", json!(reconnect_count)),
        ("fromPoint", json!(format!("{:?}", from_point))),
    ])
}

pub(super) fn sync_error_trace_fields(
    peer_addr: SocketAddr,
    error: &impl ToString,
    current_point: Point,
) -> BTreeMap<String, Value> {
    let mut fields = peer_point_trace_fields(peer_addr, current_point);
    fields.insert("error".to_owned(), json!(error.to_string()));
    fields
}

pub(super) fn verified_sync_batch_trace_fields(
    peer_addr: SocketAddr,
    current_point: Point,
    progress: &MultiEraSyncProgress,
    run_state: &ReconnectingRunState,
    extras: BatchTraceExtras,
) -> BTreeMap<String, Value> {
    let mut fields = peer_point_trace_fields(peer_addr, current_point);
    fields.insert(
        "batchFetchedBlocks".to_owned(),
        json!(progress.fetched_blocks),
    );
    fields.insert("batchRollbacks".to_owned(), json!(progress.rollback_count));
    fields.insert("totalBlocks".to_owned(), json!(run_state.total_blocks));
    fields.insert(
        "batchesCompleted".to_owned(),
        json!(run_state.batches_completed),
    );
    if let Some(stable_block_count) = extras.stable_block_count {
        fields.insert("stableBlocks".to_owned(), json!(stable_block_count));
    }
    if let Some(checkpoint_tracked) = extras.checkpoint_tracked {
        fields.insert("checkpointTracked".to_owned(), json!(checkpoint_tracked));
    }

    fields
}
