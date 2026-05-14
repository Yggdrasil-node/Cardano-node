//! `tracing-subscriber` Layer<S> adapter that forwards every
//! `tracing::Event` to a cardano-tracer-compatible Unix socket.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side adapter that bridges the
//! `tracing` crate's `Layer<S>` API (a sync callback that fires on
//! every `info!` / `warn!` / `error!` / etc.) to the async
//! [`super::forwarding_task::run`] pipeline. Upstream `cardano-node`
//! emits events through the `iohk-monitoring-framework` /
//! `contra-tracer` stack which is conceptually equivalent; the
//! Rust-side Layer<S> closes the API mismatch between the two.
//!
//! ## How a binary wires it
//!
//! ```ignore
//! use tokio::sync::mpsc;
//! use tokio::net::UnixStream;
//!
//! let (tx, rx) = mpsc::unbounded_channel();
//! let socket = UnixStream::connect("/run/cardano-tracer.sock").await?;
//! let bearer = Bearer::new(socket);
//!
//! tokio::spawn(forwarding_task::run(
//!     rx,
//!     bearer,
//!     ForwardingTaskConfig::default(),
//! ));
//!
//! let forwarder_layer = TraceForwardingLayer::new(tx, "yggdrasil-node-01".to_string());
//!
//! tracing_subscriber::registry()
//!     .with(haskell_json_fmt_layer)
//!     .with(forwarder_layer)
//!     .init();
//! ```
//!
//! Behaviour: each `tracing::Event` is converted to a
//! [`super::TraceObject`] via [`super::event_builder::build_trace_object_from_event`]
//! and pushed through the channel. The forwarding task drains the
//! channel and batches replies onto the bearer. Channel-send is
//! non-blocking — under sustained back-pressure (peer is slow or
//! gone) the channel grows unbounded and the producer is decoupled
//! from the consumer. The drop policy is "never drop, always
//! buffer"; operators concerned about memory growth should run
//! `forwarding_task::run` against a working cardano-tracer (the
//! standard case) or omit this layer entirely.

use tokio::sync::mpsc::UnboundedSender;

use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use super::TraceObject;
use super::event_builder::build_trace_object_from_event;

/// `tracing-subscriber` Layer<S> that forwards every event to a
/// cardano-tracer Unix socket.
///
/// Wrap with `.with(forwarder_layer)` on a `tracing_subscriber::registry()`
/// chain. Pair with a separate `tokio::spawn(forwarding_task::run(rx,
/// bearer, …))` to actually drain the channel onto the wire.
pub struct TraceForwardingLayer {
    tx: UnboundedSender<TraceObject>,
    hostname: String,
}

impl TraceForwardingLayer {
    /// Construct a forwarder over an mpsc Sender already wired to a
    /// running `forwarding_task::run`. `hostname` lands in every
    /// emitted TraceObject's `to_hostname` field and should be the
    /// operator-visible node identifier (typically the binary name +
    /// a per-instance suffix).
    pub fn new(tx: UnboundedSender<TraceObject>, hostname: String) -> Self {
        Self { tx, hostname }
    }
}

impl<S> Layer<S> for TraceForwardingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let trace_object = build_trace_object_from_event::<S>(event, &self.hostname);
        // try_send is correct here: the receiver is unbounded, so
        // send only fails if every receiver has been dropped. That
        // case is the operator removing the cardano-tracer
        // integration without uninstalling the layer; silently
        // dropping the event is the right call (the local stdout
        // formatter still emitted it).
        let _ = self.tx.send(trace_object);
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;
    use crate::trace_forwarder::{TraceDetail, TraceSeverity};
    use tokio::sync::mpsc;
    use tracing::Level;
    use tracing_subscriber::Registry;
    use tracing_subscriber::prelude::*;

    /// End-to-end Layer test: install the forwarder layer, emit
    /// `tracing::info!` / `warn!` / `error!` events, drain the
    /// channel, assert each event arrives as a TraceObject with the
    /// expected severity + hostname.
    #[test]
    fn layer_forwards_events_to_channel() {
        let (tx, mut rx) = mpsc::unbounded_channel::<TraceObject>();
        let layer = TraceForwardingLayer::new(tx, "test-host".to_string());

        let subscriber = Registry::default().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(field = "hello", "info-event");
            tracing::warn!("warn-event");
            tracing::error!(code = 42_u64, "error-event");
        });

        // Three events should be queued.
        let mut received = Vec::new();
        while let Ok(to) = rx.try_recv() {
            received.push(to);
        }
        assert_eq!(received.len(), 3, "expected 3 forwarded events");

        // Severities map 1:1 from Level → SeverityS.
        assert_eq!(received[0].to_severity, TraceSeverity::Info);
        assert_eq!(received[1].to_severity, TraceSeverity::Warning);
        assert_eq!(received[2].to_severity, TraceSeverity::Error);

        // Hostname plumbed through.
        for to in &received {
            assert_eq!(to.to_hostname, "test-host");
            assert_eq!(to.to_details, TraceDetail::DNormal);
            assert!(to.to_human.is_none());
        }

        // Field map encoded into to_machine.
        assert!(
            received[0].to_machine.contains("\"field\":\"hello\""),
            "info event's field should be in to_machine: got {}",
            received[0].to_machine
        );
        assert!(
            received[2].to_machine.contains("\"code\":42"),
            "error event's u64 code should be in to_machine: got {}",
            received[2].to_machine
        );
    }

    /// When every Sender is dropped (i.e., the forwarding task
    /// exited and its rx half is gone), Layer::on_event silently
    /// swallows the send-error rather than panicking.
    #[test]
    fn layer_send_to_closed_channel_is_silent() {
        let (tx, rx) = mpsc::unbounded_channel::<TraceObject>();
        drop(rx); // No receiver — sender is "dead" for any further send.
        let layer = TraceForwardingLayer::new(tx, "test-host".to_string());

        let subscriber = Registry::default().with(layer);
        // Must not panic.
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("event-into-the-void");
        });
    }

    /// Severity floor: TRACE collapses to Debug per the upstream
    /// mapping (cardano-tracer SeverityS has no TRACE level).
    #[test]
    fn layer_trace_level_maps_to_debug_severity() {
        let (tx, mut rx) = mpsc::unbounded_channel::<TraceObject>();
        let layer = TraceForwardingLayer::new(tx, "test-host".to_string());

        // The Registry subscriber by default doesn't filter by level
        // (LookupSpan tree only); the layer's on_event fires for
        // every level. We assert TRACE → Debug.
        let subscriber = Registry::default().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::event!(Level::TRACE, "trace-event");
        });

        let to = rx.try_recv().expect("trace event forwarded");
        assert_eq!(
            to.to_severity,
            TraceSeverity::Debug,
            "TRACE must collapse to Debug per upstream SeverityS"
        );
    }
}
