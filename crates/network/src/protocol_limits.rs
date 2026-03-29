//! Per-protocol per-state time limits matching upstream `ProtocolTimeLimits`.
//!
//! Each mini-protocol defines how long the receiving party may wait for the
//! next message in a given state before considering the peer stalled.
//! `None` means *wait forever* (no timeout).
//!
//! Reference: `Ouroboros.Network.Protocol.Limits` (`shortWait`, `longWait`,
//! `waitForever`) and per-protocol codec modules in
//! `ouroboros-network-protocols`.

use std::time::Duration;

/// Short wait — 10 seconds.
///
/// Upstream: `shortWait = Just 10`.
pub const SHORT_WAIT: Option<Duration> = Some(Duration::from_secs(10));

/// Long wait — 60 seconds.
///
/// Upstream: `longWait = Just 60`.
pub const LONG_WAIT: Option<Duration> = Some(Duration::from_secs(60));

/// Wait forever — no timeout.
///
/// Upstream: `waitForever = Nothing`.
pub const WAIT_FOREVER: Option<Duration> = None;

// ---------------------------------------------------------------------------
// KeepAlive time limits
// ---------------------------------------------------------------------------

/// KeepAlive per-state time limits.
///
/// Reference: `Ouroboros.Network.Protocol.KeepAlive.Codec.timeLimitsKeepAlive`.
pub mod keepalive {
    use super::*;

    /// Server waits for `MsgKeepAlive` from the client.
    ///
    /// Upstream `SingServer → Just 60`.
    pub const SERVER: Option<Duration> = LONG_WAIT;

    /// Client waits for `MsgKeepAliveResponse` from the server.
    ///
    /// Upstream `SingClient → Just 97`.
    pub const CLIENT: Option<Duration> = Some(Duration::from_secs(97));
}

// ---------------------------------------------------------------------------
// BlockFetch time limits
// ---------------------------------------------------------------------------

/// BlockFetch per-state time limits.
///
/// Reference: `Ouroboros.Network.Protocol.BlockFetch.Codec.timeLimitsBlockFetch`.
pub mod blockfetch {
    use super::*;

    /// Server idle: waiting for client's `MsgRequestRange` or `MsgClientDone`.
    ///
    /// Upstream `SingBFIdle → waitForever`.
    pub const BF_IDLE: Option<Duration> = WAIT_FOREVER;

    /// Client waiting for server's `MsgStartBatch` or `MsgNoBlocks` after
    /// sending a range request.
    ///
    /// Upstream `SingBFBusy → longWait (60s)`.
    pub const BF_BUSY: Option<Duration> = LONG_WAIT;

    /// Client waiting for next `MsgBlock` or `MsgBatchDone` during streaming.
    ///
    /// Upstream `SingBFStreaming → longWait (60s)`.
    pub const BF_STREAMING: Option<Duration> = LONG_WAIT;
}

// ---------------------------------------------------------------------------
// ChainSync time limits
// ---------------------------------------------------------------------------

/// ChainSync per-state time limits.
///
/// Reference: `Ouroboros.Network.Protocol.ChainSync.Codec.timeLimitsChainSync`.
pub mod chainsync {
    use super::*;

    /// Server idle: waiting for client's `MsgRequestNext` / `MsgFindIntersect`
    /// / `MsgDone`.
    ///
    /// Upstream `SingStIdle → Nothing` (configurable via `ChainSyncIdleTimeout`
    /// for non-trustable peers).
    pub const ST_IDLE: Option<Duration> = WAIT_FOREVER;

    /// Client waiting for server's `IntersectFound` / `IntersectNotFound`.
    ///
    /// Upstream `SingStIntersect → shortWait (10s)`.
    pub const ST_INTERSECT: Option<Duration> = SHORT_WAIT;

    /// Client waiting for server's `RollForward` / `RollBackward` when the
    /// server said it *can* await (tip reached).
    ///
    /// Upstream `SingStNext SingCanAwait → shortWait (10s)`.
    pub const ST_NEXT_CAN_AWAIT: Option<Duration> = SHORT_WAIT;

    /// Client waiting for server's `RollForward` / `RollBackward` when the
    /// server said the client *must* reply (next-block is available).
    ///
    /// For trustable peers: `waitForever`.
    /// For non-trustable peers: randomized 135–269 s (upstream uses a VRF-
    /// derived formula; this constant uses the lower bound).
    ///
    /// Upstream: `SingStNext SingMustReply → computeTimeLimits`.
    pub const ST_NEXT_MUST_REPLY_TRUSTABLE: Option<Duration> = WAIT_FOREVER;

    /// Minimum ChainSync timeout for non-trustable peers (seconds).
    ///
    /// Upstream: `minChainSyncTimeout = 601` (but the effective per-wait
    /// minimum is ~135 s after the VRF formula).
    pub const MUST_REPLY_MIN_SECS: u64 = 135;

    /// Maximum ChainSync timeout for non-trustable peers (seconds).
    ///
    /// Upstream: `maxChainSyncTimeout = 911` (effective per-wait max ~269 s).
    pub const MUST_REPLY_MAX_SECS: u64 = 269;
}

