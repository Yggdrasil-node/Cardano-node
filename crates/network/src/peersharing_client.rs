//! PeerSharing mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the PeerSharing state machine invariants.  The client requests batches
//! of peer addresses from the server for peer governor discovery.
//!
//! Per-state time limits from `protocol_limits::peersharing` are enforced on
//! the server's response.  Upstream reference:
//! `Ouroboros.Network.Protocol.PeerSharing.Codec.timeLimitsPeerSharing`.
//!
//! Reference: `Ouroboros.Network.Protocol.PeerSharing.Client`.

use std::time::Duration;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocol_limits::peersharing as ps_limits;
use crate::protocols::{
    PeerSharingMessage, PeerSharingState, PeerSharingTransitionError, SharedPeerAddress,
};

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the PeerSharing client driver.
#[derive(Debug, thiserror::Error)]
pub enum PeerSharingClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// The server did not respond within the per-state time limit.
    ///
    /// Upstream: `ExceededTimeLimit` from `ProtocolTimeLimits`.
    #[error("protocol timeout ({0:?})")]
    Timeout(Duration),

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] PeerSharingTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// PeerSharingClient
// ---------------------------------------------------------------------------

/// A PeerSharing client driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`Self::share_request`] to ask the server for peer addresses.
/// 2. Repeat step 1 as needed (each call is a full request/reply cycle).
/// 3. Call [`Self::done`] to terminate the protocol cleanly.
pub struct PeerSharingClient {
    channel: MessageChannel,
    state: PeerSharingState,
}

impl PeerSharingClient {
    /// Create a new client driver from a PeerSharing `ProtocolHandle`.
    ///
    /// The protocol starts in `StClient` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: PeerSharingState::StClient,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> PeerSharingState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &PeerSharingMessage) -> Result<(), PeerSharingClientError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(PeerSharingClientError::Mux)
    }

    /// Receive with an optional per-state time limit.
    async fn recv_msg_timeout(
        &mut self,
        limit: Option<Duration>,
    ) -> Result<PeerSharingMessage, PeerSharingClientError> {
        let raw = match limit {
            Some(d) => tokio::time::timeout(d, self.channel.recv())
                .await
                .map_err(|_| PeerSharingClientError::Timeout(d))?
                .ok_or(PeerSharingClientError::ConnectionClosed)?,
            None => self
                .channel
                .recv()
                .await
                .ok_or(PeerSharingClientError::ConnectionClosed)?,
        };
        let msg = PeerSharingMessage::from_cbor(&raw)
            .map_err(|e| PeerSharingClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgShareRequest` requesting up to `amount` peers and wait for
    /// `MsgSharePeers`.  Returns the list of shared peer addresses.
    ///
    /// Enforces `peersharing::ST_BUSY` time limit (60 s) on the server's
    /// response.
    ///
    /// The client must be in `StClient`.
    pub async fn share_request(
        &mut self,
        amount: u16,
    ) -> Result<Vec<SharedPeerAddress>, PeerSharingClientError> {
        self.send_msg(&PeerSharingMessage::MsgShareRequest { amount })
            .await?;
        let msg = self.recv_msg_timeout(ps_limits::ST_BUSY).await?;
        match msg {
            PeerSharingMessage::MsgSharePeers { peers } => Ok(peers),
            _ => Err(PeerSharingClientError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Send `MsgDone` to terminate the protocol cleanly.
    ///
    /// The client must be in `StClient`.  After this call the driver is in
    /// `StDone` and no further messages can be sent.
    pub async fn done(mut self) -> Result<(), PeerSharingClientError> {
        self.send_msg(&PeerSharingMessage::MsgDone).await
    }
}
