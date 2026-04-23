//! PeerSharing mini-protocol server (responder) driver.
//!
//! Wraps a [`ProtocolHandle`] with typed receive/send methods that maintain
//! the PeerSharing state machine invariants from the server side.  The
//! server receives peer-address requests and replies with known peers.
//!
//! Reference: `Ouroboros.Network.Protocol.PeerSharing.Server`.

use crate::connection::timeouts::PROTOCOL_RECV_TIMEOUT;
use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    PeerSharingMessage, PeerSharingState, PeerSharingTransitionError, SharedPeerAddress,
};

// ---------------------------------------------------------------------------
// Server error
// ---------------------------------------------------------------------------

/// Errors from the PeerSharing server driver.
#[derive(Debug, thiserror::Error)]
pub enum PeerSharingServerError {
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

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Per-state time limit exceeded (upstream `ExceededTimeLimit`).
    #[error("protocol timeout")]
    Timeout,
}

// ---------------------------------------------------------------------------
// Request type
// ---------------------------------------------------------------------------

/// A request received from the PeerSharing client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerSharingServerRequest {
    /// Client requested up to `amount` peer addresses.
    ShareRequest { amount: u16 },
    /// Client terminated the protocol.
    Done,
}

// ---------------------------------------------------------------------------
// PeerSharingServer
// ---------------------------------------------------------------------------

/// A PeerSharing server driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`Self::recv_request`] to wait for the client's next message.
/// 2. If `ShareRequest`, call [`Self::share_peers`] with available addresses.
/// 3. Repeat until `Done`.
pub struct PeerSharingServer {
    channel: MessageChannel,
    state: PeerSharingState,
}

impl PeerSharingServer {
    /// Create a new server driver from a PeerSharing `ProtocolHandle`.
    ///
    /// The protocol starts in `StClient` — client has agency first.
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

    async fn send_msg(&mut self, msg: &PeerSharingMessage) -> Result<(), PeerSharingServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(PeerSharingServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<PeerSharingMessage, PeerSharingServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(PeerSharingServerError::ConnectionClosed)?;
        let msg = PeerSharingMessage::from_cbor(&raw)
            .map_err(|e| PeerSharingServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Wait for the next client request.
    ///
    /// The server must be in `StClient` (awaiting client agency).  Times out
    /// after [`PROTOCOL_RECV_TIMEOUT`] if the client sends nothing (upstream
    /// `timeLimitsPeerSharing` `shortWait` for `StClient`).
    pub async fn recv_request(
        &mut self,
    ) -> Result<PeerSharingServerRequest, PeerSharingServerError> {
        let msg = tokio::time::timeout(PROTOCOL_RECV_TIMEOUT, self.recv_msg())
            .await
            .map_err(|_| PeerSharingServerError::Timeout)??;
        match msg {
            PeerSharingMessage::MsgShareRequest { amount } => {
                Ok(PeerSharingServerRequest::ShareRequest { amount })
            }
            PeerSharingMessage::MsgDone => Ok(PeerSharingServerRequest::Done),
            _ => Err(PeerSharingServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Send `MsgSharePeers` with the given list of peer addresses.
    ///
    /// The server must be in `StServer` (server has agency after a request).
    pub async fn share_peers(
        &mut self,
        peers: Vec<SharedPeerAddress>,
    ) -> Result<(), PeerSharingServerError> {
        self.send_msg(&PeerSharingMessage::MsgSharePeers { peers })
            .await
    }

    /// Serve requests in a loop until the client sends `MsgDone`.
    ///
    /// `provider` is called with the requested amount and should return
    /// the available peer addresses.
    pub async fn serve_loop<F>(&mut self, mut provider: F) -> Result<(), PeerSharingServerError>
    where
        F: FnMut(u16) -> Vec<SharedPeerAddress>,
    {
        loop {
            match self.recv_request().await? {
                PeerSharingServerRequest::ShareRequest { amount } => {
                    let peers = provider(amount);
                    self.share_peers(peers).await?;
                }
                PeerSharingServerRequest::Done => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_ps_server_connection_closed() {
        let s = format!("{}", PeerSharingServerError::ConnectionClosed);
        assert!(s.to_lowercase().contains("connection closed"));
    }

    #[test]
    fn display_ps_server_decode_propagates_inner() {
        let e = PeerSharingServerError::Decode("peer-address CBOR malformed".into());
        let s = format!("{e}");
        assert!(s.contains("CBOR decode"));
        assert!(s.contains("peer-address CBOR malformed"));
    }

    #[test]
    fn display_ps_server_unexpected_message_propagates_inner() {
        let e = PeerSharingServerError::UnexpectedMessage("MsgShareRequest in StBusy".into());
        let s = format!("{e}");
        assert!(s.contains("unexpected message"));
        assert!(s.contains("MsgShareRequest in StBusy"));
    }
}
