//! TxSubmission2 mini-protocol server driver.
//!
//! In TxSubmission2 the *server* drives the conversation by requesting
//! transaction identifiers and bodies from the *client*.  This driver
//! wraps a [`ProtocolHandle`] and maintains the state machine, providing
//! typed methods to send requests and receive replies.
//!
//! Reference: `Ouroboros.Network.Protocol.TxSubmission2.Server`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    TxIdAndSize, TxSubmissionMessage, TxSubmissionState, TxSubmissionTransitionError,
};
use yggdrasil_ledger::TxId;

// ---------------------------------------------------------------------------
// Server error
// ---------------------------------------------------------------------------

/// Errors from the TxSubmission server driver.
#[derive(Debug, thiserror::Error)]
pub enum TxSubmissionServerError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] TxSubmissionTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// Reply types
// ---------------------------------------------------------------------------

/// The result of a `request_tx_ids` call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxIdsReply {
    /// The client replied with transaction identifiers.
    TxIds(Vec<TxIdAndSize>),
    /// The client terminated the protocol (only valid from a blocking
    /// request).
    Done,
}

// ---------------------------------------------------------------------------
// TxSubmissionServer
// ---------------------------------------------------------------------------

/// A TxSubmission2 server driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`recv_init`] to receive the client's `MsgInit`.
/// 2. Call [`request_tx_ids`] to ask for transaction identifiers.
/// 3. Call [`request_txs`] to fetch specific transactions by id.
/// 4. Repeat from step 2 until the client sends `MsgDone`.
pub struct TxSubmissionServer {
    channel: MessageChannel,
    state: TxSubmissionState,
}

impl TxSubmissionServer {
    /// Create a new server driver from a TxSubmission `ProtocolHandle`.
    ///
    /// The protocol starts in `StInit` — the server waits for the client
    /// to send `MsgInit`.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: TxSubmissionState::StInit,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> TxSubmissionState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &TxSubmissionMessage) -> Result<(), TxSubmissionServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(TxSubmissionServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<TxSubmissionMessage, TxSubmissionServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(TxSubmissionServerError::ConnectionClosed)?;
        let msg = TxSubmissionMessage::from_cbor(&raw)
            .map_err(|e| TxSubmissionServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API: initialisation ---------------------------------------

    /// Wait for the client to send `MsgInit`.
    ///
    /// Must be called exactly once, immediately after construction.
    pub async fn recv_init(&mut self) -> Result<(), TxSubmissionServerError> {
        match self.recv_msg().await? {
            TxSubmissionMessage::MsgInit => Ok(()),
            msg => Err(TxSubmissionServerError::UnexpectedMessage(format!(
                "expected MsgInit, got {msg:?}"
            ))),
        }
    }

    // -- public API: server requests --------------------------------------

    /// Send `MsgRequestTxIds` and receive the client's reply.
    ///
    /// * `blocking` — if `true`, the client must reply with at least one
    ///   txid or send `MsgDone`.
    /// * `ack` — number of previously advertised txids to acknowledge.
    /// * `req` — maximum number of new txids to request.
    ///
    /// Returns `TxIdsReply::TxIds(..)` with the client's advertised
    /// identifiers, or `TxIdsReply::Done` when the client terminates.
    pub async fn request_tx_ids(
        &mut self,
        blocking: bool,
        ack: u16,
        req: u16,
    ) -> Result<TxIdsReply, TxSubmissionServerError> {
        self.send_msg(&TxSubmissionMessage::MsgRequestTxIds {
            blocking,
            ack,
            req,
        })
        .await?;

        match self.recv_msg().await? {
            TxSubmissionMessage::MsgReplyTxIds { txids } => Ok(TxIdsReply::TxIds(txids)),
            TxSubmissionMessage::MsgDone => Ok(TxIdsReply::Done),
            msg => Err(TxSubmissionServerError::UnexpectedMessage(format!(
                "expected MsgReplyTxIds or MsgDone, got {msg:?}"
            ))),
        }
    }

    /// Send `MsgRequestTxs` and receive the client's reply.
    ///
    /// Returns the list of serialized transaction bodies.
    pub async fn request_txs(
        &mut self,
        txids: Vec<TxId>,
    ) -> Result<Vec<Vec<u8>>, TxSubmissionServerError> {
        self.send_msg(&TxSubmissionMessage::MsgRequestTxs { txids })
            .await?;

        match self.recv_msg().await? {
            TxSubmissionMessage::MsgReplyTxs { txs } => Ok(txs),
            msg => Err(TxSubmissionServerError::UnexpectedMessage(format!(
                "expected MsgReplyTxs, got {msg:?}"
            ))),
        }
    }
}
