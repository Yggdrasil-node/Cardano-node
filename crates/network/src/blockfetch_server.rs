//! BlockFetch mini-protocol server driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the BlockFetch state machine invariants. The server receives range
//! requests from a client and streams blocks from storage.
//!
//! Reference: `Ouroboros.Network.Protocol.BlockFetch.Server`.

use crate::connection::timeouts::PROTOCOL_RECV_TIMEOUT;
use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocols::{BlockFetchMessage, BlockFetchState, BlockFetchTransitionError, ChainRange};

// ---------------------------------------------------------------------------
// Server error
// ---------------------------------------------------------------------------

/// Errors from the BlockFetch server driver.
#[derive(Debug, thiserror::Error)]
pub enum BlockFetchServerError {
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

    /// Unexpected message from the client.
    #[error("unexpected message: {0}")]
    UnexpectedMessage(String),

    /// Per-state time limit exceeded (upstream `ExceededTimeLimit`).
    #[error("protocol timeout")]
    Timeout,
}

// ---------------------------------------------------------------------------
// Server request
// ---------------------------------------------------------------------------

/// A request received from the BlockFetch client.
#[derive(Clone, Debug)]
pub enum BlockFetchServerRequest {
    /// Client requests blocks in the given range.
    RequestRange(ChainRange),
    /// Client terminates the protocol.
    ClientDone,
}

// ---------------------------------------------------------------------------
// BlockFetchServer
// ---------------------------------------------------------------------------

/// A BlockFetch server driver maintaining the protocol state machine.
///
/// The server loop:
/// 1. Wait for `MsgRequestRange` or `MsgClientDone`.
/// 2. On `MsgRequestRange`:
///    a. If blocks available: send `MsgStartBatch`, then stream `MsgBlock`
///    for each block, then send `MsgBatchDone`.
///    b. If no blocks: send `MsgNoBlocks`.
/// 3. Repeat until the client sends `MsgClientDone`.
pub struct BlockFetchServer {
    channel: MessageChannel,
    state: BlockFetchState,
}

impl BlockFetchServer {
    /// Create a new server driver from a BlockFetch `ProtocolHandle`.
    ///
    /// The protocol starts in `StIdle` — the server waits for the client
    /// to send the first request.
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

    async fn send_msg(&mut self, msg: &BlockFetchMessage) -> Result<(), BlockFetchServerError> {
        self.state = self.state.transition(msg)?;
        self.channel
            .send(msg.to_cbor())
            .await
            .map_err(BlockFetchServerError::Mux)
    }

    async fn recv_msg(&mut self) -> Result<BlockFetchMessage, BlockFetchServerError> {
        let raw = self
            .channel
            .recv()
            .await
            .ok_or(BlockFetchServerError::ConnectionClosed)?;
        let msg = BlockFetchMessage::from_cbor(&raw)
            .map_err(|e| BlockFetchServerError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Wait for the next client request in `StIdle`.
    ///
    /// Returns `RequestRange(range)` or `ClientDone`.  Times out after
    /// [`PROTOCOL_RECV_TIMEOUT`] if the client sends nothing (upstream
    /// `timeLimitsBlockFetch` `shortWait` for `StIdle`).
    pub async fn recv_request(&mut self) -> Result<BlockFetchServerRequest, BlockFetchServerError> {
        let msg = tokio::time::timeout(PROTOCOL_RECV_TIMEOUT, self.recv_msg())
            .await
            .map_err(|_| BlockFetchServerError::Timeout)??;
        match msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                Ok(BlockFetchServerRequest::RequestRange(range))
            }
            BlockFetchMessage::MsgClientDone => Ok(BlockFetchServerRequest::ClientDone),
            msg => Err(BlockFetchServerError::UnexpectedMessage(format!("{msg:?}"))),
        }
    }

    /// Signal that a batch of blocks is about to be streamed.
    ///
    /// Must be called from `StBusy`. Transitions to `StStreaming`.
    pub async fn start_batch(&mut self) -> Result<(), BlockFetchServerError> {
        self.send_msg(&BlockFetchMessage::MsgStartBatch).await
    }

    /// Signal that no blocks are available for the requested range.
    ///
    /// Must be called from `StBusy`. Transitions back to `StIdle`.
    pub async fn no_blocks(&mut self) -> Result<(), BlockFetchServerError> {
        self.send_msg(&BlockFetchMessage::MsgNoBlocks).await
    }

    /// Send a single block body during a streaming batch.
    ///
    /// Must be called from `StStreaming`. Remains in `StStreaming`.
    pub async fn send_block(&mut self, block: Vec<u8>) -> Result<(), BlockFetchServerError> {
        self.send_msg(&BlockFetchMessage::MsgBlock { block }).await
    }

    /// Signal the end of the current streaming batch.
    ///
    /// Must be called from `StStreaming`. Transitions back to `StIdle`.
    pub async fn batch_done(&mut self) -> Result<(), BlockFetchServerError> {
        self.send_msg(&BlockFetchMessage::MsgBatchDone).await
    }

    /// Serve a batch of blocks for the given range.
    ///
    /// If `blocks` is empty, sends `MsgNoBlocks`. Otherwise sends
    /// `MsgStartBatch`, streams each block, then sends `MsgBatchDone`.
    pub async fn serve_batch(&mut self, blocks: Vec<Vec<u8>>) -> Result<(), BlockFetchServerError> {
        if blocks.is_empty() {
            self.no_blocks().await
        } else {
            self.start_batch().await?;
            for block in blocks {
                self.send_block(block).await?;
            }
            self.batch_done().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BlockFetchServerError Display-content tests ────────────────────
    //
    // Server-side mirror of slice-69's client-side coverage. Server errors
    // appear in peer-attribution logs when we refuse a BlockFetch client
    // — diagnostic content must survive format-string refactors.

    #[test]
    fn display_blockfetch_server_connection_closed() {
        let s = format!("{}", BlockFetchServerError::ConnectionClosed);
        assert!(s.to_lowercase().contains("connection closed"));
    }

    #[test]
    fn display_blockfetch_server_timeout() {
        let s = format!("{}", BlockFetchServerError::Timeout);
        assert!(s.to_lowercase().contains("timeout"));
    }

    #[test]
    fn display_blockfetch_server_decode_propagates_inner() {
        let e = BlockFetchServerError::Decode("invalid range point".into());
        let s = format!("{e}");
        assert!(s.contains("CBOR decode"));
        assert!(s.contains("invalid range point"));
    }

    #[test]
    fn display_blockfetch_server_unexpected_message_propagates_inner() {
        let e = BlockFetchServerError::UnexpectedMessage("MsgRequestRange in StStreaming".into());
        let s = format!("{e}");
        assert!(s.contains("unexpected message"));
        assert!(s.contains("MsgRequestRange in StStreaming"));
    }
}
