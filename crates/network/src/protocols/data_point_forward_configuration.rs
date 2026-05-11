//! Configuration types for the trace-forwarder DataPoint mini-
//! protocol — caller-supplied parameters that pin the stop-flag and
//! (optional) tracing channel.
//!
//! ## Naming parity
//!
//! **Strict mirror:** trace-forward/src/Trace/Forward/Configuration/DataPoint.hs.
//!
//! Filename flattens the upstream directory; mirror of upstream's
//! `Trace.Forward.Configuration.DataPoint` module which exposes
//! `AcceptorConfiguration` + `ForwarderConfiguration` records used
//! by `Trace.Forward.Run.DataPoint.{Acceptor, Forwarder}` (R457+).
//!
//! Sister to [`super::trace_object_forward_configuration`] (R420)
//! with two differences:
//! - **No `whatToRequest` field** — DataPoint requests are
//!   external-context-driven (the consumer of node-info supplies
//!   the name list to request), unlike TraceObject which has a
//!   fixed per-round batch size.
//! - **`ForwarderConfiguration` is a single-field newtype** —
//!   upstream has no `queueSize` field for DataPoint, because the
//!   forwarder produces values on demand from a map rather than
//!   from a queue.
//!
//! Mapping summary:
//!
//! | Upstream                                                    | Yggdrasil                                |
//! |-------------------------------------------------------------|------------------------------------------|
//! | `data AcceptorConfiguration = AcceptorConfiguration { ... }`     | [`DataPointAcceptorConfiguration`]      |
//! | `acceptorTracer :: Tracer IO (TraceSendRecv DataPointForward)` | [`DataPointAcceptorConfiguration::acceptor_tracer`] (synthesis — see below) |
//! | `shouldWeStop :: TVar Bool`                                 | [`DataPointAcceptorConfiguration::should_we_stop`] |
//! | `newtype ForwarderConfiguration = ForwarderConfiguration { ... }` | [`DataPointForwarderConfiguration`]    |
//! | `forwarderTracer :: Tracer IO (TraceSendRecv DataPointForward)` | [`DataPointForwarderConfiguration::forwarder_tracer`] (synthesis — see below) |
//!
//! Carve-outs (synthesis-mirror, NOT strict-mirror):
//!
//! - **`Tracer IO (TraceSendRecv DataPointForward)` debug channel**:
//!   upstream uses `contra-tracer`'s `Tracer` typeclass to thread a
//!   debug-trace sink through the network driver. Yggdrasil reuses
//!   the same [`super::TraceForwardTracer`] alias (introduced by
//!   R420) — both DataPoint and TraceObject sub-protocols share the
//!   single `Option<Arc<dyn Fn(&str) + Send + Sync>>` form.
//! - **`TVar Bool` stop-flag**: replaced with
//!   `Arc<tokio::sync::RwLock<bool>>` mirroring R420's pattern and
//!   the wider `ProtocolsBrake` precedent.
//!
//! Reference: `Trace.Forward.Configuration.DataPoint` from the
//! upstream `trace-forward` package.

use std::sync::Arc;

use tokio::sync::RwLock;

use super::TraceForwardTracer;

/// Acceptor-side configuration for the trace-forwarder DataPoint
/// mini-protocol.
///
/// Mirror of upstream's `data AcceptorConfiguration =
/// AcceptorConfiguration { acceptorTracer, shouldWeStop }`. There is
/// no `whatToRequest` field — DataPoint name lists are supplied
/// per-request by the external consumer of node-info data-points.
#[derive(Clone)]
pub struct DataPointAcceptorConfiguration {
    /// Optional tracer for codec send/recv events. `None` (default)
    /// disables tracing. Synthesis carve-out — see
    /// [`super::TraceForwardTracer`] for the upstream
    /// `Tracer IO (TraceSendRecv DataPointForward)` field this
    /// stands in for.
    pub acceptor_tracer: TraceForwardTracer,

    /// Brake flag. When set to `true` by an external thread, the
    /// acceptor sends `MsgDone` and terminates the session. Mirror
    /// of upstream's `shouldWeStop :: TVar Bool`.
    pub should_we_stop: Arc<RwLock<bool>>,
}

