//! `ForwardSink` — bounded queue + overflow callback used by the
//! forwarder side of the trace-forwarder mini-protocol to buffer
//! outgoing trace objects until the acceptor requests them.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Utils/ForwardSink.hs.
//!
//! Mirror of upstream's 2-field record:
//!
//! ```haskell
//! data ForwardSink lo = ForwardSink
//!   { forwardQueue     :: !(TBQueue lo)
//!   , overflowCallback :: !([lo] -> IO ())
//!   }
//! ```
//!
//! Mapping summary:
//!
//! | Upstream                                          | Yggdrasil                              |
//! |---------------------------------------------------|----------------------------------------|
//! | `forwardQueue :: TBQueue lo`                      | [`ForwardSink::forward_queue`]         |
//! | `overflowCallback :: [lo] -> IO ()`               | [`ForwardSink::overflow_callback`]     |
//!
//! Carve-outs (synthesis-mirror, NOT strict-mirror):
//!
//! - **`Control.Concurrent.STM.TBQueue.TBQueue lo`** (bounded
//!   transactional queue): replaced with a synchronous `Mutex<VecDeque>`
//!   shared via `Arc`. The full bounded-queue semantics + STM-style
//!   blocking-write require the forwarder-side ports of
//!   `writeToSink` / `readFromSink` (R424+ pending). The struct
//!   shape is locked in here so upcoming rounds can fill in the
//!   queue-mutation methods without breaking call sites.
//! - **`overflowCallback :: [lo] -> IO ()`**: replaced with
//!   `Arc<dyn Fn(Vec<TraceObj>) + Send + Sync>`. The Haskell `IO ()`
//!   side-effect signature collapses to a Rust closure trait.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Bounded buffer + overflow notifier used by the forwarder side
/// of the trace-forwarder mini-protocol.
///
/// Generic over the trace-object payload. Operationally `lo` is
/// always `cardano_tracer::logging::TraceObject` (or its DataPoint
/// counterpart for the DataPoint sub-protocol).
pub struct ForwardSink<TraceObj> {
    /// Internal queue of pending trace objects awaiting acceptor
    /// pickup. Mirror of upstream's `forwardQueue :: TBQueue lo`.
    pub forward_queue: Arc<Mutex<VecDeque<TraceObj>>>,
    /// Callback invoked when the queue overflows + objects must be
    /// flushed. Mirror of upstream's
    /// `overflowCallback :: [lo] -> IO ()`.
    pub overflow_callback: ForwardSinkOverflowCallback<TraceObj>,
}

/// Closure type for [`ForwardSink::overflow_callback`]. Factored
/// out as a type alias to dodge clippy's `type_complexity` lint.
pub type ForwardSinkOverflowCallback<TraceObj> = Arc<dyn Fn(Vec<TraceObj>) + Send + Sync>;

impl<TraceObj> ForwardSink<TraceObj> {
    /// Create a new sink with the supplied overflow callback. The
    /// queue starts empty. Operators wanting a no-op overflow
    /// behaviour can pass `Arc::new(|_| ())`.
    ///
    /// This is a thin constructor; the operationally-canonical
    /// builder is [`super::trace_object_forward_utils::init_forward_sink`].
    pub fn new(overflow_callback: ForwardSinkOverflowCallback<TraceObj>) -> Self {
        Self {
            forward_queue: Arc::new(Mutex::new(VecDeque::new())),
            overflow_callback,
        }
    }

    /// Returns the current queue length. Operationally a debug
    /// helper; mirror of upstream's `lengthTBQueue` callsite use.
    pub fn queue_len(&self) -> usize {
        self.forward_queue
            .lock()
            .map(|q| q.len())
            .unwrap_or_default()
    }
}

impl<TraceObj> Clone for ForwardSink<TraceObj> {
    fn clone(&self) -> Self {
        Self {
            forward_queue: Arc::clone(&self.forward_queue),
            overflow_callback: Arc::clone(&self.overflow_callback),
        }
    }
}

impl<TraceObj> std::fmt::Debug for ForwardSink<TraceObj> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardSink")
            .field("queue_len", &self.queue_len())
            .field("overflow_callback", &"<Fn>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_sink_starts_empty() {
        let cb: ForwardSinkOverflowCallback<u32> = Arc::new(|_v: Vec<u32>| {});
        let sink: ForwardSink<u32> = ForwardSink::new(cb);
        assert_eq!(sink.queue_len(), 0);
    }

    #[test]
    fn forward_sink_clone_shares_queue() {
        let cb: ForwardSinkOverflowCallback<u32> = Arc::new(|_v: Vec<u32>| {});
        let sink: ForwardSink<u32> = ForwardSink::new(cb);
        let clone = sink.clone();
        sink.forward_queue.lock().expect("lock").push_back(42);
        assert_eq!(
            clone.queue_len(),
            1,
            "clone should observe push via shared Arc"
        );
    }

    #[test]
    fn forward_sink_overflow_callback_is_invokable() {
        let counter: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
        let counter_clone = Arc::clone(&counter);
        let cb: ForwardSinkOverflowCallback<u32> = Arc::new(move |v: Vec<u32>| {
            *counter_clone.lock().expect("counter") += v.len() as u32;
        });
        let sink: ForwardSink<u32> = ForwardSink::new(cb);
        (sink.overflow_callback)(vec![1, 2, 3]);
        (sink.overflow_callback)(vec![4]);
        assert_eq!(*counter.lock().expect("counter final"), 4);
    }

    #[test]
    fn forward_sink_debug_redacts_callback() {
        let cb: ForwardSinkOverflowCallback<u32> = Arc::new(|_| {});
        let sink: ForwardSink<u32> = ForwardSink::new(cb);
        let s = format!("{sink:?}");
        assert!(s.contains("ForwardSink"));
        assert!(s.contains("queue_len: 0"));
        assert!(s.contains("overflow_callback: \"<Fn>\""));
    }
}
