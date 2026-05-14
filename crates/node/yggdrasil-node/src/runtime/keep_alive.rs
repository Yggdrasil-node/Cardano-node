//! Keep-alive heartbeat scheduler for the reconnecting verified-sync loops.
//!
//! Mirrors upstream `Ouroboros.Network.Protocol.KeepAlive.Client` — the
//! sync side of the KeepAlive mini-protocol that prevents idle TCP
//! connections from being torn down by the peer's `keepAliveTimeout`
//! (~97 s upstream default). Yggdrasil's `KeepAliveScheduler` issues
//! a `MsgKeepAlive` every 20 s, well below the timeout.
//!
//! Each `ReconnectingRunState` instance owns one scheduler so the
//! shared `session.keep_alive` mini-protocol client receives periodic
//! pings that match upstream's cadence. Cookies are monotonically
//! wrapping `u16` values.
//!
//! Extracted from `runtime.rs` in R271h.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime adaptor
//! that wraps the protocol-side `KeepAliveClient` driver in a
//! `KeepAliveScheduler` issuing periodic `MsgKeepAlive` at a
//! 20s cadence. Surfaces the runtime callsite for upstream
//! `Ouroboros.Network.Protocol.KeepAlive.Client::keepAliveClient`
//! (which handles the protocol state machine itself); the
//! cadence + per-peer scheduling logic is Yggdrasil-specific
//! and has no upstream parallel — upstream embeds the schedule
//! directly inside `runKeepAliveClient` rather than as a
//! separate type.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use yggdrasil_ledger::Point;
use yggdrasil_network::{KeepAliveClient, KeepAliveClientError};

use crate::sync::{MultiEraSyncProgress, SyncError};
use crate::tracer::NodeTracer;

use super::{
    BatchTraceExtras, ReconnectingRunState, sync_error_trace_fields,
    verified_sync_batch_trace_fields,
};

/// Wall-clock cadence at which the verified-sync reconnect loops emit
/// `MsgKeepAlive` heartbeats to peers.
///
/// Upstream `keepAliveTimeout` defaults to ~97 s; we send well below that
/// to keep the connection live without saturating the channel.  Reference:
/// `Ouroboros.Network.Protocol.KeepAlive.Codec`.
pub(super) const KEEPALIVE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

/// Heartbeat scheduler driving `MsgKeepAlive` traffic alongside the
/// verified-sync request/reply loop.
///
/// Each reconnecting verified-sync inner loop owns one of these so the
/// shared `session.keep_alive` driver receives a periodic ping that
/// matches upstream's `keepAliveClient` cadence.  Cookies are
/// monotonically wrapping `u16` values.
pub(super) struct KeepAliveScheduler {
    last_sent_at: Instant,
    next_cookie: u16,
}

impl KeepAliveScheduler {
    /// Create a fresh scheduler that fires its first heartbeat one
    /// `KEEPALIVE_HEARTBEAT_INTERVAL` from now.
    pub(super) fn new(now: Instant) -> Self {
        Self {
            last_sent_at: now,
            next_cookie: 1,
        }
    }

    /// Send a `MsgKeepAlive` if the heartbeat interval has elapsed.
    ///
    /// Returns `Ok(true)` when a heartbeat was sent and acknowledged,
    /// `Ok(false)` when no heartbeat was due, and propagates the
    /// underlying [`KeepAliveClient`] error otherwise so the caller can
    /// abort the mux and record a reconnect.
    pub(super) async fn tick(
        &mut self,
        client: &mut KeepAliveClient,
    ) -> Result<bool, KeepAliveClientError> {
        if self.last_sent_at.elapsed() < KEEPALIVE_HEARTBEAT_INTERVAL {
            return Ok(false);
        }
        client.keep_alive(self.next_cookie).await?;
        self.next_cookie = self.next_cookie.wrapping_add(1);
        self.last_sent_at = Instant::now();
        Ok(true)
    }
}

pub(super) fn trace_sync_failure(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    error: &SyncError,
    current_point: Point,
) {
    tracer.trace_runtime(
        "Node.Sync",
        "Error",
        "verified sync service failed",
        sync_error_trace_fields(peer_addr, error, current_point),
    );
}

pub(super) fn trace_verified_sync_batch_applied(
    tracer: &NodeTracer,
    peer_addr: SocketAddr,
    current_point: Point,
    progress: &MultiEraSyncProgress,
    run_state: &ReconnectingRunState,
    extras: BatchTraceExtras,
) {
    tracer.trace_runtime(
        "ChainSync.Client",
        "Info",
        "verified sync batch applied",
        verified_sync_batch_trace_fields(peer_addr, current_point, progress, run_state, extras),
    );
}