// ---------------------------------------------------------------------------
// TxSubmission2 time limits
// ---------------------------------------------------------------------------

/// TxSubmission2 per-state time limits.
///
/// In TxSubmission2 the *server* drives the conversation (sends requests,
/// client replies).  The server is the party waiting for client replies, so
/// server-side timeouts apply to `StTxIds NonBlocking` and `StTxs`.
///
/// Reference: `Ouroboros.Network.Protocol.TxSubmission2.Codec.timeLimitsTxSubmission2`.
pub mod txsubmission {
    use super::*;

    /// Server waiting for client's initial `MsgInit` reply.
    ///
    /// Upstream `SingStInit → waitForever`.
    pub const ST_INIT: Option<Duration> = WAIT_FOREVER;

    /// Server idle: before sending the next request.
    ///
    /// Upstream `SingStIdle → waitForever`.
    pub const ST_IDLE: Option<Duration> = WAIT_FOREVER;

    /// Server waiting for client's `MsgReplyTxIds` on a *blocking* request.
    ///
    /// Upstream `SingStTxIds SingBlocking → waitForever`.
    pub const ST_TX_IDS_BLOCKING: Option<Duration> = WAIT_FOREVER;

    /// Server waiting for client's `MsgReplyTxIds` on a *non-blocking* request.
    ///
    /// Upstream `SingStTxIds SingNonBlocking → shortWait (10s)`.
    pub const ST_TX_IDS_NON_BLOCKING: Option<Duration> = SHORT_WAIT;

    /// Server waiting for client's `MsgReplyTxs` after requesting bodies.
    ///
    /// Upstream `SingStTxs → shortWait (10s)`.
    pub const ST_TXS: Option<Duration> = SHORT_WAIT;
}

// ---------------------------------------------------------------------------
// PeerSharing time limits
// ---------------------------------------------------------------------------

/// PeerSharing per-state time limits.
///
/// Reference: `Ouroboros.Network.Protocol.PeerSharing.Codec.timeLimitsPeerSharing`.
pub mod peersharing {
    use super::*;

    /// Server idle: waiting for client's `MsgShareRequest` / `MsgDone`.
    ///
    /// Upstream `SingStIdle → waitForever`.
    pub const ST_IDLE: Option<Duration> = WAIT_FOREVER;

    /// Client waiting for server's `MsgSharePeers`.
    ///
    /// Upstream `SingStBusy → longWait (60s)`.
    pub const ST_BUSY: Option<Duration> = LONG_WAIT;
}

// ---------------------------------------------------------------------------
// Handshake time limits
// ---------------------------------------------------------------------------

/// Handshake per-state time limits.
///
/// Reference: `Ouroboros.Network.Protocol.Handshake.Codec.timeLimitsHandshake`.
pub mod handshake {
    use super::*;

    /// Waiting for `MsgProposeVersions`.
    ///
    /// Upstream `SingStPropose → shortWait (10s)`.
    pub const ST_PROPOSE: Option<Duration> = SHORT_WAIT;

    /// Waiting for `MsgAcceptVersion` / `MsgRefuse` / `MsgQueryReply`.
    ///
    /// Upstream `SingStConfirm → shortWait (10s)`.
    pub const ST_CONFIRM: Option<Duration> = SHORT_WAIT;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_wait_is_10s() {
        assert_eq!(SHORT_WAIT, Some(Duration::from_secs(10)));
    }

    #[test]
    fn long_wait_is_60s() {
        assert_eq!(LONG_WAIT, Some(Duration::from_secs(60)));
    }

    #[test]
    fn wait_forever_is_none() {
        assert_eq!(WAIT_FOREVER, None);
    }

    #[test]
    fn keepalive_server_is_60s() {
        assert_eq!(keepalive::SERVER, Some(Duration::from_secs(60)));
    }

    #[test]
    fn keepalive_client_is_97s() {
        assert_eq!(keepalive::CLIENT, Some(Duration::from_secs(97)));
    }

    #[test]
    fn blockfetch_idle_is_none() {
        assert!(blockfetch::BF_IDLE.is_none());
    }

    #[test]
    fn blockfetch_busy_is_60s() {
        assert_eq!(blockfetch::BF_BUSY, Some(Duration::from_secs(60)));
    }

    #[test]
    fn txsubmission_txs_is_10s() {
        assert_eq!(txsubmission::ST_TXS, Some(Duration::from_secs(10)));
    }

    #[test]
    fn txsubmission_non_blocking_is_10s() {
        assert_eq!(txsubmission::ST_TX_IDS_NON_BLOCKING, Some(Duration::from_secs(10)));
    }

    #[test]
    fn chainsync_intersect_is_10s() {
        assert_eq!(chainsync::ST_INTERSECT, Some(Duration::from_secs(10)));
    }

    #[test]
    fn peersharing_busy_is_60s() {
        assert_eq!(peersharing::ST_BUSY, Some(Duration::from_secs(60)));
    }

    #[test]
    fn handshake_propose_is_10s() {
        assert_eq!(handshake::ST_PROPOSE, Some(Duration::from_secs(10)));
    }
}
