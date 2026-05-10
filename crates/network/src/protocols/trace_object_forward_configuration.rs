//! Configuration types for the trace-forwarder TraceObject mini-
//! protocol — caller-supplied parameters that pin the
//! request-batch size, stop-flag, and (optional) tracing channel.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Configuration/TraceObject.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Configuration.TraceObject` module which exposes
//! `AcceptorConfiguration` + `ForwarderConfiguration` records used
//! by `Trace.Forward.Run.TraceObject.{Acceptor, Forwarder}` (R421+).
//!
//! Mapping summary:
//!
//! | Upstream                                                    | Yggdrasil                                |
//! |-------------------------------------------------------------|------------------------------------------|
//! | `data AcceptorConfiguration lo = AcceptorConfiguration { ... }`  | [`AcceptorConfiguration`]              |
//! | `acceptorTracer :: Tracer IO (TraceSendRecv ...)`           | [`AcceptorConfiguration::acceptor_tracer`] (synthesis — see below) |
//! | `whatToRequest :: NumberOfTraceObjects`                     | [`AcceptorConfiguration::what_to_request`] |
//! | `shouldWeStop :: TVar Bool`                                 | [`AcceptorConfiguration::should_we_stop`] |
//! | `data ForwarderConfiguration lo = ForwarderConfiguration { ... }` | [`ForwarderConfiguration`]             |
//! | `forwarderTracer :: Tracer IO (TraceSendRecv ...)`          | [`ForwarderConfiguration::forwarder_tracer`] (synthesis — see below) |
//! | `queueSize :: Word`                                         | [`ForwarderConfiguration::queue_size`]  |
//!
//! Carve-outs (synthesis-mirror, NOT strict-mirror):
//!
//! - **`Tracer IO (TraceSendRecv (TraceObjectForward lo))` debug
//!   channel**: upstream uses `contra-tracer`'s `Tracer` typeclass
//!   to thread a debug-trace sink through the network driver
//!   (`runPeer`'s `acceptorTracer` arg). Yggdrasil collapses this to
//!   an `Option<Arc<dyn Fn(&str) + Send + Sync>>` — the `Tracer`
//!   typeclass has no Rust analog without a workspace-wide trace-
//!   dispatcher port. Operational use cases (logging codec
//!   send/recv events) can be served by a closure.
//! - **`TVar Bool` stop-flag**: replaced with
//!   `Arc<tokio::sync::RwLock<bool>>` mirroring R371's
//!   `ProtocolsBrake` pattern. The atomic-read semantics carry
//!   across cleanly; both forms are read-mostly.
//!
//! Reference: `Trace.Forward.Configuration.TraceObject` from the
//! upstream `trace-forward` package.

use std::sync::Arc;

use tokio::sync::RwLock;

use super::NumberOfTraceObjects;

/// Optional debug-trace channel for codec send/recv events.
///
/// Synthesis carve-out for upstream's
/// `Tracer IO (TraceSendRecv (TraceObjectForward lo))` field —
/// `contra-tracer`'s `Tracer` typeclass has no Rust analog without
/// a workspace-wide trace-dispatcher port. Operators can supply a
/// closure to log or count send/recv events.
pub type TraceForwardTracer = Option<Arc<dyn Fn(&str) + Send + Sync>>;

/// Acceptor-side configuration for the trace-forwarder TraceObject
/// mini-protocol.
///
/// Mirror of upstream's `data AcceptorConfiguration lo` —
/// parameterized over the trace-object payload via the consuming
/// driver's generic, not via the configuration type itself
/// (Yggdrasil's `TraceObjectAcceptor<TraceObj>` already carries
/// the type parameter, so the configuration record can stay
/// payload-agnostic).
#[derive(Clone)]
pub struct AcceptorConfiguration {
    /// Optional tracer for codec send/recv events. `None` (default)
    /// disables tracing. Synthesis carve-out — see [`TraceForwardTracer`]
    /// for the upstream `Tracer IO (TraceSendRecv ...)` field this
    /// stands in for.
    pub acceptor_tracer: TraceForwardTracer,

    /// Number of `TraceObject`s to request per round-trip. Mirror
    /// of upstream's `whatToRequest :: NumberOfTraceObjects`.
    pub what_to_request: NumberOfTraceObjects,

    /// Brake flag. When set to `true` by an external thread, the
    /// acceptor sends `MsgDone` and terminates the session. Mirror
    /// of upstream's `shouldWeStop :: TVar Bool`.
    pub should_we_stop: Arc<RwLock<bool>>,
}

impl std::fmt::Debug for AcceptorConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcceptorConfiguration")
            .field("acceptor_tracer", &self.acceptor_tracer.is_some())
            .field("what_to_request", &self.what_to_request)
            .field("should_we_stop", &"<TVar Bool>")
            .finish()
    }
}

