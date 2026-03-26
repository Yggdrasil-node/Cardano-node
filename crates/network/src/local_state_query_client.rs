//! LocalStateQuery client driver (node-to-client).
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the [`LocalStateQueryState`] machine invariants.  Allows a client to
//! acquire a ledger snapshot at a given point and issue arbitrary typed
//! queries against that snapshot.
//!
//! Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Client`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    AcquireFailure, AcquireTarget, LocalStateQueryError, LocalStateQueryMessage,
    LocalStateQueryState,
};

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the LocalStateQuery client driver.
#[derive(Debug, thiserror::Error)]
pub enum LocalStateQueryClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// Protocol state machine violation or CBOR decode failure.
    #[error("protocol error: {0}")]
    Protocol(#[from] LocalStateQueryError),

    /// The server refused to acquire the requested point.
    #[error("acquire failed: {0:?}")]
    AcquireFailed(AcquireFailure),

    /// Unexpected message received from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// LocalStateQueryClient
// ---------------------------------------------------------------------------

/// A LocalStateQuery client driver maintaining the protocol state machine.
///
/// ## Typical workflow
/// 1. Call [`acquire`](Self::acquire) to acquire a ledger snapshot.
/// 2. Call [`query`](Self::query) with an era-tagged query payload — receive
///    the raw result bytes.
/// 3. Repeat step 2 for additional queries against the same snapshot.
/// 4. Call [`release`](Self::release) to return to `StIdle`, or
///    [`re_acquire`](Self::re_acquire) to atomically re-acquire.
/// 5. Call [`done`](Self::done) from `StIdle` to terminate the protocol.
///
/// Reference: `Ouroboros.Network.Protocol.LocalStateQuery.Client`.
pub struct LocalStateQueryClient {
    channel: MessageChannel,
    state: LocalStateQueryState,
}

impl LocalStateQueryClient {
    /// Create a new client driver from a LocalStateQuery [`ProtocolHandle`].
    ///
    /// The protocol starts in `StIdle` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: LocalStateQueryState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> LocalStateQueryState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(
        &mut self,
        msg: &LocalStateQueryMessage,
    ) -> Result<(), LocalStateQueryClientError> {
        let next = msg
            .apply(self.state)
            .ok_or_else(|| LocalStateQueryError::InvalidTransition {
                tag: msg.tag(),
                state: self.state,
            })?;
        self.channel
            .send(msg.encode_cbor())
            .await
            .map_err(LocalStateQueryClientError::Mux)?;
        self.state = next;
        Ok(())
    }

    async fn recv_msg(&mut self) -> Result<LocalStateQueryMessage, LocalStateQueryClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalStateQueryClientError::ConnectionClosed)?
            .map_err(LocalStateQueryClientError::Mux)?;
        let msg = LocalStateQueryMessage::decode_cbor(&raw)
            .map_err(LocalStateQueryClientError::Protocol)?;
        let next = msg
            .apply(self.state)
            .ok_or_else(|| LocalStateQueryError::InvalidTransition {
                tag: msg.tag(),
                state: self.state,
            })?;
        self.state = next;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Acquire a ledger snapshot at the given `target`.
    ///
    /// Sends `MsgAcquire` and waits for `MsgAcquired` or `MsgFailure`.
    /// Returns `Ok(())` on success, or `Err(AcquireFailed(_))` if the node
    /// cannot serve that point.
    ///
    /// The client must be in `StIdle`.  On success the driver is in `StAcquired`.
    pub async fn acquire(
        &mut self,
        target: AcquireTarget,
    ) -> Result<(), LocalStateQueryClientError> {
        self.send_msg(&LocalStateQueryMessage::MsgAcquire { target })
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalStateQueryMessage::MsgAcquired => Ok(()),
            LocalStateQueryMessage::MsgFailure { failure } => {
                Err(LocalStateQueryClientError::AcquireFailed(failure))
            }
            other => Err(LocalStateQueryClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Issue a query against the currently acquired snapshot.
    ///
    /// `query` should be an era-tagged CBOR-encoded query (e.g. `[era, query_body]`).
    /// Returns the raw CBOR result bytes from the server.
    ///
    /// The client must be in `StAcquired`.  After the call the driver is back
    /// in `StAcquired`.
    pub async fn query(&mut self, query: Vec<u8>) -> Result<Vec<u8>, LocalStateQueryClientError> {
        self.send_msg(&LocalStateQueryMessage::MsgQuery { query })
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalStateQueryMessage::MsgResult { result } => Ok(result),
            other => Err(LocalStateQueryClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Release the current snapshot and return to `StIdle`.
    ///
    /// The client must be in `StAcquired`.
    pub async fn release(&mut self) -> Result<(), LocalStateQueryClientError> {
        self.send_msg(&LocalStateQueryMessage::MsgRelease).await
    }

    /// Re-acquire at a new (or the same) `target` without returning to `StIdle`.
    ///
    /// Sends `MsgReAcquire` and waits for `MsgAcquired` or `MsgFailure`.
    ///
    /// The client must be in `StAcquired`.  On success the driver is in
    /// `StAcquired` (at the new snapshot); on failure it returns to `StIdle`
    /// via the error path — callers should check whether `StIdle` is the
    /// expected state before proceeding.
    pub async fn re_acquire(
        &mut self,
        target: AcquireTarget,
    ) -> Result<(), LocalStateQueryClientError> {
        self.send_msg(&LocalStateQueryMessage::MsgReAcquire { target })
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalStateQueryMessage::MsgAcquired => Ok(()),
            LocalStateQueryMessage::MsgFailure { failure } => {
                Err(LocalStateQueryClientError::AcquireFailed(failure))
            }
            other => Err(LocalStateQueryClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Terminate the LocalStateQuery protocol cleanly by sending `MsgDone`.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are valid.
    pub async fn done(mut self) -> Result<(), LocalStateQueryClientError> {
        self.send_msg(&LocalStateQueryMessage::MsgDone).await
    }
}
