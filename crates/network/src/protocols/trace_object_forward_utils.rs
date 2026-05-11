//! Helpers for the trace-forwarder TraceObject mini-protocol —
//! sink initialization, write-side overflow handling, and the
//! reply-list extractor consumed by the acceptor side.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Utils/TraceObject.hs.
//!
//! Mirror of upstream's bounded subset. R422 landed the acceptor-
//! side entry points (`init_forward_sink`,
//! `get_trace_objects_from_reply`); R467 landed the forwarder-side
//! queue-mutation helpers (`write_to_sink`,
//! `read_from_sink_non_blocking`). The blocking variant of
//! `readFromSink` remains deferred pending the
//! `TraceObjectForwarder` driver port.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                                |
//! |---------------------------------------------------------|------------------------------------------|
//! | `initForwardSink :: ForwarderConfiguration lo -> ([lo] -> IO ()) -> IO (ForwardSink lo)` | [`init_forward_sink`] (R422) |
//! | `getTraceObjectsFromReply :: BlockingReplyList blocking lo -> [lo]` | [`get_trace_objects_from_reply`] (R422) |
//! | `writeToSink :: ForwardSink lo -> lo -> IO ()`          | [`write_to_sink`] (R467)                 |
//! | `writeToSinkSTM :: TBQueue lo -> lo -> STM [lo]`        | (collapsed into [`write_to_sink`] — Yggdrasil's `Arc<Mutex<VecDeque>>` doesn't need an STM transaction wrapper) |
//! | `readFromSinkSTM queue TokNonBlocking n :: STM [lo]`    | [`read_from_sink_non_blocking`] (R467)   |
//! | `readFromSinkSTM queue TokBlocking n :: STM [lo]`       | (still deferred — see [`read_from_sink_status`]) |
//! | `readFromSink :: ForwardSink lo -> Forwarder.TraceObjectForwarder lo IO ()` | (still deferred — needs `TraceObjectForwarder` driver port; see [`read_from_sink_status`]) |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`readFromSink` (full driver) + blocking variant of
//!   `readFromSinkSTM`**: depend on a `TraceObjectForwarder`
//!   driver type + `tokio::sync::Notify`-based wake-on-write
//!   integration. R467 shipped the non-blocking variant which
//!   covers the cardano-node forwarder's poll-loop use case.
//!   Blocking semantics ship alongside a future Forwarder driver
//!   port if needed.
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

/// Push a trace object into the forwarder-side sink. Mirror of
/// upstream `writeToSink :: ForwardSink lo -> lo -> IO ()` +
/// `writeToSinkSTM :: TBQueue lo -> lo -> STM [lo]` (collapsed to
/// a single non-STM helper since Yggdrasil's `Arc<Mutex<VecDeque>>`
/// doesn't need the STM transaction).
///
/// Bounded-queue semantics: if the queue is already at `capacity`,
/// the function drains all currently-queued items, fires
/// `sink.overflow_callback` with the drained items, then pushes
/// the new item. This matches upstream's
/// ```haskell
/// isFull <- isFullTBQueue queue
/// !flushedTraceObjects <- if isFull
///                         then flushTBQueue queue
///                         else pure []
/// writeTBQueue queue traceObject
/// ```
/// pattern. Returns the count of flushed items (0 if no overflow).
///
/// `capacity` is supplied by the caller per `ForwarderConfiguration::
/// queue_size`. R467 closure.
pub fn write_to_sink<TraceObj>(
    sink: &ForwardSink<TraceObj>,
    capacity: usize,
    trace_object: TraceObj,
) -> usize {
    let flushed: Vec<TraceObj> = {
        let mut queue = match sink.forward_queue.lock() {
            Ok(q) => q,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut flushed = Vec::new();
        if queue.len() >= capacity {
            // Drain everything (matches upstream's `flushTBQueue`).
            flushed.extend(queue.drain(..));
        }
        queue.push_back(trace_object);
        flushed
    };
    let count = flushed.len();
    if !flushed.is_empty() {
        // Fire the overflow callback OUTSIDE the lock (mirror of
        // upstream's "case flushedTraceObjects of ... overflowCallback
        // flushedTraceObjects" pattern, which runs outside
        // `atomically`).
        (sink.overflow_callback)(flushed);
    }
    count
}

/// Drain up to `n` items from the sink, non-blocking. Mirror of
/// upstream `readFromSinkSTM queue TokNonBlocking n` — returns
/// empty if the queue is empty.
///
/// If `n >= queue.len()`, drains everything (matches upstream's
/// `if fromEnum n >= fromEnum queueLength then flushTBQueue queue`
/// branch); otherwise drains exactly `n` items from the front.
/// R467 closure.
pub fn read_from_sink_non_blocking<TraceObj>(
    sink: &ForwardSink<TraceObj>,
    n: usize,
) -> Vec<TraceObj> {
    let mut queue = match sink.forward_queue.lock() {
        Ok(q) => q,
        Err(poisoned) => poisoned.into_inner(),
    };
    let take = n.min(queue.len());
    queue.drain(..take).collect()
}

/// Status descriptor for `writeToSink` / `writeToSinkSTM`. R467
/// closure — the function now ships as [`write_to_sink`].
pub fn write_to_sink_status() -> &'static str {
    "writeToSink / writeToSinkSTM: closed at R467. The forwarder-side \
     queue mutation now ships as write_to_sink(sink, capacity, item) — \
     a non-STM port that mirrors upstream's flush-on-full + push-new \
     semantics via Arc<Mutex<VecDeque>> + the existing overflow_callback. \
     Yggdrasil-side cardano-node forwarder consumers can call this \
     directly."
}