impl AcceptorConfiguration {
    /// Construct a configuration with defaults: no tracer, the
    /// supplied request batch size, and a fresh stop-flag in the
    /// running state. Mirror of the operationally-most-common
    /// upstream construction site.
    pub fn new(what_to_request: NumberOfTraceObjects) -> Self {
        Self {
            acceptor_tracer: None,
            what_to_request,
            should_we_stop: Arc::new(RwLock::new(false)),
        }
    }

    /// Engage the brake flag. After this call the next acceptor
    /// loop iteration will send `MsgDone` and terminate.
    pub async fn request_stop(&self) {
        *self.should_we_stop.write().await = true;
    }

    /// Read the current brake state.
    pub async fn is_stopped(&self) -> bool {
        *self.should_we_stop.read().await
    }
}

/// Forwarder-side configuration for the trace-forwarder TraceObject
/// mini-protocol.
///
/// Mirror of upstream's `data ForwarderConfiguration lo`. Used by
/// `Trace.Forward.Run.TraceObject.Forwarder` (later round; not
/// part of Yggdrasil's R420 cardano-tracer-side scope).
#[derive(Clone)]
pub struct ForwarderConfiguration {
    /// Optional tracer for codec send/recv events. Same synthesis
    /// carve-out as [`AcceptorConfiguration::acceptor_tracer`].
    pub forwarder_tracer: TraceForwardTracer,

    /// Size of the internal queue for tracing items. Mirror of
    /// upstream's `queueSize :: Word`.
    ///
    /// Per upstream's documentation: "Use a size suitable for the
    /// beginning of the session, to avoid queue overflows, because
    /// initially there is no connection with acceptor yet, and the
    /// number of tracing items after the node starts may be very
    /// big. At the same time choose a number that reduces memory
    /// usage in the node."
    pub queue_size: u32,
}

impl std::fmt::Debug for ForwarderConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwarderConfiguration")
            .field("forwarder_tracer", &self.forwarder_tracer.is_some())
            .field("queue_size", &self.queue_size)
            .finish()
    }
}

impl ForwarderConfiguration {
    /// Construct a configuration with defaults: no tracer + the
    /// supplied queue size.
    pub fn new(queue_size: u32) -> Self {
        Self {
            forwarder_tracer: None,
            queue_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acceptor_configuration_default_state() {
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(10));
        assert_eq!(config.what_to_request, NumberOfTraceObjects(10));
        assert!(!config.is_stopped().await, "default stop flag = false");
        assert!(config.acceptor_tracer.is_none(), "default tracer = None");
    }

    #[tokio::test]
    async fn acceptor_configuration_request_stop_engages_brake() {
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(1));
        assert!(!config.is_stopped().await);
        config.request_stop().await;
        assert!(config.is_stopped().await, "after request_stop, flag set");
    }

    #[tokio::test]
    async fn acceptor_configuration_clone_shares_brake() {
        // Arc<RwLock<bool>> shares state across clones — engaging
        // the brake on one clone propagates to all others.
        let a = AcceptorConfiguration::new(NumberOfTraceObjects(5));
        let b = a.clone();
        a.request_stop().await;
        assert!(b.is_stopped().await, "clone observes brake engagement");
    }

    #[test]
    fn acceptor_configuration_debug_redacts_brake_value() {
        // The Debug impl prints the brake as a placeholder rather
        // than the actual bool — the lock would need to be acquired
        // synchronously, which Debug can't do. Operators inspecting
        // logs see the type, not the runtime state.
        let config = AcceptorConfiguration::new(NumberOfTraceObjects(7));
        let s = format!("{config:?}");
        assert!(s.contains("AcceptorConfiguration"));
        assert!(s.contains("what_to_request"));
        assert!(s.contains("<TVar Bool>"));
    }

    #[tokio::test]
    async fn acceptor_configuration_with_tracer_set() {
        let mut config = AcceptorConfiguration::new(NumberOfTraceObjects(3));
        let tracer: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|_msg: &str| {});
        config.acceptor_tracer = Some(tracer);
        let s = format!("{config:?}");
        assert!(s.contains("acceptor_tracer: true"));
    }

    #[test]
    fn forwarder_configuration_default_state() {
        let config = ForwarderConfiguration::new(1024);
        assert_eq!(config.queue_size, 1024);
        assert!(config.forwarder_tracer.is_none());
    }

    #[test]
    fn forwarder_configuration_debug_includes_queue_size() {
        let config = ForwarderConfiguration::new(2048);
        let s = format!("{config:?}");
        assert!(s.contains("ForwarderConfiguration"));
        assert!(s.contains("queue_size: 2048"));
        assert!(s.contains("forwarder_tracer: false"));
    }
}
