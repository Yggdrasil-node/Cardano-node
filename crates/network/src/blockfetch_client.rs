//! BlockFetch mini-protocol client driver.
//!
//! Wraps a [`ProtocolHandle`] with typed send/receive methods that maintain
//! the BlockFetch state machine invariants.  The driver operates at the
//! client-agency level: it sends `MsgRequestRange` and `MsgClientDone`,
//! and receives streaming blocks from the server.
//!
//! Per-state time limits from `protocol_limits::blockfetch` are enforced on
//! every server response.  Upstream reference:
//! `Ouroboros.Network.Protocol.BlockFetch.Codec.timeLimitsBlockFetch`.
//!
//! Reference: `Ouroboros.Network.Protocol.BlockFetch.Client`.

use std::time::Duration;

use crate::mux::{MessageChannel, MuxError, ProtocolHandle};
use crate::protocol_limits::blockfetch as bf_limits;
use crate::protocols::{BlockFetchMessage, BlockFetchState, BlockFetchTransitionError, ChainRange};
use yggdrasil_ledger::{CborDecode, CborEncode, LedgerError, Point};

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

    /// The server did not respond within the per-state time limit.
    ///
    /// Upstream: `ExceededTimeLimit` from `ProtocolTimeLimits`.
    #[error("protocol timeout ({0:?})")]
    Timeout(Duration),

    /// State machine violation.
    #[error("protocol error: {0}")]
    Protocol(#[from] BlockFetchTransitionError),

    /// CBOR decode failure.
    #[error("CBOR decode error: {0}")]
    Decode(String),

    /// Decoding a streamed block payload into a domain type failed.
    #[error("block decode error: {0}")]
    BlockDecode(#[from] LedgerError),

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
/// 1. Call [`Self::request_range`] to initiate a batch.
/// 2. If `BatchResponse::StartedBatch`, call [`Self::recv_block`] repeatedly
///    until it returns `None` (which indicates `MsgBatchDone`).
/// 3. Repeat from step 1 for more ranges, or call [`Self::done`] to terminate.
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

    /// Receive with an optional per-state time limit.
    async fn recv_msg_timeout(
        &mut self,
        limit: Option<Duration>,
    ) -> Result<BlockFetchMessage, BlockFetchClientError> {
        let raw = match limit {
            Some(d) => tokio::time::timeout(d, self.channel.recv())
                .await
                .map_err(|_| BlockFetchClientError::Timeout(d))?
                .ok_or(BlockFetchClientError::ConnectionClosed)?,
            None => self
                .channel
                .recv()
                .await
                .ok_or(BlockFetchClientError::ConnectionClosed)?,
        };
        let msg = BlockFetchMessage::from_cbor(&raw)
            .map_err(|e| BlockFetchClientError::Decode(e.to_string()))?;
        self.state = self.state.transition(&msg)?;
        Ok(msg)
    }

    // -- public API -------------------------------------------------------

    /// Send `MsgRequestRange` and wait for `MsgStartBatch` or `MsgNoBlocks`.
    ///
    /// Enforces `blockfetch::BF_BUSY` time limit (60 s) on the server's
    /// batch-start response.
    ///
    /// The client must be in `StIdle`.  If `BatchResponse::StartedBatch` is
    /// returned the client enters `StStreaming` and the caller should call
    /// [`Self::recv_block`] to consume the stream.
    pub async fn request_range(
        &mut self,
        range: ChainRange,
    ) -> Result<BatchResponse, BlockFetchClientError> {
        let msg = BlockFetchMessage::MsgRequestRange(range);
        if std::env::var("YGG_SYNC_DEBUG").is_ok_and(|v| v != "0") {
            use std::fmt::Write;
            let bytes = msg.to_cbor();
            let hex = bytes
                .iter()
                .fold(String::with_capacity(bytes.len() * 2), |mut acc, b| {
                    let _ = write!(acc, "{b:02x}");
                    acc
                });
            eprintln!("[ygg-sync-debug] blockfetch-request-cbor={hex}");
        }
        self.send_msg(&msg).await?;
        let msg = self.recv_msg_timeout(bf_limits::BF_BUSY).await?;
        match msg {
            BlockFetchMessage::MsgStartBatch => Ok(BatchResponse::StartedBatch),
            BlockFetchMessage::MsgNoBlocks => Ok(BatchResponse::NoBlocks),
            other => Err(BlockFetchClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Send `MsgRequestRange` using typed ledger points for the lower and
    /// upper bounds.
    pub async fn request_range_points(
        &mut self,
        lower: Point,
        upper: Point,
    ) -> Result<BatchResponse, BlockFetchClientError> {
        self.request_range(ChainRange {
            lower: lower.to_cbor_bytes(),
            upper: upper.to_cbor_bytes(),
        })
        .await
    }

    /// Request a range and collect the full batch as raw block payloads.
    ///
    /// If the server returns `MsgNoBlocks`, an empty vector is returned.
    pub async fn request_range_collect(
        &mut self,
        range: ChainRange,
    ) -> Result<Vec<Vec<u8>>, BlockFetchClientError> {
        let mut blocks = Vec::new();
        match self.request_range(range).await? {
            BatchResponse::NoBlocks => Ok(blocks),
            BatchResponse::StartedBatch => {
                while let Some(block) = self.recv_block().await? {
                    blocks.push(block);
                }
                Ok(blocks)
            }
        }
    }

    /// Request a typed point range and collect the full batch as raw block
    /// payloads.
    pub async fn request_range_collect_points(
        &mut self,
        lower: Point,
        upper: Point,
    ) -> Result<Vec<Vec<u8>>, BlockFetchClientError> {
        self.request_range_collect(ChainRange {
            lower: lower.to_cbor_bytes(),
            upper: upper.to_cbor_bytes(),
        })
        .await
    }

    /// Receive the next block in the current batch.
    ///
    /// Enforces `blockfetch::BF_STREAMING` time limit (60 s) between
    /// successive blocks.
    ///
    /// Returns `Some(block_bytes)` for each `MsgBlock`, or `None` when the
    /// server sends `MsgBatchDone` (returning to `StIdle`).
    ///
    /// The client must be in `StStreaming`.
    pub async fn recv_block(&mut self) -> Result<Option<Vec<u8>>, BlockFetchClientError> {
        let msg = self.recv_msg_timeout(bf_limits::BF_STREAMING).await?;
        match msg {
            BlockFetchMessage::MsgBlock { block } => Ok(Some(block)),
            BlockFetchMessage::MsgBatchDone => Ok(None),
            other => Err(BlockFetchClientError::UnexpectedMessage(
                other.tag_name().to_string(),
            )),
        }
    }

    /// Receive the next block in the current batch and decode it into a typed
    /// ledger/domain value.
    ///
    /// Returns `Some(block)` for each `MsgBlock`, or `None` when the server
    /// sends `MsgBatchDone` (returning to `StIdle`).
    pub async fn recv_block_decoded<B: CborDecode>(
        &mut self,
    ) -> Result<Option<B>, BlockFetchClientError> {
        match self.recv_block().await? {
            Some(block) => Ok(Some(B::from_cbor_bytes(&block)?)),
            None => Ok(None),
        }
    }

    /// Request a range and collect the full batch as decoded blocks.
    pub async fn request_range_collect_decoded<B: CborDecode>(
        &mut self,
        range: ChainRange,
    ) -> Result<Vec<B>, BlockFetchClientError> {
        let mut blocks = Vec::new();
        match self.request_range(range).await? {
            BatchResponse::NoBlocks => Ok(blocks),
            BatchResponse::StartedBatch => {
                while let Some(block) = self.recv_block_decoded::<B>().await? {
                    blocks.push(block);
                }
                Ok(blocks)
            }
        }
    }

    /// Request a typed point range and collect the full batch as decoded
    /// blocks.
    pub async fn request_range_collect_points_decoded<B: CborDecode>(
        &mut self,
        lower: Point,
        upper: Point,
    ) -> Result<Vec<B>, BlockFetchClientError> {
        self.request_range_collect_decoded(ChainRange {
            lower: lower.to_cbor_bytes(),
            upper: upper.to_cbor_bytes(),
        })
        .await
    }

    /// Receive the next block in the current batch and decode it using a
    /// caller-supplied function.
    ///
    /// This is useful when block decoding is not a simple `CborDecode`
    /// implementation on the target type, but still reports `LedgerError`.
    pub async fn recv_block_with<B, F>(
        &mut self,
        decode: F,
    ) -> Result<Option<B>, BlockFetchClientError>
    where
        F: FnOnce(&[u8]) -> Result<B, LedgerError>,
    {
        match self.recv_block().await? {
            Some(block) => Ok(Some(decode(&block)?)),
            None => Ok(None),
        }
    }

    /// Request a range and collect the full batch using a caller-supplied
    /// decoder.
    pub async fn request_range_collect_with<B, F>(
        &mut self,
        range: ChainRange,
        decode: F,
    ) -> Result<Vec<B>, BlockFetchClientError>
    where
        F: Fn(&[u8]) -> Result<B, LedgerError>,
    {
        let mut blocks = Vec::new();
        match self.request_range(range).await? {
            BatchResponse::NoBlocks => Ok(blocks),
            BatchResponse::StartedBatch => {
                while let Some(block) = self.recv_block_with(&decode).await? {
                    blocks.push(block);
                }
                Ok(blocks)
            }
        }
    }

    /// Request a typed point range and collect the full batch using a
    /// caller-supplied decoder.
    pub async fn request_range_collect_points_with<B, F>(
        &mut self,
        lower: Point,
        upper: Point,
        decode: F,
    ) -> Result<Vec<B>, BlockFetchClientError>
    where
        F: Fn(&[u8]) -> Result<B, LedgerError>,
    {
        self.request_range_collect_with(
            ChainRange {
                lower: lower.to_cbor_bytes(),
                upper: upper.to_cbor_bytes(),
            },
            decode,
        )
        .await
    }

    /// Receive the next block in the current batch, returning both the raw
    /// bytes and the decoded value.
    pub async fn recv_block_raw_decoded<B: CborDecode>(
        &mut self,
    ) -> Result<Option<(Vec<u8>, B)>, BlockFetchClientError> {
        self.recv_block_raw_with(B::from_cbor_bytes).await
    }

    /// Receive the next block in the current batch, returning both the raw
    /// bytes and the result of a caller-supplied decoder.
    pub async fn recv_block_raw_with<B, F>(
        &mut self,
        decode: F,
    ) -> Result<Option<(Vec<u8>, B)>, BlockFetchClientError>
    where
        F: FnOnce(&[u8]) -> Result<B, LedgerError>,
    {
        match self.recv_block().await? {
            Some(block) => {
                let decoded = decode(&block)?;
                Ok(Some((block, decoded)))
            }
            None => Ok(None),
        }
    }

    /// Request a range and collect the full batch as `(raw, decoded)` pairs.
    pub async fn request_range_collect_raw_with<B, F>(
        &mut self,
        range: ChainRange,
        decode: F,
    ) -> Result<Vec<(Vec<u8>, B)>, BlockFetchClientError>
    where
        F: Fn(&[u8]) -> Result<B, LedgerError>,
    {
        let mut blocks = Vec::new();
        match self.request_range(range).await? {
            BatchResponse::NoBlocks => Ok(blocks),
            BatchResponse::StartedBatch => {
                while let Some(block) = self.recv_block_raw_with(&decode).await? {
                    blocks.push(block);
                }
                Ok(blocks)
            }
        }
    }

    /// Request a typed point range and collect the full batch as `(raw,
    /// decoded)` pairs.
    pub async fn request_range_collect_points_raw_with<B, F>(
        &mut self,
        lower: Point,
        upper: Point,
        decode: F,
    ) -> Result<Vec<(Vec<u8>, B)>, BlockFetchClientError>
    where
        F: Fn(&[u8]) -> Result<B, LedgerError>,
    {
        self.request_range_collect_raw_with(
            ChainRange {
                lower: lower.to_cbor_bytes(),
                upper: upper.to_cbor_bytes(),
            },
            decode,
        )
        .await
    }

    /// Send `MsgClientDone` to terminate the protocol cleanly.
    ///
    /// The client must be in `StIdle`.  After this call the driver is in
    /// `StDone` and no further operations are possible.
    pub async fn done(mut self) -> Result<(), BlockFetchClientError> {
        self.send_msg(&BlockFetchMessage::MsgClientDone).await
    }
}
