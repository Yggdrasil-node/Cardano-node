//! Sync orchestration helpers for node-to-node ChainSync + BlockFetch.
//!
//! This module provides a thin runtime coordination layer between the
//! `ChainSyncClient` and `BlockFetchClient` drivers from `yggdrasil-network`.
//! It intentionally keeps ledger and consensus validation out of the node
//! crate and focuses only on protocol sequencing.

use yggdrasil_network::{
    BatchResponse, BlockFetchClient, BlockFetchClientError, ChainRange, ChainSyncClient,
    ChainSyncClientError, NextResponse,
};
use yggdrasil_ledger::{CborDecode, LedgerError, ShelleyBlock};

/// Error type for sync orchestration operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// ChainSync protocol error while requesting next chain update.
    #[error("chainsync error: {0}")]
    ChainSync(#[from] ChainSyncClientError),

    /// BlockFetch protocol error while fetching blocks for a roll-forward.
    #[error("blockfetch error: {0}")]
    BlockFetch(#[from] BlockFetchClientError),

    /// Ledger decode error while deserializing fetched block bytes.
    #[error("ledger decode error: {0}")]
    LedgerDecode(#[from] LedgerError),
}

/// Result of a single sync step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncStep {
    /// The peer rolled forward; any fetched blocks for the announced range are
    /// included in `blocks`.
    RollForward {
        /// Opaque header payload from ChainSync.
        header: Vec<u8>,
        /// Opaque tip/point payload from ChainSync.
        tip: Vec<u8>,
        /// Serialized blocks returned by BlockFetch for this step.
        blocks: Vec<Vec<u8>>,
    },

    /// The peer rolled backward to `point`.
    RollBackward {
        /// Opaque rollback target point from ChainSync.
        point: Vec<u8>,
        /// Opaque current tip from ChainSync.
        tip: Vec<u8>,
    },
}

/// Aggregate output from running multiple sync steps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncProgress {
    /// The latest known chain point after applying all steps.
    pub current_point: Vec<u8>,
    /// Ordered sequence of step outcomes.
    pub steps: Vec<SyncStep>,
    /// Total number of fetched blocks across all roll-forward steps.
    pub fetched_blocks: usize,
}

/// Result of a sync step where roll-forward blocks are decoded into
/// `ShelleyBlock` structures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedSyncStep {
    /// The peer rolled forward and all fetched blocks decoded successfully.
    RollForward {
        /// Opaque header payload from ChainSync.
        header: Vec<u8>,
        /// Opaque tip/point payload from ChainSync.
        tip: Vec<u8>,
        /// Decoded Shelley blocks fetched for this step.
        blocks: Vec<ShelleyBlock>,
    },

    /// The peer rolled backward to `point`.
    RollBackward {
        /// Opaque rollback target point from ChainSync.
        point: Vec<u8>,
        /// Opaque current tip from ChainSync.
        tip: Vec<u8>,
    },
}

/// Decode a list of raw BlockFetch payloads into Shelley blocks.
pub fn decode_shelley_blocks(raw_blocks: &[Vec<u8>]) -> Result<Vec<ShelleyBlock>, SyncError> {
    raw_blocks
        .iter()
        .map(|raw| ShelleyBlock::from_cbor_bytes(raw))
        .collect::<Result<Vec<_>, _>>()
        .map_err(SyncError::LedgerDecode)
}

async fn fetch_range_blocks(
    block_fetch: &mut BlockFetchClient,
    lower: Vec<u8>,
    upper: Vec<u8>,
) -> Result<Vec<Vec<u8>>, SyncError> {
    let mut blocks = Vec::new();
    let range = ChainRange { lower, upper };

    match block_fetch.request_range(range).await? {
        BatchResponse::NoBlocks => Ok(blocks),
        BatchResponse::StartedBatch => {
            while let Some(block) = block_fetch.recv_block().await? {
                blocks.push(block);
            }
            Ok(blocks)
        }
    }
}

/// Execute one sync step:
/// 1. Request the next ChainSync update.
/// 2. If roll-forward, request matching blocks via BlockFetch.
///
/// `from_point` is used as the lower bound for BlockFetch range requests.
pub async fn sync_step(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Vec<u8>,
) -> Result<SyncStep, SyncError> {
    let next = chain_sync.request_next().await?;

    match next {
        NextResponse::RollForward { header, tip }
        | NextResponse::AwaitRollForward { header, tip } => {
            let blocks = fetch_range_blocks(block_fetch, from_point, tip.clone()).await?;
            Ok(SyncStep::RollForward {
                header,
                tip,
                blocks,
            })
        }
        NextResponse::RollBackward { point, tip }
        | NextResponse::AwaitRollBackward { point, tip } => {
            Ok(SyncStep::RollBackward { point, tip })
        }
    }
}

/// Execute one sync step and decode any roll-forward block payloads into
/// typed `ShelleyBlock` values.
pub async fn sync_step_decoded(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Vec<u8>,
) -> Result<DecodedSyncStep, SyncError> {
    let step = sync_step(chain_sync, block_fetch, from_point).await?;
    match step {
        SyncStep::RollForward {
            header,
            tip,
            blocks,
        } => Ok(DecodedSyncStep::RollForward {
            header,
            tip,
            blocks: decode_shelley_blocks(&blocks)?,
        }),
        SyncStep::RollBackward { point, tip } => Ok(DecodedSyncStep::RollBackward { point, tip }),
    }
}

/// Execute `count` consecutive sync steps, carrying forward the latest chain
/// point between each step.
///
/// Point update rules:
/// - `RollForward`: `current_point = tip`
/// - `RollBackward`: `current_point = point`
pub async fn sync_steps(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Vec<u8>,
    count: usize,
) -> Result<SyncProgress, SyncError> {
    let mut steps = Vec::with_capacity(count);
    let mut fetched_blocks = 0usize;

    for _ in 0..count {
        let step = sync_step(chain_sync, block_fetch, from_point.clone()).await?;
        match &step {
            SyncStep::RollForward { tip, blocks, .. } => {
                from_point = tip.clone();
                fetched_blocks += blocks.len();
            }
            SyncStep::RollBackward { point, .. } => {
                from_point = point.clone();
            }
        }
        steps.push(step);
    }

    Ok(SyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
    })
}
