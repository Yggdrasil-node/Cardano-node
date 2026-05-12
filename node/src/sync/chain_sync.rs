//! Typed ChainSync sync-step API, intersection helpers, and volatile-store apply.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime adapter that
//! drives the typed `ChainSyncClient` from `yggdrasil_network`
//! through one or many `request_next` transitions, optionally
//! dispatching matching BlockFetch ranges via the typed
//! `BlockFetchClient`, and either returning raw / decoded / typed
//! step bundles or applying them directly into a `VolatileStore`.
//!
//! Mirrors the upstream
//! `Ouroboros.Consensus.MiniProtocol.ChainSync.Client` driver loop
//! plus `Ouroboros.Network.Protocol.ChainSync.Client` intersection
//! semantics. Upstream splits these concerns across mini-protocol
//! state machines, the consensus-side client wrapper, and the
//! storage `ChainDB` apply path; Yggdrasil collapses the driver into
//! these helper functions.
//!
//! Public functions moved from `node/src/sync.rs`:
//!
//! - `sync_step` — single raw step.
//! - `sync_step_decoded` — single step with decoded `ShelleyBlock` payloads.
//! - `sync_step_typed` — single step with typed `ShelleyHeader` + `Point`.
//! - `sync_steps` — N raw steps.
//! - `sync_steps_typed` — N typed steps.
//! - `sync_until_typed` — typed steps with stop-at predicate.
//! - `apply_typed_step_to_volatile` — single typed step → volatile store.
//! - `apply_typed_progress_to_volatile` — typed progress bundle → store.
//! - `typed_find_intersect` — typed `find_intersect` query.
//! - `sync_batch_apply` — `sync_until_typed` + `apply_typed_progress_to_volatile`.
//!
//! Plus the `TypedIntersectResult` enum.
//!
//! Extracted from `node/src/sync.rs` in R501 (sync.rs R-arc, 4th
//! slice). See `docs/operational-runs/2026-05-12-round-498-plan-sync-rs-split-arc.md`
//! for the multi-round plan.

use yggdrasil_ledger::{Point, ShelleyBlock, ShelleyHeader, SlotNo};
use yggdrasil_network::{
    BlockFetchClient, ChainSyncClient, DecodedHeaderNextResponse, NextResponse,
    TypedIntersectResponse,
};
use yggdrasil_storage::VolatileStore;

use super::block_fetch::{
    fetch_range_blocks, fetch_range_blocks_decoded, fetch_range_blocks_typed,
    normalize_blockfetch_range_bytes, normalize_blockfetch_range_points,
    point_bytes_from_raw_header_or_tip,
};
use super::shelley_decoders::shelley_block_to_block;
use super::{DecodedSyncStep, SyncError, SyncProgress, SyncStep, TypedSyncProgress, TypedSyncStep};

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
            let range_upper = point_bytes_from_raw_header_or_tip(&header, tip.clone());
            let blocks = if let Some((lower, upper)) =
                normalize_blockfetch_range_bytes(from_point, range_upper)
            {
                fetch_range_blocks(block_fetch, lower, upper).await?
            } else {
                Vec::new()
            };
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
    let next = chain_sync.request_next().await?;
    match next {
        NextResponse::RollForward { header, tip }
        | NextResponse::AwaitRollForward { header, tip } => Ok(DecodedSyncStep::RollForward {
            blocks: {
                let range_upper = point_bytes_from_raw_header_or_tip(&header, tip.clone());
                if let Some((lower, upper)) =
                    normalize_blockfetch_range_bytes(from_point, range_upper)
                {
                    fetch_range_blocks_decoded(block_fetch, lower, upper).await?
                } else {
                    Vec::new()
                }
            },
            header,
            tip: tip.clone(),
        }),
        NextResponse::RollBackward { point, tip }
        | NextResponse::AwaitRollBackward { point, tip } => {
            Ok(DecodedSyncStep::RollBackward { point, tip })
        }
    }
}

/// Execute one sync step and decode all ChainSync and BlockFetch payloads
/// into typed ledger values.
pub async fn sync_step_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    from_point: Point,
) -> Result<TypedSyncStep, SyncError> {
    let next = chain_sync
        .request_next_decoded_header::<ShelleyHeader>()
        .await?;

    match next {
        DecodedHeaderNextResponse::RollForward { header, tip }
        | DecodedHeaderNextResponse::AwaitRollForward { header, tip } => {
            let header_point = Point::BlockPoint(SlotNo(header.body.slot), header.header_hash());
            let pairs = if let Some((lower, upper)) =
                normalize_blockfetch_range_points(from_point, header_point)
            {
                fetch_range_blocks_typed(block_fetch, lower, upper).await?
            } else {
                Vec::new()
            };
            let (raw_blocks, blocks): (Vec<Vec<u8>>, Vec<ShelleyBlock>) = pairs.into_iter().unzip();
            Ok(TypedSyncStep::RollForward {
                header: Box::new(header),
                tip,
                blocks,
                raw_blocks,
            })
        }
        DecodedHeaderNextResponse::RollBackward { point, tip }
        | DecodedHeaderNextResponse::AwaitRollBackward { point, tip } => {
            Ok(TypedSyncStep::RollBackward { point, tip })
        }
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

/// Execute `count` consecutive typed sync steps, carrying forward the latest
/// typed chain point between each step.
pub async fn sync_steps_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Point,
    count: usize,
) -> Result<TypedSyncProgress, SyncError> {
    let mut steps = Vec::with_capacity(count);
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..count {
        let step = sync_step_typed(chain_sync, block_fetch, from_point).await?;

        match &step {
            TypedSyncStep::RollForward { tip, blocks, .. } => {
                from_point = *tip;
                fetched_blocks += blocks.len();
            }
            TypedSyncStep::RollBackward { point, .. } => {
                from_point = *point;
                rollback_count += 1;
            }
        }

        steps.push(step);
    }

    Ok(TypedSyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
        rollback_count,
    })
}

