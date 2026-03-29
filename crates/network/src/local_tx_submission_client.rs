//! LocalTxSubmission client driver (node-to-client).
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the [`LocalTxSubmissionState`] machine invariants.  The client submits
//! serialised transactions to the node's mempool and receives an accept or
//! reject response.
//!
//! Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Client`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    LocalTxSubmissionMessage, LocalTxSubmissionState, LocalTxSubmissionTransitionError,
};

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the LocalTxSubmission client driver.
#[derive(Debug, thiserror::Error)]
pub enum LocalTxSubmissionClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// Protocol state machine violation.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Transaction rejected by the node, with the raw era-specific rejection bytes.
    #[error("transaction rejected")]
    TransactionRejected(Vec<u8>),

    /// Unexpected message received from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

impl From<LocalTxSubmissionTransitionError> for LocalTxSubmissionClientError {
    fn from(e: LocalTxSubmissionTransitionError) -> Self {
        Self::Protocol(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// LocalTxSubmissionClient
// ---------------------------------------------------------------------------

/// A LocalTxSubmission client driver maintaining the protocol state machine.
///
/// ## Usage
/// 1. Call [`submit`](Self::submit) with serialised transaction bytes — the
///    driver sends `MsgSubmitTx` and waits for either `MsgAcceptTx` or
///    `MsgRejectTx`.
/// 2. Repeat step 1 for each transaction to submit.
/// 3. Call [`done`](Self::done) to terminate the protocol cleanly.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxSubmission.Client`.
pub struct LocalTxSubmissionClient {
    channel: MessageChannel,
    state: LocalTxSubmissionState,
}

impl LocalTxSubmissionClient {
    /// Create a new client driver from a LocalTxSubmission [`ProtocolHandle`].
    ///
    /// The protocol starts in `StIdle` — client agency.
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
    ) -> Result<(), LocalTxSubmissionClientError> {
        let next = self
            .state
            .transition(msg)
            .map_err(|e| LocalTxSubmissionClientError::Protocol(e.to_string()))?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(LocalTxSubmissionClientError::Mux)?;
        self.state = next;
        Ok(())
    }

    async fn recv_msg(&mut self) -> Result<LocalTxSubmissionMessage, LocalTxSubmissionClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalTxSubmissionClientError::ConnectionClosed)?;
        let msg = LocalTxSubmissionMessage::from_cbor(&raw)
            .map_err(|e| LocalTxSubmissionClientError::Protocol(e.to_string()))?;
        let next = self
            .state
            .transition(&msg)
            .map_err(|e| LocalTxSubmissionClientError::Protocol(e.to_string()))?;
        self.state = next;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Submit a serialised transaction and wait for the server response.
    ///
    /// Returns `Ok(())` if the node accepted the transaction into the mempool,
    /// or `Err(LocalTxSubmissionClientError::TransactionRejected(bytes))` if
    /// the node rejected it and returned era-specific rejection bytes.
    ///
    /// `tx` should be era-tagged CBOR (the same bytes you would submit via
    /// the cardano-submit-api or cardano-cli).
    ///
    /// The client must be in `StIdle` before calling this method.
    pub async fn submit(&mut self, tx: Vec<u8>) -> Result<(), LocalTxSubmissionClientError> {
        self.send_msg(&LocalTxSubmissionMessage::MsgSubmitTx { tx })
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalTxSubmissionMessage::MsgAcceptTx => Ok(()),
            LocalTxSubmissionMessage::MsgRejectTx { reason } => {
                Err(LocalTxSubmissionClientError::TransactionRejected(reason))
            }
            other => Err(LocalTxSubmissionClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Terminate the LocalTxSubmission protocol cleanly by sending `MsgDone`.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are valid.
    pub async fn done(mut self) -> Result<(), LocalTxSubmissionClientError> {
        self.send_msg(&LocalTxSubmissionMessage::MsgDone).await
    }
}
