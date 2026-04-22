//! LocalTxSubmission mini-protocol server driver.
//!
//! In the LocalTxSubmission protocol the *client* (wallet / tooling) submits
//! a signed transaction and the *server* (the node) either accepts or rejects
//! it.  This driver wraps a [`ProtocolHandle`] and provides typed methods for
//! the server-agency states.
//!
//! Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Server`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    LocalTxSubmissionMessage, LocalTxSubmissionState, LocalTxSubmissionTransitionError,
};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the LocalTxSubmission server driver.
#[derive(Debug, thiserror::Error)]
pub enum LocalTxSubmissionServerError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] LocalTxSubmissionTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// Incoming client request
// ---------------------------------------------------------------------------

/// The result of receiving a client request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxRequest {
    /// The client submitted a transaction.
    SubmitTx {
        /// Raw transaction bytes (CBOR-encoded, era-specific).
        tx: Vec<u8>,
    },
    /// The client closed the protocol.
    Done,
}

// ---------------------------------------------------------------------------
// LocalTxSubmissionServer
// ---------------------------------------------------------------------------

/// A LocalTxSubmission server driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`Self::recv_request`] to wait for a client transaction or `MsgDone`.
/// 2. If a transaction was submitted, validate it and call [`Self::accept`] or
///    [`Self::reject`].
/// 3. Repeat from step 1 until `Done` is received.
pub struct LocalTxSubmissionServer {
    channel: MessageChannel,
    state: LocalTxSubmissionState,
}

impl LocalTxSubmissionServer {
    /// Create a new server driver from a LocalTxSubmission `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — the server waits for the client's
    /// first message.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: LocalTxSubmissionState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> LocalTxSubmissionState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(
        &mut self,
        msg: &LocalTxSubmissionMessage,
    ) -> Result<(), LocalTxSubmissionServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(LocalTxSubmissionServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<LocalTxSubmissionMessage, LocalTxSubmissionServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalTxSubmissionServerError::ConnectionClosed)?;
        let msg = LocalTxSubmissionMessage::from_cbor(&raw)
            .map_err(|e| LocalTxSubmissionServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Wait for the next client request.
    ///
    /// Returns [`LocalTxRequest::SubmitTx`] when the client submits a
    /// transaction, or [`LocalTxRequest::Done`] when the client closes the
    /// protocol.
    ///
    /// Must be called when the server is in `StIdle` (client agency).
    pub async fn recv_request(&mut self) -> Result<LocalTxRequest, LocalTxSubmissionServerError> {
        match self.recv_msg().await? {
            LocalTxSubmissionMessage::MsgSubmitTx { tx } => Ok(LocalTxRequest::SubmitTx { tx }),
            LocalTxSubmissionMessage::MsgDone => Ok(LocalTxRequest::Done),
            msg => Err(LocalTxSubmissionServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Accept the submitted transaction.
    ///
    /// Sends `MsgAcceptTx` and transitions back to `StIdle`.
    /// Must be called when the server is in `StBusy` (server agency).
    pub async fn accept(&mut self) -> Result<(), LocalTxSubmissionServerError> {
        self.send_msg(&LocalTxSubmissionMessage::MsgAcceptTx).await
    }

    /// Reject the submitted transaction with an opaque reason.
    ///
    /// Sends `MsgRejectTx(reason)` and transitions back to `StIdle`.
    /// Must be called when the server is in `StBusy` (server agency).
    ///
    /// The `reason` bytes are era-specific CBOR encoding of the rejection
    /// reason; the node layer produces these.
    pub async fn reject(&mut self, reason: Vec<u8>) -> Result<(), LocalTxSubmissionServerError> {
        self.send_msg(&LocalTxSubmissionMessage::MsgRejectTx { reason })
            .await
    }
}
