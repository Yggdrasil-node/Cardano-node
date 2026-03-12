//! ChainSync mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the ChainSync state machine invariants.  The driver operates entirely
//! at the client-agency level: it sends `MsgRequestNext`, `MsgFindIntersect`,
//! and `MsgDone`, and awaits the corresponding server responses.
//!
//! Reference: `Ouroboros.Network.Protocol.ChainSync.Client`.

use crate::mux::{MuxError, ProtocolHandle};
use crate::protocols::{ChainSyncMessage, ChainSyncState, ChainSyncTransitionError};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// The server's response to a `MsgRequestNext`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NextResponse {
    /// A new header was rolled forward.
    RollForward {
        /// Serialised block header.
        header: Vec<u8>,
        /// Serialised tip.
        tip: Vec<u8>,
    },
    /// The chain rolled backward to a prior point.
    RollBackward {
        /// Serialised point to roll back to.
        point: Vec<u8>,
        /// Serialised tip.
        tip: Vec<u8>,
    },
    /// The server asked us to wait and then later delivered a roll-forward.
    AwaitRollForward {
        header: Vec<u8>,
        tip: Vec<u8>,
    },
    /// The server asked us to wait and then later delivered a rolled-backward.
    AwaitRollBackward {
        point: Vec<u8>,
        tip: Vec<u8>,
    },
}

/// The server's response to a `MsgFindIntersect`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntersectResponse {
    /// An intersection was found.
    Found {
        /// The intersection point.
        point: Vec<u8>,
        /// Current tip.
        tip: Vec<u8>,
    },
    /// No intersection was found.
    NotFound {
        /// Current tip.
        tip: Vec<u8>,
    },
}

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the ChainSync client driver.
#[derive(Debug, thiserror::Error)]
pub enum ChainSyncClientError {
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

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// ChainSyncClient
// ---------------------------------------------------------------------------

/// A ChainSync client driver maintaining the protocol state machine.
///
/// All public methods advance the state machine and return strongly-typed
/// responses.  The driver is cancel-safe: dropping it in any state is
/// allowed (the muxer will clean up the channel).
pub struct ChainSyncClient {
    handle: ProtocolHandle,
    state: ChainSyncState,
}

impl ChainSyncClient {
    /// Create a new client driver from a ChainSync `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            handle,
            state: ChainSyncState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> ChainSyncState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &ChainSyncMessage) -> Result<(), ChainSyncClientError> {
        self.state = self.state.transition(msg)?;
        self.handle
            .send(msg.to_cbor())
            .await
            .map_err(ChainSyncClientError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<ChainSyncMessage, ChainSyncClientError> {
        let raw = self
            .handle
            .recv()
            .await
            .ok_or(ChainSyncClientError::ConnectionClosed)?;
        let msg = ChainSyncMessage::from_cbor(&raw)
            .map_err(|e| ChainSyncClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgFindIntersect` with the given candidate points and wait
    /// for the server's `MsgIntersectFound` or `MsgIntersectNotFound`.
    ///
    /// The client must be in `StIdle`.
    pub async fn find_intersect(
        &mut self,
        points: Vec<Vec<u8>>,
    ) -> Result<IntersectResponse, ChainSyncClientError> {
        self.send_msg(&ChainSyncMessage::MsgFindIntersect { points })
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            ChainSyncMessage::MsgIntersectFound { point, tip } => {
                Ok(IntersectResponse::Found { point, tip })
            }
            ChainSyncMessage::MsgIntersectNotFound { tip } => {
                Ok(IntersectResponse::NotFound { tip })
            }
            other => Err(ChainSyncClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Send `MsgRequestNext` and wait for the server's roll-forward,
    /// roll-backward, or await-then-reply sequence.
    ///
    /// The client must be in `StIdle`.
    pub async fn request_next(&mut self) -> Result<NextResponse, ChainSyncClientError> {
        self.send_msg(&ChainSyncMessage::MsgRequestNext).await?;
        let msg = self.recv_msg().await?;
        match msg {
            ChainSyncMessage::MsgRollForward { header, tip } => {
                Ok(NextResponse::RollForward { header, tip })
            }
            ChainSyncMessage::MsgRollBackward { point, tip } => {
                Ok(NextResponse::RollBackward { point, tip })
            }
            ChainSyncMessage::MsgAwaitReply => {
                // Wait for the real response after AwaitReply.
                let follow_up = self.recv_msg().await?;
                match follow_up {
                    ChainSyncMessage::MsgRollForward { header, tip } => {
                        Ok(NextResponse::AwaitRollForward { header, tip })
                    }
                    ChainSyncMessage::MsgRollBackward { point, tip } => {
                        Ok(NextResponse::AwaitRollBackward { point, tip })
                    }
                    other => Err(ChainSyncClientError::UnexpectedMessage(
                        other.tag_name().to_string(),
                    )),
                }
            }
            other => Err(ChainSyncClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Send `MsgDone` to terminate the protocol cleanly.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are possible.
    pub async fn done(mut self) -> Result<(), ChainSyncClientError> {
        self.send_msg(&ChainSyncMessage::MsgDone).await
    }
}
