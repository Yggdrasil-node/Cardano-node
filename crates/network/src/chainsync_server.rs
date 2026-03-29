//! ChainSync mini-protocol server driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the ChainSync state machine invariants. The server responds to header
//! requests and intersection queries from the client.
//!
//! Reference: `Ouroboros.Network.Protocol.ChainSync.Server`.

use crate::connection::timeouts::PROTOCOL_RECV_TIMEOUT;
use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{ChainSyncMessage, ChainSyncState, ChainSyncTransitionError};

// ---------------------------------------------------------------------------
// Server error
// ---------------------------------------------------------------------------

/// Errors from the ChainSync server driver.
#[derive(Debug, thiserror::Error)]
pub enum ChainSyncServerError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] ChainSyncTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Per-state time limit exceeded (upstream `ExceededTimeLimit`).
    #[error("protocol timeout")]
    Timeout,
}

// ---------------------------------------------------------------------------
// Server request
// ---------------------------------------------------------------------------

/// A request received from the ChainSync client.
#[derive(Clone, Debug)]
pub enum ChainSyncServerRequest {
    /// Client requests the next header update.
    RequestNext,
    /// Client requests the best intersection from the given points.
    FindIntersect {
        /// CBOR-encoded candidate points from the client.
        points: Vec<Vec<u8>>,
    },
    /// Client terminates the protocol.
    Done,
}

// ---------------------------------------------------------------------------
// ChainSyncServer
// ---------------------------------------------------------------------------

/// A ChainSync server driver maintaining the protocol state machine.
///
/// The server responds to two kinds of client requests:
///
/// 1. **RequestNext** — the server checks whether there is a new header:
///    - Header available: send `MsgRollForward(header, tip)`.
///    - Chain rolled back: send `MsgRollBackward(point, tip)`.
///    - No update yet: send `MsgAwaitReply`, then later send a
///      roll-forward or roll-backward when data arrives.
///
/// 2. **FindIntersect** — the server finds the best intersection from
///    the client's candidate points:
///    - Found: send `MsgIntersectFound(point, tip)`.
///    - Not found: send `MsgIntersectNotFound(tip)`.
pub struct ChainSyncServer {
    channel: MessageChannel,
    state: ChainSyncState,
}

impl ChainSyncServer {
    /// Create a new server driver from a ChainSync `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — the server waits for the client
    /// to send the first request.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: ChainSyncState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> ChainSyncState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &ChainSyncMessage) -> Result<(), ChainSyncServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(ChainSyncServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<ChainSyncMessage, ChainSyncServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(ChainSyncServerError::ConnectionClosed)?;
        let msg = ChainSyncMessage::from_cbor(&raw)
            .map_err(|e| ChainSyncServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API: receiving requests -----------------------------------

    /// Wait for the next client request in `StIdle`.
    ///
    /// Returns `RequestNext`, `FindIntersect`, or `Done`.  Times out after
    /// [`PROTOCOL_RECV_TIMEOUT`] if the client sends nothing (upstream
    /// `timeLimitsChainSync` `shortWait` for `StIdle`).
    pub async fn recv_request(&mut self) -> Result<ChainSyncServerRequest, ChainSyncServerError> {
        let msg = tokio::time::timeout(PROTOCOL_RECV_TIMEOUT, self.recv_msg())
            .await
            .map_err(|_| ChainSyncServerError::Timeout)??;
        match msg {
            ChainSyncMessage::MsgRequestNext => Ok(ChainSyncServerRequest::RequestNext),
            ChainSyncMessage::MsgFindIntersect { points } => {
                Ok(ChainSyncServerRequest::FindIntersect { points })
            }
            ChainSyncMessage::MsgDone => Ok(ChainSyncServerRequest::Done),
            msg => Err(ChainSyncServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    // -- public API: sending responses ------------------------------------

    /// Send `MsgRollForward` with a header and tip.
    ///
    /// Valid from `StCanAwait` or `StMustReply`. Transitions to `StIdle`.
    pub async fn roll_forward(
        &mut self,
        header: Vec<u8>,
        tip: Vec<u8>,
    ) -> Result<(), ChainSyncServerError> {
        self.send_msg(&ChainSyncMessage::MsgRollForward { header, tip })
            .await
    }

    /// Send `MsgRollBackward` with a point and tip.
    ///
    /// Valid from `StCanAwait` or `StMustReply`. Transitions to `StIdle`.
    pub async fn roll_backward(
        &mut self,
        point: Vec<u8>,
        tip: Vec<u8>,
    ) -> Result<(), ChainSyncServerError> {
        self.send_msg(&ChainSyncMessage::MsgRollBackward { point, tip })
            .await
    }

    /// Send `MsgAwaitReply` to tell the client to wait for new data.
    ///
    /// Valid from `StCanAwait`. Transitions to `StMustReply`.
    pub async fn await_reply(&mut self) -> Result<(), ChainSyncServerError> {
        self.send_msg(&ChainSyncMessage::MsgAwaitReply).await
    }

    /// Send `MsgIntersectFound` with the found point and current tip.
    ///
    /// Valid from `StIntersect`. Transitions to `StIdle`.
    pub async fn intersect_found(
        &mut self,
        point: Vec<u8>,
        tip: Vec<u8>,
    ) -> Result<(), ChainSyncServerError> {
        self.send_msg(&ChainSyncMessage::MsgIntersectFound { point, tip })
            .await
    }

    /// Send `MsgIntersectNotFound` with the current tip.
    ///
    /// Valid from `StIntersect`. Transitions to `StIdle`.
    pub async fn intersect_not_found(
        &mut self,
        tip: Vec<u8>,
    ) -> Result<(), ChainSyncServerError> {
        self.send_msg(&ChainSyncMessage::MsgIntersectNotFound { tip })
            .await
    }
}