/// Status descriptor for `readFromSink` / `readFromSinkSTM`. R467
/// closure — the non-blocking drain ships as
/// [`read_from_sink_non_blocking`]. The blocking variant
/// (`TokBlocking` arm) remains deferred pending the
/// `TraceObjectForwarder` driver port (which needs the
/// `tokio::sync::Notify`-based blocking semantics + the forwarder-
/// side mux entry point); operationally the non-blocking variant
/// covers the standard cardano-node forwarder lo_handler call site
/// where the forwarder polls.
pub fn read_from_sink_status() -> &'static str {
    "readFromSink / readFromSinkSTM: non-blocking variant closed at \
     R467 as read_from_sink_non_blocking(sink, n). The blocking \
     variant remains deferred pending the TraceObjectForwarder \
     driver port + tokio::sync::Notify wake-on-write integration; \
     operationally cardano-node forwarders use the non-blocking \
     variant in a poll loop, which is fully supported."
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
    fn write_to_sink_status_describes_closure() {
        let s = write_to_sink_status();
        assert!(s.contains("closed at R467"));
        assert!(s.contains("write_to_sink"));
    }

    #[test]
    fn read_from_sink_status_describes_closure() {
        let s = read_from_sink_status();
        assert!(s.contains("closed at R467"));
        assert!(s.contains("read_from_sink_non_blocking"));
    }

    // ----- R467 write_to_sink + read_from_sink tests ---------------------

    #[test]
    fn write_to_sink_below_capacity_pushes_no_flush() {
        let cb: ForwardSinkOverflowCallback<TestPayload> =
            Arc::new(|_| panic!("overflow_callback should not fire when under capacity"));
        let sink: ForwardSink<TestPayload> = ForwardSink::new(cb);
        let flushed = write_to_sink(&sink, 10, TestPayload(1));
        assert_eq!(flushed, 0);
        assert_eq!(sink.queue_len(), 1);
    }

    #[test]
    fn write_to_sink_at_capacity_flushes_then_pushes() {
        let flushed_count = Arc::new(std::sync::Mutex::new(0u32));
        let flushed_clone = Arc::clone(&flushed_count);
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(move |v| {
            *flushed_clone.lock().expect("lock") = v.len() as u32;
        });
        let sink: ForwardSink<TestPayload> = ForwardSink::new(cb);
        // Fill to capacity = 3.
        for i in 0..3u32 {
            write_to_sink(&sink, 3, TestPayload(i));
        }
        assert_eq!(sink.queue_len(), 3);
        // The 4th push triggers overflow: flush all 3 + push 1 new.
        let flushed = write_to_sink(&sink, 3, TestPayload(99));
        assert_eq!(flushed, 3, "should have flushed 3 items");
        assert_eq!(sink.queue_len(), 1, "only the new item remains");
        // The overflow callback received the flushed 3.
        assert_eq!(*flushed_count.lock().expect("lock"), 3);
    }

    #[test]
    fn read_from_sink_non_blocking_drains_up_to_n() {
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(|_| {});
        let sink: ForwardSink<TestPayload> = ForwardSink::new(cb);
        for i in 0..5u32 {
            write_to_sink(&sink, 10, TestPayload(i));
        }
        let drained = read_from_sink_non_blocking(&sink, 3);
        assert_eq!(
            drained,
            vec![TestPayload(0), TestPayload(1), TestPayload(2)]
        );
        assert_eq!(sink.queue_len(), 2);
    }

    #[test]
    fn read_from_sink_non_blocking_n_exceeds_queue_returns_all() {
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(|_| {});
        let sink: ForwardSink<TestPayload> = ForwardSink::new(cb);
        for i in 0..3u32 {
            write_to_sink(&sink, 10, TestPayload(i));
        }
        let drained = read_from_sink_non_blocking(&sink, 100);
        assert_eq!(drained.len(), 3);
        assert_eq!(sink.queue_len(), 0);
    }

    #[test]
    fn read_from_sink_non_blocking_empty_returns_empty() {
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(|_| {});
        let sink: ForwardSink<TestPayload> = ForwardSink::new(cb);
        let drained = read_from_sink_non_blocking(&sink, 5);
        assert!(drained.is_empty());
    }

    #[test]
    fn write_then_read_full_round_trip() {
        let cb: ForwardSinkOverflowCallback<TestPayload> = Arc::new(|_| {});
        let sink: ForwardSink<TestPayload> = ForwardSink::new(cb);
        for i in 0..4u32 {
            write_to_sink(&sink, 10, TestPayload(i));
        }
        let drained = read_from_sink_non_blocking(&sink, 4);
        assert_eq!(
            drained,
            vec![
                TestPayload(0),
                TestPayload(1),
                TestPayload(2),
                TestPayload(3)
            ]
        );
        assert_eq!(sink.queue_len(), 0);
    }
}
