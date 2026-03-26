//! LocalTxMonitor mini-protocol server driver.
//!
//! The LocalTxMonitor protocol lets a local client acquire a snapshot of the
//! node's mempool and iterate over its contents, check transaction membership,
//! or query aggregate sizes.  This driver wraps a [`ProtocolHandle`] and
//! exposes typed methods for each server-agency state.
//!
//! Transaction bodies and identifiers remain opaque (`Vec<u8>`) at this layer.
//! The node layer translates between mempool entries and wire-format bytes.
//!
//! Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Server`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{
    LocalTxMonitorMessage, LocalTxMonitorState, LocalTxMonitorTransitionError,
};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the LocalTxMonitor server driver.
#[derive(Debug, thiserror::Error)]
pub enum LocalTxMonitorServerError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] LocalTxMonitorTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// Typed requests
// ---------------------------------------------------------------------------

/// A request received from the client in the `StIdle` state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxMonitorIdleRequest {
    /// Client wants to acquire a mempool snapshot.
    Acquire,
    /// Client terminates the protocol.
    Done,
}

/// A request received while a mempool snapshot is acquired (`StAcquired`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalTxMonitorAcquiredRequest {
    /// Client requests the next transaction from the snapshot iterator.
    NextTx,
    /// Client asks whether a given transaction is in the mempool.
    HasTx {
        /// Transaction identifier (typically 32-byte Blake2b-256 hash).
        tx_id: Vec<u8>,
    },
    /// Client requests aggregate mempool size information.
    GetSizes,
    /// Client releases the snapshot and returns to idle.
    Release,
    /// Client re-acquires a fresh snapshot (equivalent to release + acquire).
    AwaitAcquire,
}

// ---------------------------------------------------------------------------
// LocalTxMonitorServer
// ---------------------------------------------------------------------------

/// A LocalTxMonitor server driver maintaining the protocol state machine.
///
/// Server loop:
/// 1. Call [`recv_idle_request`] — either `Acquire` or `Done`.
/// 2. Take a mempool snapshot, respond with [`acquired`] (sending the
///    current slot).
/// 3. Loop on [`recv_acquired_request`] until `Release` or `AwaitAcquire`:
///    - `NextTx`: call [`reply_next_tx`] with the next tx (or `None`).
///    - `HasTx`: call [`reply_has_tx`] with membership result.
///    - `GetSizes`: call [`reply_get_sizes`] with capacity/size/count.
///    - `Release`: returns to idle (go to step 1).
///    - `AwaitAcquire`: re-snapshot, send [`acquired`], stay in step 3.
/// 4. When `Done` is received, the session ends.
///
/// [`recv_idle_request`]: Self::recv_idle_request
/// [`acquired`]: Self::acquired
/// [`recv_acquired_request`]: Self::recv_acquired_request
/// [`reply_next_tx`]: Self::reply_next_tx
/// [`reply_has_tx`]: Self::reply_has_tx
/// [`reply_get_sizes`]: Self::reply_get_sizes
pub struct LocalTxMonitorServer {
    channel: MessageChannel,
    state: LocalTxMonitorState,
}

impl LocalTxMonitorServer {
    /// Create a new server driver from a LocalTxMonitor `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — the server waits for the first
    /// client request.
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
    ) -> Result<(), LocalTxMonitorServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(LocalTxMonitorServerError::Mux)
    }

    async fn recv_msg(
        &mut self,
    ) -> Result<LocalTxMonitorMessage, LocalTxMonitorServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(LocalTxMonitorServerError::ConnectionClosed)?;
        let msg = LocalTxMonitorMessage::from_cbor(&raw)
            .map_err(|e| LocalTxMonitorServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Wait for the next idle-state request from the client.
    ///
    /// Returns either `Acquire` or `Done`.
    ///
    /// Must be called when the server is in `StIdle`.
    pub async fn recv_idle_request(
        &mut self,
    ) -> Result<LocalTxMonitorIdleRequest, LocalTxMonitorServerError> {
        match self.recv_msg().await? {
            LocalTxMonitorMessage::MsgAcquire => Ok(LocalTxMonitorIdleRequest::Acquire),
            LocalTxMonitorMessage::MsgDone => Ok(LocalTxMonitorIdleRequest::Done),
            msg => Err(LocalTxMonitorServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Confirm mempool snapshot acquisition at the given slot.
    ///
    /// Sends `MsgAcquired(slot_no)` and transitions to `StAcquired`.
    /// Must be called when the server is in `StAcquiring`.
    pub async fn acquired(
        &mut self,
        slot_no: u64,
    ) -> Result<(), LocalTxMonitorServerError> {
        self.send_msg(&LocalTxMonitorMessage::MsgAcquired { slot_no })
            .await
    }

    /// Wait for the next request from the client while a snapshot is acquired.
    ///
    /// Must be called when the server is in `StAcquired`.
    pub async fn recv_acquired_request(
        &mut self,
    ) -> Result<LocalTxMonitorAcquiredRequest, LocalTxMonitorServerError> {
        match self.recv_msg().await? {
            LocalTxMonitorMessage::MsgNextTx => Ok(LocalTxMonitorAcquiredRequest::NextTx),
            LocalTxMonitorMessage::MsgHasTx { tx_id } => {
                Ok(LocalTxMonitorAcquiredRequest::HasTx { tx_id })
            }
            LocalTxMonitorMessage::MsgGetSizes => Ok(LocalTxMonitorAcquiredRequest::GetSizes),
            LocalTxMonitorMessage::MsgRelease => Ok(LocalTxMonitorAcquiredRequest::Release),
            LocalTxMonitorMessage::MsgAcquire => {
                Ok(LocalTxMonitorAcquiredRequest::AwaitAcquire)
            }
            msg => Err(LocalTxMonitorServerError::UnexpectedMessage(format!(
                "{msg:?}"
            ))),
        }
    }

    /// Reply to a `MsgNextTx` with the next transaction or `None`.
    ///
    /// Pass `Some(tx_bytes)` for the next CBOR-encoded transaction from the
    /// snapshot iterator, or `None` when the iterator is exhausted.
    ///
    /// Must be called when the last received message was `MsgNextTx`.
    pub async fn reply_next_tx(
        &mut self,
        tx: Option<Vec<u8>>,
    ) -> Result<(), LocalTxMonitorServerError> {
        self.send_msg(&LocalTxMonitorMessage::MsgReplyNextTx { tx })
            .await
    }

    /// Reply to a `MsgHasTx` with membership result.
    ///
    /// Must be called when the last received message was `MsgHasTx`.
    pub async fn reply_has_tx(
        &mut self,
        has_tx: bool,
    ) -> Result<(), LocalTxMonitorServerError> {
        self.send_msg(&LocalTxMonitorMessage::MsgReplyHasTx { has_tx })
            .await
    }

    /// Reply to a `MsgGetSizes` with aggregate mempool metrics.
    ///
    /// Must be called when the last received message was `MsgGetSizes`.
    pub async fn reply_get_sizes(
        &mut self,
        capacity_in_bytes: u32,
        size_in_bytes: u32,
        num_txs: u32,
    ) -> Result<(), LocalTxMonitorServerError> {
        self.send_msg(&LocalTxMonitorMessage::MsgReplyGetSizes {
            capacity_in_bytes,
            size_in_bytes,
            num_txs,
        })
        .await
    }
}
