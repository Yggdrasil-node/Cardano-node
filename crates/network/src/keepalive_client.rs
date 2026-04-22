//! KeepAlive mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the KeepAlive state machine invariants.  The client periodically sends
//! `MsgKeepAlive` with a cookie and expects the server to echo it back.
//!
//! Per-state time limits from `protocol_limits::keepalive` are enforced on
//! the server's response.  Upstream reference:
//! `Ouroboros.Network.Protocol.KeepAlive.Codec.timeLimitsKeepAlive`.
//!
//! Reference: `Ouroboros.Network.Protocol.KeepAlive.Client`.

use std::time::Duration;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocol_limits::keepalive as ka_limits;
use crate::protocols::{KeepAliveMessage, KeepAliveState, KeepAliveTransitionError};

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the KeepAlive client driver.
#[derive(Debug, thiserror::Error)]
pub enum KeepAliveClientError {
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
    Protocol(#[from] KeepAliveTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Server echoed a different cookie than the one we sent.
    #[error("cookie mismatch: sent {sent}, received {received}")]
    CookieMismatch { sent: u16, received: u16 },

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// KeepAliveClient
// ---------------------------------------------------------------------------

/// A KeepAlive client driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`Self::keep_alive`] with a cookie — the driver sends `MsgKeepAlive`
///    and waits for `MsgKeepAliveResponse`, verifying the echoed cookie.
/// 2. Repeat step 1 as many times as needed.
/// 3. Call [`Self::done`] to terminate the protocol cleanly.
pub struct KeepAliveClient {
    channel: MessageChannel,
    state: KeepAliveState,
}

impl KeepAliveClient {
    /// Create a new client driver from a KeepAlive `ProtocolHandle`.
    ///
    /// The protocol starts in `StClient` — client agency.
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

    async fn send_msg(&mut self, msg: &KeepAliveMessage) -> Result<(), KeepAliveClientError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(KeepAliveClientError::Mux)
    }

    /// Receive with an optional per-state time limit.
    async fn recv_msg_timeout(
        &mut self,
        limit: Option<Duration>,
    ) -> Result<KeepAliveMessage, KeepAliveClientError> {
        let raw = match limit {
            Some(d) => tokio::time::timeout(d, self.channel.recv())
                .await
                .map_err(|_| KeepAliveClientError::Timeout(d))?
                .ok_or(KeepAliveClientError::ConnectionClosed)?,
            None => self
                .channel
                .recv()
                .await
                .ok_or(KeepAliveClientError::ConnectionClosed)?,
        };
        let msg = KeepAliveMessage::from_cbor(&raw)
            .map_err(|e| KeepAliveClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgKeepAlive` with the given `cookie` and wait for
    /// `MsgKeepAliveResponse`.  Returns an error if the echoed cookie
    /// does not match.
    ///
    /// Enforces `keepalive::CLIENT` time limit (97 s) on the server's
    /// response.
    ///
    /// The client must be in `StClient`.
    pub async fn keep_alive(&mut self, cookie: u16) -> Result<(), KeepAliveClientError> {
        self.send_msg(&KeepAliveMessage::MsgKeepAlive { cookie })
            .await?;
        let msg = self.recv_msg_timeout(ka_limits::CLIENT).await?;
        match msg {
            KeepAliveMessage::MsgKeepAliveResponse {
                cookie: echoed_cookie,
            } => {
                if echoed_cookie == cookie {
                    Ok(())
                } else {
                    Err(KeepAliveClientError::CookieMismatch {
                        sent: cookie,
                        received: echoed_cookie,
                    })
                }
            }
            _ => Err(KeepAliveClientError::UnexpectedMessage(format!("{msg:?}"))),
        }
    }

    /// Send `MsgDone` to terminate the protocol cleanly.
    ///
    /// The client must be in `StClient`.  After this call the driver is in
    /// `StDone` and no further operations are possible.
    pub async fn done(mut self) -> Result<(), KeepAliveClientError> {
        self.send_msg(&KeepAliveMessage::MsgDone).await
    }
}
