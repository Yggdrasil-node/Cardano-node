//! TxSubmission2 mini-protocol client driver.
//!
//! The TxSubmission2 protocol is pull-based: the *server* requests transaction
//! identifiers and bodies from the *client*.  This driver wraps a
//! [`ProtocolHandle`] and maintains the state machine, providing typed
//! methods to initialise the protocol, receive server requests, and send
//! replies.
//!
//! Reference: `Ouroboros.Network.Protocol.TxSubmission2.Client`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    TxIdAndSize, TxSubmissionMessage, TxSubmissionState, TxSubmissionTransitionError,
};
use yggdrasil_ledger::{Tx, TxId};

// ---------------------------------------------------------------------------
// Server request types
// ---------------------------------------------------------------------------

/// A request from the server to the TxSubmission client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxServerRequest {
    /// The server asks for transaction identifiers.
    RequestTxIds {
        /// `true` if the server is blocking (wants non-empty reply or MsgDone).
        blocking: bool,
        /// Number of previously advertised txids to acknowledge.
        ack: u16,
        /// Maximum number of new txids the server wants.
        req: u16,
    },
    /// The server asks for specific transactions by id.
    RequestTxs {
        /// Transaction identifiers to fetch.
        txids: Vec<TxId>,
    },
}

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the TxSubmission client driver.
#[derive(Debug, thiserror::Error)]
pub enum TxSubmissionClientError {
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

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// TxSubmissionClient
// ---------------------------------------------------------------------------

/// A TxSubmission2 client driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`init`] to send `MsgInit`.
/// 2. Call [`recv_request`] to receive the next server request.
/// 3. Call [`reply_tx_ids`] or [`reply_txs`] depending on the request.
/// 4. Repeat from step 2.
/// 5. Call [`done`] from a blocking `StTxIds` state to terminate.
pub struct TxSubmissionClient {
    channel: MessageChannel,
    state: TxSubmissionState,
}

impl TxSubmissionClient {
    /// Create a new client driver from a TxSubmission `ProtocolHandle`.
    ///
    /// The protocol starts in `StInit` — client agency.
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

    async fn send_msg(
        &mut self,
        msg: &TxSubmissionMessage,
    ) -> Result<(), TxSubmissionClientError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(TxSubmissionClientError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<TxSubmissionMessage, TxSubmissionClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(TxSubmissionClientError::ConnectionClosed)?;
        let msg = TxSubmissionMessage::from_cbor(&raw)
            .map_err(|e| TxSubmissionClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgInit` to initialise the protocol.
    ///
    /// Must be called exactly once, immediately after construction.
    pub async fn init(&mut self) -> Result<(), TxSubmissionClientError> {
        self.send_msg(&TxSubmissionMessage::MsgInit).await
    }

    /// Wait for the next server request.
    ///
    /// The client must be in `StIdle` (server agency). Returns either
    /// `TxServerRequest::RequestTxIds` or `TxServerRequest::RequestTxs`.
    pub async fn recv_request(&mut self) -> Result<TxServerRequest, TxSubmissionClientError> {
        let msg = self.recv_msg().await?;
        match msg {
            TxSubmissionMessage::MsgRequestTxIds { blocking, ack, req } => {
                Ok(TxServerRequest::RequestTxIds { blocking, ack, req })
            }
            TxSubmissionMessage::MsgRequestTxs { txids } => {
                Ok(TxServerRequest::RequestTxs { txids })
            }
            other => Err(TxSubmissionClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Reply with transaction identifiers.
    ///
    /// The client must be in `StTxIds`.
    pub async fn reply_tx_ids(
        &mut self,
        txids: Vec<TxIdAndSize>,
    ) -> Result<(), TxSubmissionClientError> {
        self.send_msg(&TxSubmissionMessage::MsgReplyTxIds { txids })
            .await
    }

    /// Reply with transaction bodies.
    ///
    /// The client must be in `StTxs`.
    pub async fn reply_txs(
        &mut self,
        txs: Vec<Vec<u8>>,
    ) -> Result<(), TxSubmissionClientError> {
        self.send_msg(&TxSubmissionMessage::MsgReplyTxs { txs })
            .await
    }

    /// Reply with typed ledger transactions.
    ///
    /// The wire protocol carries only serialized transaction bodies, so this
    /// helper strips the canonical `Tx` wrapper to preserve a typed client API.
    pub async fn reply_txs_typed(
        &mut self,
        txs: Vec<Tx>,
    ) -> Result<(), TxSubmissionClientError> {
        self.reply_txs(txs.into_iter().map(|tx| tx.body).collect())
            .await
    }

    /// Send `MsgDone` to terminate the protocol.
    ///
    /// The client must be in `StTxIds { blocking: true }`.
    pub async fn done(mut self) -> Result<(), TxSubmissionClientError> {
        self.send_msg(&TxSubmissionMessage::MsgDone).await
    }
}
