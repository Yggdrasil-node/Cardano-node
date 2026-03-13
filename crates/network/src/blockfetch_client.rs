//! BlockFetch mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the BlockFetch state machine invariants.  The driver operates at the
//! client-agency level: it sends `MsgRequestRange` and `MsgClientDone`,
//! and receives streaming blocks from the server.
//!
//! Reference: `Ouroboros.Network.Protocol.BlockFetch.Client`.

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{BlockFetchMessage, BlockFetchState, BlockFetchTransitionError, ChainRange};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// The server's response to a `MsgRequestRange`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatchResponse {
    /// The server started streaming — call [`BlockFetchClient::recv_block`]
    /// to retrieve individual blocks until `None` signals `MsgBatchDone`.
    StartedBatch,
    /// The server had no blocks for the requested range.
    NoBlocks,
}

// ---------------------------------------------------------------------------
// Client error
// ---------------------------------------------------------------------------

/// Errors from the BlockFetch client driver.
#[derive(Debug, thiserror::Error)]
pub enum BlockFetchClientError {
    /// Multiplexer transport error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// Connection closed by the remote peer.
    #[error("connection closed")]
    ConnectionClosed,

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] BlockFetchTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Unexpected message from the server.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),
}

// ---------------------------------------------------------------------------
// BlockFetchClient
// ---------------------------------------------------------------------------

/// A BlockFetch client driver maintaining the protocol state machine.
///
/// Usage:
/// 1. Call [`request_range`] to initiate a batch.
/// 2. If `BatchResponse::StartedBatch`, call [`recv_block`] repeatedly
///    until it returns `None` (which indicates `MsgBatchDone`).
/// 3. Repeat from step 1 for more ranges, or call [`done`] to terminate.
pub struct BlockFetchClient {
    channel: MessageChannel,
    state: BlockFetchState,
}

impl BlockFetchClient {
    /// Create a new client driver from a BlockFetch `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — client agency.
    pub fn new(handle: ProtocolHandle) -> Self {
        Self {
            channel: MessageChannel::new(handle),
            state: BlockFetchState::StIdle,
        }
    }

    /// Returns the current protocol state.
    pub fn state(&self) -> BlockFetchState {
        self.state
    }

    // -- helpers ----------------------------------------------------------

    async fn send_msg(&mut self, msg: &BlockFetchMessage) -> Result<(), BlockFetchClientError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(BlockFetchClientError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<BlockFetchMessage, BlockFetchClientError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(BlockFetchClientError::ConnectionClosed)?;
        let msg = BlockFetchMessage::from_cbor(&raw)
            .map_err(|e| BlockFetchClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgRequestRange` and wait for `MsgStartBatch` or `MsgNoBlocks`.
    ///
    /// The client must be in `StIdle`.  If `BatchResponse::StartedBatch` is
    /// returned the client enters `StStreaming` and the caller should call
    /// [`recv_block`] to consume the stream.
    pub async fn request_range(
        &mut self,
        range: ChainRange,
    ) -> Result<BatchResponse, BlockFetchClientError> {
        self.send_msg(&BlockFetchMessage::MsgRequestRange(range))
            .await?;
        let msg = self.recv_msg().await?;
        match msg {
            BlockFetchMessage::MsgStartBatch => Ok(BatchResponse::StartedBatch),
            BlockFetchMessage::MsgNoBlocks => Ok(BatchResponse::NoBlocks),
            other => Err(BlockFetchClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Receive the next block in the current batch.
    ///
    /// Returns `Some(block_bytes)` for each `MsgBlock`, or `None` when the
    /// server sends `MsgBatchDone` (returning to `StIdle`).
    ///
    /// The client must be in `StStreaming`.
    pub async fn recv_block(&mut self) -> Result<Option<Vec<u8>>, BlockFetchClientError> {
        let msg = self.recv_msg().await?;
        match msg {
            BlockFetchMessage::MsgBlock { block } => Ok(Some(block)),
            BlockFetchMessage::MsgBatchDone => Ok(None),
            other => Err(BlockFetchClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Send `MsgClientDone` to terminate the protocol cleanly.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are possible.
    pub async fn done(mut self) -> Result<(), BlockFetchClientError> {
        self.send_msg(&BlockFetchMessage::MsgClientDone).await
    }
}