/// Run typed sync for up to `max_steps` transitions, optionally stopping early
/// once `stop_at` is reached as the current point.
pub async fn sync_until_typed(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    mut from_point: Point,
    max_steps: usize,
    stop_at: Option<Point>,
) -> Result<TypedSyncProgress, SyncError> {
    let mut steps = Vec::new();
    let mut fetched_blocks = 0usize;
    let mut rollback_count = 0usize;

    for _ in 0..max_steps {
        if stop_at.is_some_and(|target| target == from_point) {
            break;
        }

        let step = sync_step_typed(chain_sync, block_fetch, from_point).await?;

        match &step {
            TypedSyncStep::RollForward { tip, blocks, .. } => {
                from_point = *tip;
                fetched_blocks += blocks.len();
            }
            TypedSyncStep::RollBackward { point, .. } => {
                from_point = *point;
                rollback_count += 1;
            }
        }

        steps.push(step);

        if stop_at.is_some_and(|target| target == from_point) {
            break;
        }
    }

    Ok(TypedSyncProgress {
        current_point: from_point,
        steps,
        fetched_blocks,
        rollback_count,
    })
}

/// Apply one typed sync step into a volatile store.
///
/// Roll-forward blocks are converted and appended in order; roll-backward
/// steps trigger store rollback to the provided point.
pub fn apply_typed_step_to_volatile<S: VolatileStore>(
    store: &mut S,
    step: &TypedSyncStep,
) -> Result<(), SyncError> {
    match step {
        TypedSyncStep::RollForward {
            blocks, raw_blocks, ..
        } => {
            for (b, raw) in blocks.iter().zip(raw_blocks.iter()) {
                store.add_block(shelley_block_to_block(b, raw))?;
            }
        }
        TypedSyncStep::RollBackward { point, .. } => {
            store.rollback_to(point);
        }
    }
    Ok(())
}

/// Apply a typed sync progress bundle into a volatile store.
pub fn apply_typed_progress_to_volatile<S: VolatileStore>(
    store: &mut S,
    progress: &TypedSyncProgress,
) -> Result<(), SyncError> {
    for step in &progress.steps {
        apply_typed_step_to_volatile(store, step)?;
    }
    Ok(())
}

/// Result of a typed intersection query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedIntersectResult {
    /// The peer found a common intersection at `point`.
    Found {
        /// The intersection point agreed upon by the peer.
        point: Point,
        /// The peer's current tip.
        tip: Point,
    },
    /// The peer did not find any of the proposed points in its chain.
    NotFound {
        /// The peer's current tip.
        tip: Point,
    },
}

/// Find the intersection between our known chain and the peer's chain using
/// typed `Point` values.
///
/// Encodes the candidate points, calls `ChainSyncClient::find_intersect`, and
/// decodes the response into typed ledger values.
pub async fn typed_find_intersect(
    chain_sync: &mut ChainSyncClient,
    points: &[Point],
) -> Result<TypedIntersectResult, SyncError> {
    match chain_sync.find_intersect_points(points.to_vec()).await? {
        TypedIntersectResponse::Found { point, tip } => {
            Ok(TypedIntersectResult::Found { point, tip })
        }
        TypedIntersectResponse::NotFound { tip } => Ok(TypedIntersectResult::NotFound { tip }),
    }
}

/// Execute one batch of typed sync and apply the results to a volatile store.
///
/// Combines `sync_until_typed` with `apply_typed_progress_to_volatile` into
/// a single composable step. Returns the updated current point.
pub async fn sync_batch_apply<S: VolatileStore>(
    chain_sync: &mut ChainSyncClient,
    block_fetch: &mut BlockFetchClient,
    store: &mut S,
    from_point: Point,
    batch_size: usize,
    stop_at: Option<Point>,
) -> Result<TypedSyncProgress, SyncError> {
    let progress =
        sync_until_typed(chain_sync, block_fetch, from_point, batch_size, stop_at).await?;
    apply_typed_progress_to_volatile(store, &progress)?;
    Ok(progress)
}
