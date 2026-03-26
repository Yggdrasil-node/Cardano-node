//! LocalTxMonitor client driver (node-to-client).
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the [`LocalTxMonitorState`] machine invariants.  Allows a client to
//! observe the node's mempool: acquire a snapshot, iterate pending
//! transactions, check membership, and query size/capacity.
//!
//! Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Client`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    LocalTxMonitorError, LocalTxMonitorMessage, LocalTxMonitorState, MempoolSizeAndCapacity,
};

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the LocalTxMonitor client driver.
#[derive(Debug, thiserror::Error)]
pub enum LocalTxMonitorClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// Protocol state machine violation or CBOR decode failure.
    #[error("protocol error: {0}")]
    Protocol(#[from] LocalTxMonitorError),

    /// Unexpected message received from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// Acquired snapshot handle
// ---------------------------------------------------------------------------

/// A handle to an acquired mempool snapshot, obtained via
/// [`LocalTxMonitorClient::acquire`].
///
/// Carries the slot number at which the snapshot was taken.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Client.ClientStAcquired`.
#[derive(Clone, Copy, Debug)]
pub struct MempoolSnapshot {
    /// Slot at which this snapshot was taken.
    pub slot_no: u64,
}

// ---------------------------------------------------------------------------
// LocalTxMonitorClient
// ---------------------------------------------------------------------------

/// A LocalTxMonitor client driver maintaining the protocol state machine.
///
/// ## Typical workflow
/// 1. Call [`acquire`](Self::acquire) to acquire a mempool snapshot.
/// 2. Call [`next_tx`](Self::next_tx) repeatedly until `None` is returned
///    (snapshot exhausted).
/// 3. Call [`get_sizes`](Self::get_sizes) to check current mempool capacity.
/// 4. Call [`release`](Self::release) to return to `StIdle`.
/// 5. Call [`done`](Self::done) from `StIdle` to terminate the protocol.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Client`.
pub struct LocalTxMonitorClient {
    channel: MessageChannel,
    state: LocalTxMonitorState,
}

impl LocalTxMonitorClient {
    /// Create a new client driver from a LocalTxMonitor [`ProtocolHandle`].
    ///
    /// The protocol starts in `StIdle` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: LocalTxMonitorState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> LocalTxMonitorState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(
        &mut self,
        msg: &LocalTxMonitorMessage,
    ) -> Result<(), LocalTxMonitorClientError> {
        let next = msg
            .apply(self.state)
            .ok_or_else(|| LocalTxMonitorError::InvalidTransition {
                tag: msg.tag(),
                state: self.state,
            })?;
        self.channel
            .send(msg.encode_cbor())
            .await
            .map_err(LocalTxMonitorClientError::Mux)?;
        self.state = next;
        Ok(())
    }

    async fn recv_msg(&mut self) -> Result<LocalTxMonitorMessage, LocalTxMonitorClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalTxMonitorClientError::ConnectionClosed)?;
        let msg = LocalTxMonitorMessage::decode_cbor(&raw)
            .map_err(LocalTxMonitorClientError::Protocol)?;
        let next = msg
            .apply(self.state)
            .ok_or_else(|| LocalTxMonitorError::InvalidTransition {
                tag: msg.tag(),
                state: self.state,
            })?;
        self.state = next;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Acquire a new mempool snapshot.
    ///
    /// Sends `MsgAcquire` and waits for `MsgAcquired`.  Returns a
    /// [`MempoolSnapshot`] carrying the snapshot slot.
    ///
    /// The client must be in `StIdle`.  On success the driver is in
    /// `StAcquired`.
    pub async fn acquire(&mut self) -> Result<MempoolSnapshot, LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgAcquire).await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalTxMonitorMessage::MsgAcquired { slot_no } => Ok(MempoolSnapshot { slot_no }),
            other => Err(LocalTxMonitorClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Ask the server to wait until the mempool changes and then re-acquire.
    ///
    /// Sends `MsgAwaitAcquire` and waits for `MsgAcquired`.  Returns a new
    /// [`MempoolSnapshot`] after the mempool has changed.
    ///
    /// The client must be in `StAcquired`.  On success the driver is in
    /// `StAcquired`.
    pub async fn await_acquire(&mut self) -> Result<MempoolSnapshot, LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgAwaitAcquire)
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalTxMonitorMessage::MsgAcquired { slot_no } => Ok(MempoolSnapshot { slot_no }),
            other => Err(LocalTxMonitorClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Fetch the next transaction in the current snapshot.
    ///
    /// Returns `Some(tx_bytes)` for the next pending transaction, or `None`
    /// when the snapshot has been fully iterated.
    ///
    /// The client must be in `StAcquired`.  After the call the driver is back
    /// in `StAcquired`.
    pub async fn next_tx(&mut self) -> Result<Option<Vec<u8>>, LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgNextTx).await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalTxMonitorMessage::MsgReplyNextTx { tx } => Ok(tx),
            other => Err(LocalTxMonitorClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Check whether a transaction with the given `tx_id` is in the snapshot.
    ///
    /// The client must be in `StAcquired`.  After the call the driver is back
    /// in `StAcquired`.
    pub async fn has_tx(&mut self, tx_id: Vec<u8>) -> Result<bool, LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgHasTx { tx_id })
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalTxMonitorMessage::MsgReplyHasTx { has_tx } => Ok(has_tx),
            other => Err(LocalTxMonitorClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Query the current mempool size and capacity.
    ///
    /// Returns a [`MempoolSizeAndCapacity`] with the byte capacity, current
    /// byte size, and number of transactions in the mempool.
    ///
    /// The client must be in `StAcquired`.  After the call the driver is back
    /// in `StAcquired`.
    pub async fn get_sizes(&mut self) -> Result<MempoolSizeAndCapacity, LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgGetSizes).await?;
        let msg = self.recv_msg().await?;
        match msg {
            LocalTxMonitorMessage::MsgReplyGetSizes { sizes } => Ok(sizes),
            other => Err(LocalTxMonitorClientError::UnexpectedMessage(format!(
                "{other:?}"
            ))),
        }
    }

    /// Release the current snapshot and return to `StIdle`.
    ///
    /// The client must be in `StAcquired`.
    pub async fn release(&mut self) -> Result<(), LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgRelease).await
    }

    /// Terminate the LocalTxMonitor protocol cleanly by sending `MsgDone`.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are valid.
    pub async fn done(mut self) -> Result<(), LocalTxMonitorClientError> {
        self.send_msg(&LocalTxMonitorMessage::MsgDone).await
    }
}