impl std::fmt::Debug for DataPointAcceptorConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataPointAcceptorConfiguration")
            .field("acceptor_tracer", &self.acceptor_tracer.is_some())
            .field("should_we_stop", &"<TVar Bool>")
            .finish()
    }
}

impl Default for DataPointAcceptorConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

impl DataPointAcceptorConfiguration {
    /// Construct a configuration with defaults: no tracer and a
    /// fresh stop-flag in the running state. Mirror of the
    /// operationally-most-common upstream construction site.
    pub fn new() -> Self {
        Self {
            acceptor_tracer: None,
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

/// Forwarder-side configuration for the trace-forwarder DataPoint
/// mini-protocol.
///
/// Mirror of upstream's `newtype ForwarderConfiguration =
/// ForwarderConfiguration { forwarderTracer }`. There is no
/// `queueSize` field — the forwarder produces values on demand from
/// a map (`DataPointStore`) rather than from a queue, unlike
/// TraceObject's bounded-buffer model.
///
/// Used by `Trace.Forward.Run.DataPoint.Forwarder` (later round; not
/// part of Yggdrasil's R455 cardano-tracer-side scope — the
/// cardano-tracer side only needs the acceptor configuration).
#[derive(Clone)]
pub struct DataPointForwarderConfiguration {
    /// Optional tracer for codec send/recv events. Same synthesis
    /// carve-out as
    /// [`DataPointAcceptorConfiguration::acceptor_tracer`].
    pub forwarder_tracer: TraceForwardTracer,
}

impl std::fmt::Debug for DataPointForwarderConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataPointForwarderConfiguration")
            .field("forwarder_tracer", &self.forwarder_tracer.is_some())
            .finish()
    }
}

impl Default for DataPointForwarderConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

impl DataPointForwarderConfiguration {
    /// Construct a configuration with defaults: no tracer.
    pub fn new() -> Self {
        Self {
            forwarder_tracer: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acceptor_configuration_default_state() {
        let config = DataPointAcceptorConfiguration::new();
        assert!(!config.is_stopped().await, "default stop flag = false");
        assert!(config.acceptor_tracer.is_none(), "default tracer = None");
    }

    #[tokio::test]
    async fn acceptor_configuration_request_stop_engages_brake() {
        let config = DataPointAcceptorConfiguration::new();
        assert!(!config.is_stopped().await);
        config.request_stop().await;
        assert!(config.is_stopped().await, "after request_stop, flag set");
    }

    #[tokio::test]
    async fn acceptor_configuration_clone_shares_brake() {
        // Arc<RwLock<bool>> shares state across clones — engaging
        // the brake on one clone propagates to all others.
        let a = DataPointAcceptorConfiguration::new();
        let b = a.clone();
        a.request_stop().await;
        assert!(b.is_stopped().await, "clone observes brake engagement");
    }

    #[test]
    fn acceptor_configuration_debug_redacts_brake_value() {
        // The Debug impl prints the brake as a placeholder rather
        // than the actual bool — the lock would need to be acquired
        // synchronously, which Debug can't do.
        let config = DataPointAcceptorConfiguration::new();
        let s = format!("{config:?}");
        assert!(s.contains("DataPointAcceptorConfiguration"));
        assert!(s.contains("<TVar Bool>"));
    }

    #[tokio::test]
    async fn acceptor_configuration_with_tracer_set() {
        let mut config = DataPointAcceptorConfiguration::new();
        let tracer: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|_msg: &str| {});
        config.acceptor_tracer = Some(tracer);
        let s = format!("{config:?}");
        assert!(s.contains("acceptor_tracer: true"));
    }

    #[test]
    fn forwarder_configuration_default_state() {
        let config = DataPointForwarderConfiguration::new();
        assert!(config.forwarder_tracer.is_none());
    }

    #[test]
    fn forwarder_configuration_debug_includes_tracer_flag() {
        let config = DataPointForwarderConfiguration::new();
        let s = format!("{config:?}");
        assert!(s.contains("DataPointForwarderConfiguration"));
        assert!(s.contains("forwarder_tracer: false"));
    }
}
