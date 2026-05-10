//! Helpers for the trace-forwarder TraceObject mini-protocol —
//! sink initialization, write-side overflow handling, and the
//! reply-list extractor consumed by the acceptor side.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Utils/TraceObject.hs.
//!
//! Mirror of upstream's bounded subset. Lands the entry points
//! that the R411-R430 acceptor-side path needs first
//! (`getTraceObjectsFromReply`); the forwarder-side
//! `writeToSinkSTM` / `readFromSinkSTM` functions defer pending
//! the cardano-node forwarder port (R424+).
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                                |
//! |---------------------------------------------------------|------------------------------------------|
//! | `initForwardSink :: ForwarderConfiguration lo -> ([lo] -> IO ()) -> IO (ForwardSink lo)` | [`init_forward_sink`] |
//! | `getTraceObjectsFromReply :: BlockingReplyList blocking lo -> [lo]` | [`get_trace_objects_from_reply`] |
//! | `writeToSink :: ForwardSink lo -> lo -> IO ()`          | (deferred — see [`write_to_sink_status`]) |
//! | `writeToSinkSTM :: TBQueue lo -> lo -> STM [lo]`        | (deferred — same)                        |
//! | `readFromSink :: ForwardSink lo -> Forwarder.TraceObjectForwarder lo IO ()` | (deferred — see [`read_from_sink_status`]) |
//! | `readFromSinkSTM :: TBQueue lo -> TokBlockingStyle b -> Word16 -> STM [lo]` | (deferred — same)              |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`writeToSink` / `writeToSinkSTM` / `readFromSink` /
//!   `readFromSinkSTM`**: forwarder-side queue manipulation. The
//!   `TBQueue` STM transactional semantics translate cleanly to
//!   `Arc<Mutex<VecDeque>>` + a CondVar, but the call surface is
//!   only consumed by the cardano-node forwarder side (out of the
//!   R411-R430 cardano-tracer arc scope). Status surfaced
//!   programmatically via [`write_to_sink_status`] +
//!   [`read_from_sink_status`].
//! - **`Cardano.Logging.Utils.tryEvalNF`**: upstream's deep-NF
//!   evaluation guard for trace-object rendering errors. Yggdrasil's
//!   `TraceObject` is `Clone + Eq` and rendering errors don't
//!   surface as Haskell exceptions; the equivalent is a
//!   `Result<TraceObj, RenderError>` returned by render helpers,
//!   not a runtime forced-NF guard.

use super::forward_sink::ForwardSinkOverflowCallback;
use super::{BlockingReplyList, ForwardSink, ForwarderConfiguration};

/// Initialize a forwarder-side sink with the supplied
/// [`ForwarderConfiguration`] + overflow callback. Mirror of
/// upstream's `initForwardSink config callback`.
///
/// The Yggdrasil port honours the operator-supplied
/// `queue_size` from the configuration record (mirror of
/// upstream's `fromIntegral queueSize`) — the
/// `Mutex<VecDeque>` is preallocated with that capacity so the
/// hot path doesn't reallocate on the first burst of trace objects.
pub fn init_forward_sink<TraceObj>(
    config: &ForwarderConfiguration,
    overflow_callback: ForwardSinkOverflowCallback<TraceObj>,
) -> ForwardSink<TraceObj> {
    let sink = ForwardSink::new(overflow_callback);
    // Preallocate the queue capacity per operator's config.
    if let Ok(mut queue) = sink.forward_queue.lock() {
        *queue = std::collections::VecDeque::with_capacity(config.queue_size as usize);
    }
    sink
}

/// Extract the trace-object list from a [`BlockingReplyList`]
/// regardless of blocking style. Mirror of upstream's
/// `getTraceObjectsFromReply :: BlockingReplyList blocking lo -> [lo]`.
///
/// This is the canonical accessor for the acceptor side's
/// reply-handling path (used by
/// `Trace.Forward.Run.TraceObject.Acceptor::acceptorActions`,
/// already wired via R421's `accept_trace_objects_resp`). Yggdrasil
/// callers can equivalently use `BlockingReplyList::into_items()`
/// directly; this free function exists for upstream-naming-parity
/// at the call site.
pub fn get_trace_objects_from_reply<TraceObj>(reply: BlockingReplyList<TraceObj>) -> Vec<TraceObj> {
    reply.into_items()
}

/// Status descriptor for the deferred `writeToSink` /
/// `writeToSinkSTM` forwarder-side helpers. Surfaces the carve-out
/// programmatically so callers can introspect the deferral
/// rationale.
pub fn write_to_sink_status() -> &'static str {
    "writeToSink / writeToSinkSTM (forwarder-side queue mutation): \
     deferred pending cardano-node forwarder port (R424+). \
     Yggdrasil operationally exercises only the acceptor-side path \
     in the R411-R430 arc scope."
}

/// Status descriptor for the deferred `readFromSink` /
/// `readFromSinkSTM` forwarder-side helpers.
pub fn read_from_sink_status() -> &'static str {
    "readFromSink / readFromSinkSTM (forwarder-side queue drain + \
     reply construction): deferred pending cardano-node forwarder \
     port (R424+). Same scope rationale as write_to_sink_status."
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestPayload(u32);

    #[test]
    fn init_forward_sink_creates_empty_queue() {
        let config = ForwarderConfiguration::new(64);
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(|_| {});
        let sink = init_forward_sink::<TestPayload>(&config, cb);
        assert_eq!(sink.queue_len(), 0);
    }

    #[test]
    fn init_forward_sink_preallocates_queue_capacity() {
        let config = ForwarderConfiguration::new(128);
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(|_| {});
        let sink = init_forward_sink::<TestPayload>(&config, cb);
        let q = sink.forward_queue.lock().expect("queue");
        // The VecDeque grows in powers of two; the actual capacity
        // is at least the requested size.
        assert!(q.capacity() >= 128, "got capacity {}", q.capacity());
    }

    #[test]
    fn get_trace_objects_from_reply_unifies_blocking_variant() {
        let reply: BlockingReplyList<TestPayload> =
            BlockingReplyList::blocking(vec![TestPayload(1), TestPayload(2)]).expect("seed");
        let items = get_trace_objects_from_reply(reply);
        assert_eq!(items, vec![TestPayload(1), TestPayload(2)]);
    }

    #[test]
    fn get_trace_objects_from_reply_unifies_non_blocking_variant() {
        let reply: BlockingReplyList<TestPayload> =
            BlockingReplyList::non_blocking(vec![TestPayload(7)]);
        let items = get_trace_objects_from_reply(reply);
        assert_eq!(items, vec![TestPayload(7)]);
    }

    #[test]
    fn get_trace_objects_from_reply_handles_empty_non_blocking() {
        let reply: BlockingReplyList<TestPayload> = BlockingReplyList::non_blocking(vec![]);
        let items = get_trace_objects_from_reply(reply);
        assert!(items.is_empty());
    }

    #[test]
    fn write_to_sink_status_describes_deferral() {
        let s = write_to_sink_status();
        assert!(s.contains("deferred"));
        assert!(s.contains("R424+"));
        assert!(s.contains("writeToSink"));
    }

    #[test]
    fn read_from_sink_status_describes_deferral() {
        let s = read_from_sink_status();
        assert!(s.contains("deferred"));
        assert!(s.contains("R424+"));
        assert!(s.contains("readFromSink"));
    }
}
