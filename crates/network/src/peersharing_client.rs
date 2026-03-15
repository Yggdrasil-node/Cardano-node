//! PeerSharing mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the PeerSharing state machine invariants.  The client requests batches
//! of peer addresses from the server for peer governor discovery.
//!
//! Reference: `Ouroboros.Network.Protocol.PeerSharing.Client`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
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
/// 1. Call [`share_request`] to ask the server for peer addresses.
/// 2. Repeat step 1 as needed (each call is a full request/reply cycle).
/// 3. Call [`done`] to terminate the protocol cleanly.
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

    async fn recv_msg(&mut self) -> Result<PeerSharingMessage, PeerSharingClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(PeerSharingClientError::ConnectionClosed)?;
        let msg = PeerSharingMessage::from_cbor(&raw)
            .map_err(|e| PeerSharingClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgShareRequest` requesting up to `amount` peers and wait for
    /// `MsgSharePeers`.  Returns the list of shared peer addresses.
    ///
    /// The client must be in `StClient`.
    pub async fn share_request(
        &mut self,
        amount: u16,
    ) -> Result<Vec<SharedPeerAddress>, PeerSharingClientError> {
        self.send_msg(&PeerSharingMessage::MsgShareRequest { amount })
            .await?;
        let msg = self.recv_msg().await?;
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
