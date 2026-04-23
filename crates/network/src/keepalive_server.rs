//! KeepAlive mini-protocol server driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the KeepAlive state machine invariants. The server waits for
//! `MsgKeepAlive` from the client and echoes the cookie back via
//! `MsgKeepAliveResponse`.
//!
//! Reference: `Ouroboros.Network.Protocol.KeepAlive.Server`.

use crate::connection::timeouts::PROTOCOL_RECV_TIMEOUT;
use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{KeepAliveMessage, KeepAliveState, KeepAliveTransitionError};

// ---------------------------------------------------------------------------
// Server error
// ---------------------------------------------------------------------------

/// Errors from the KeepAlive server driver.
#[derive(Debug, thiserror::Error)]
pub enum KeepAliveServerError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] KeepAliveTransitionError),

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
// KeepAliveServer
// ---------------------------------------------------------------------------

/// A KeepAlive server driver maintaining the protocol state machine.
///
/// The server loop:
/// 1. Wait for `MsgKeepAlive` with a cookie.
/// 2. Echo the cookie via `MsgKeepAliveResponse`.
/// 3. Repeat until the client sends `MsgDone`.
pub struct KeepAliveServer {
    channel: MessageChannel,
    state: KeepAliveState,
}

impl KeepAliveServer {
    /// Create a new server driver from a KeepAlive `ProtocolHandle`.
    ///
    /// The protocol starts in `StClient` — the server waits for the
    /// client to send the first message.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: KeepAliveState::StClient,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> KeepAliveState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &KeepAliveMessage) -> Result<(), KeepAliveServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(KeepAliveServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<KeepAliveMessage, KeepAliveServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(KeepAliveServerError::ConnectionClosed)?;
        let msg = KeepAliveMessage::from_cbor(&raw)
            .map_err(|e| KeepAliveServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Wait for the next client message.
    ///
    /// Returns `Some(cookie)` when the client sends `MsgKeepAlive`, or
    /// `None` when the client sends `MsgDone` (protocol terminates).  Times
    /// out after [`PROTOCOL_RECV_TIMEOUT`] if the client stops sending keep-
    /// alive pings (upstream `timeLimitsKeepAlive` `shortWait` for `StClient`;
    /// generous to accommodate wide ping intervals).
    ///
    /// Must be called when the server is in `StClient` (client agency).
    pub async fn recv_keep_alive(&mut self) -> Result<Option<u16>, KeepAliveServerError> {
        let msg = tokio::time::timeout(PROTOCOL_RECV_TIMEOUT, self.recv_msg())
            .await
            .map_err(|_| KeepAliveServerError::Timeout)??;
        match msg {
            KeepAliveMessage::MsgKeepAlive { cookie } => Ok(Some(cookie)),
            KeepAliveMessage::MsgDone => Ok(None),
            msg => Err(KeepAliveServerError::UnexpectedMessage(format!("{msg:?}"))),
        }
    }

    /// Send `MsgKeepAliveResponse` echoing the given cookie.
    ///
    /// Must be called when the server is in `StServer` (server agency).
    pub async fn respond(&mut self, cookie: u16) -> Result<(), KeepAliveServerError> {
        self.send_msg(&KeepAliveMessage::MsgKeepAliveResponse { cookie })
            .await
    }

    /// Run the KeepAlive server loop until the client terminates.
    ///
    /// For each `MsgKeepAlive`, echoes the cookie via `MsgKeepAliveResponse`.
    /// Returns `Ok(())` when the client sends `MsgDone`.
    pub async fn serve_loop(&mut self) -> Result<(), KeepAliveServerError> {
        loop {
            match self.recv_keep_alive().await? {
                Some(cookie) => self.respond(cookie).await?,
                None => return Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── KeepAliveServerError Display-content tests ─────────────────────

    #[test]
    fn display_keepalive_server_connection_closed() {
        let s = format!("{}", KeepAliveServerError::ConnectionClosed);
        assert!(s.to_lowercase().contains("connection closed"));
    }

    #[test]
    fn display_keepalive_server_timeout() {
        let s = format!("{}", KeepAliveServerError::Timeout);
        assert!(s.to_lowercase().contains("timeout"));
    }

    #[test]
    fn display_keepalive_server_decode_propagates_inner() {
        let e = KeepAliveServerError::Decode("malformed cookie".into());
        let s = format!("{e}");
        assert!(s.contains("CBOR decode"));
        assert!(s.contains("malformed cookie"));
    }

    #[test]
    fn display_keepalive_server_unexpected_message_propagates_inner() {
        let e = KeepAliveServerError::UnexpectedMessage("MsgKeepAliveResponse in StIdle".into());
        let s = format!("{e}");
        assert!(s.contains("unexpected message"));
        assert!(s.contains("MsgKeepAliveResponse in StIdle"));
    }
}
