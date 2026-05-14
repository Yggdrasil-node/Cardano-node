//! Reconnecting verified-sync orchestrator state.
//!
//! Mirrors upstream `Ouroboros.Consensus.Node.Run.runWith` reconnect
//! loop state — the parts that aren't cleanly captured in the per-protocol
//! mini-protocol clients themselves:
//!
//! - `ReconnectingVerifiedSyncContext<'a>` — the immutable input bundle
//!   (peer addresses, sync config, optional cross-task handles).
//! - `ReconnectingVerifiedSyncState` — the mutable per-attempt state
//!   (current chain point, nonce-evolution state, checkpoint tracking).
//! - `ReconnectingRunState` — the run-level statistics + transient
//!   per-batch state (block totals, rollback counts, batches completed,
//!   tentative state, density observations, etc.).
//! - `RollbackReAdmissionStats` — Slice GD bookkeeping for txs
//!   re-admitted from the mempool after a rollback unwound their
//!   confirmation.
//! - `BatchTraceExtras` — per-batch trace bundle wired into
//!   `super::tracing::verified_sync_batch_trace_fields`.
//! - `BatchErrorDisposition` — three-way disposition for sync errors
//!   (Reconnect / Fail / Continue) emitted by `handle_reconnect_batch_error`.
//! - `record_verified_batch_progress(...)` — accumulator updater run
//!   after each successful batch application.
//!
//! Two impl blocks for `ReconnectingRunState` cover (1) construction
//! and per-batch updates and (2) reconnect-aware accumulators. A
//! `_runstate_impl_marker` module preserves the split-impl boundary
//! visually so future contributors don't accidentally insert items
//! between the two halves.
//!
//! Extracted from `runtime.rs` in R271j as the orchestrator-state
//! prelude for the upcoming async-fn extractions
//! (`run_reconnecting_verified_sync_service*` family + `governor_loop`
//! + `block_producer_loop`).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side reconnect-loop state
//! machine. Mirrors the runtime-state portion of upstream
//! `Ouroboros.Consensus.Node.Run.runWith` not captured by per-
//! protocol mini-protocol clients themselves. Haskell uses STM
//! TVars to express this state; Yggdrasil uses Rust async types.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};

use yggdrasil_consensus::mempool::{MempoolEntry, MempoolError, SharedMempool, SharedTxState};
use yggdrasil_consensus::{ChainState, NonceEvolutionConfig, NonceEvolutionState, TentativeState};
use yggdrasil_ledger::{Point, ShelleyTxIn, SlotNo, TxId};
use yggdrasil_network::{PeerRegistry, PeerSource, PeerStatus, UseLedgerPeers};

use yggdrasil_node_sync::{
    LedgerCheckpointTracking, MultiEraSyncProgress, MultiEraSyncStep, VerifiedSyncServiceConfig,
    apply_nonce_evolution_to_progress, extract_consumed_inputs, extract_tx_ids,
};
use yggdrasil_node_tracer::{NodeMetrics, NodeTracer};

use super::peer_session::{NodeConfig, ReconnectingSyncServiceOutcome};
use super::{ChainTipNotify, CheckpointTracking, SharedBlockProducerState};

pub(super) struct ReconnectingVerifiedSyncContext<'a> {
    pub(super) node_config: &'a NodeConfig,
    pub(super) fallback_peer_addrs: &'a [SocketAddr],
    pub(super) use_ledger_peers: Option<UseLedgerPeers>,
    pub(super) peer_snapshot_path: Option<&'a Path>,
    pub(super) config: &'a VerifiedSyncServiceConfig,
    pub(super) tracer: &'a NodeTracer,
    pub(super) metrics: Option<&'a NodeMetrics>,
    pub(super) peer_registry: Option<Arc<RwLock<PeerRegistry>>>,
    pub(super) mempool: Option<SharedMempool>,
    pub(super) tentative_state: Option<Arc<RwLock<TentativeState>>>,
    pub(super) tip_notify: Option<ChainTipNotify>,
    pub(super) bp_state: Option<Arc<RwLock<SharedBlockProducerState>>>,
    pub(super) bp_pool_key_hash: Option<[u8; 28]>,
    /// Optional shared TxSubmission inbound dedup state.  When present,
    /// the eviction pipeline notifies it of confirmed TxIds so peers that
    /// re-advertise on-chain transactions are immediately acked.
    pub(super) inbound_tx_state: Option<SharedTxState>,
}

pub(super) struct ReconnectingVerifiedSyncState {
    pub(super) from_point: Point,
    pub(super) nonce_state: Option<NonceEvolutionState>,
    pub(super) checkpoint_tracking: Option<CheckpointTracking>,
}

pub(super) struct ReconnectingRunState {
    pub(super) total_blocks: usize,
    pub(super) total_rollbacks: usize,
    pub(super) batches_completed: usize,
    pub(super) stable_block_count: usize,
    pub(super) reconnect_count: usize,
    pub(super) last_connected_peer_addr: Option<SocketAddr>,
    /// Consecutive failures without making progress (for exponential backoff).
    /// Reset to 0 whenever a batch completes successfully.
    pub(super) consecutive_failures: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RollbackReAdmissionStats {
    pub(super) re_admitted: usize,
    pub(super) duplicate: usize,
    pub(super) expired: usize,
    pub(super) conflicting: usize,
    pub(super) capacity_exceeded: usize,
    pub(super) protocol_rejected: usize,
    pub(super) missing_cache_entry: usize,
}

pub(super) fn cache_confirmed_entries(
    mempool: &SharedMempool,
    confirmed_ids: &[TxId],
    recently_confirmed: &mut BTreeMap<TxId, MempoolEntry>,
) -> usize {
    if confirmed_ids.is_empty() {
        return 0;
    }

    let snapshot = mempool.snapshot();
    let mut cached = 0usize;
    for tx_id in confirmed_ids {
        if recently_confirmed.contains_key(tx_id) {
            continue;
        }
        if let Some(entry) = snapshot.mempool_lookup_tx_by_id(tx_id) {
            recently_confirmed.insert(*tx_id, entry.clone());
            cached += 1;
        }
    }
    cached
}

pub(super) fn re_admit_rolled_back_tx_ids(
    mempool: &SharedMempool,
    rolled_back_tx_ids: &[TxId],
    current_slot: SlotNo,
    recently_confirmed: &mut BTreeMap<TxId, MempoolEntry>,
) -> RollbackReAdmissionStats {
    let mut stats = RollbackReAdmissionStats::default();
    for tx_id in rolled_back_tx_ids {
        let Some(entry) = recently_confirmed.remove(tx_id) else {
            stats.missing_cache_entry += 1;
            continue;
        };

        match mempool.insert_checked(entry, current_slot, None) {
            Ok(()) => stats.re_admitted += 1,
            Err(MempoolError::Duplicate(_)) => stats.duplicate += 1,
            Err(MempoolError::TtlExpired { .. }) => stats.expired += 1,
            Err(MempoolError::ConflictingInputs(_)) => stats.conflicting += 1,
            Err(MempoolError::CapacityExceeded { .. })
            | Err(MempoolError::EvictionInsufficientSpace { .. })
            | Err(MempoolError::EvictionNotWorthwhile { .. }) => {
                // Eviction-policy variants are reachable only via
                // `insert_with_eviction`. The rollback re-admission path
                // uses `insert_checked` so only `CapacityExceeded` is
                // produced today, but the wider arm keeps the match
                // exhaustive against future call-graph changes that
                // route re-admissions through the eviction-aware path.
                stats.capacity_exceeded += 1;
            }
            Err(MempoolError::FeeTooSmall { .. })
            | Err(MempoolError::TxTooLarge { .. })
            | Err(MempoolError::ExUnitsExceedTxLimit { .. })
            | Err(MempoolError::ProtocolParamValidation(_)) => stats.protocol_rejected += 1,
        }
    }
    stats
}

/// Evict confirmed, conflicting, expired, and ledger-invalid mempool
/// entries after a roll-forward batch.
///
/// This implements the upstream `syncWithLedger` / `revalidateTxsFor` flow:
/// after structural eviction (confirmed, double-spend, TTL), remaining
/// entries are fully re-applied against a scratch copy of the post-block
/// ledger state.  Entries that fail re-application are evicted.
///
/// When `inbound_tx_state` is provided, the confirmed TxIds are also
/// recorded in the cross-peer TxSubmission dedup state via
/// [`SharedTxState::mark_confirmed`] so inbound peers stop re-advertising
/// transactions that have just been included on-chain.  Mirrors upstream
/// `Ouroboros.Network.TxSubmission.Inbound.V2.State` `bufferedTxs`
/// population on block confirmation.
///
/// Returns a tuple of `(cached, confirmed, conflicting, expired, revalidated)`.
pub(super) fn evict_mempool_after_roll_forward(
    mempool: &SharedMempool,
    blocks: &[yggdrasil_node_sync::MultiEraBlock],
    block_spans: &[yggdrasil_ledger::BlockTxRawSpans],
    tip: &Point,
    recently_confirmed: &mut BTreeMap<TxId, MempoolEntry>,
    checkpoint_tracking: Option<&LedgerCheckpointTracking>,
    inbound_tx_state: Option<&SharedTxState>,
) -> (usize, usize, usize, usize, usize) {
    // Use the pre-extracted on-wire body byte spans cached on the
    // RollForward step so tx_id derivation matches what the wallet
    // originally submitted (see `extract_tx_ids` for the rationale).
    let confirmed_ids: Vec<TxId> = blocks
        .iter()
        .enumerate()
        .flat_map(|(i, b)| extract_tx_ids(b, block_spans.get(i)))
        .collect();
    if confirmed_ids.is_empty() {
        return (0, 0, 0, 0, 0);
    }
    let cached = cache_confirmed_entries(mempool, &confirmed_ids, recently_confirmed);
    let removed = mempool.remove_confirmed(&confirmed_ids);
    // Notify the cross-peer TxSubmission dedup state that these TxIds are
    // now on-chain so peers that re-advertise them are immediately acked
    // without re-fetching the bodies (upstream `bufferedTxs` semantics).
    if let Some(tx_state) = inbound_tx_state {
        tx_state.mark_confirmed(&confirmed_ids);
    }
    // Evict mempool txs whose inputs were consumed by
    // a *different* on-chain tx (double-spend conflict).
    // Reference: syncWithLedger / revalidateTxsFor.
    let consumed: Vec<ShelleyTxIn> = blocks.iter().flat_map(extract_consumed_inputs).collect();
    let conflicting = mempool.remove_conflicting_inputs(&consumed);
    let tip_slot = tip.slot().unwrap_or(SlotNo(0));
    let purged = mempool.purge_expired(tip_slot);
    // Full ledger re-validation: upstream `syncWithLedger` /
    // `revalidateTxsFor` re-applies every remaining tx
    // against the post-block ledger state.
    let revalidated = if let Some(tracking) = checkpoint_tracking {
        let mut scratch = tracking.ledger_state.clone();
        let eval = tracking.plutus_evaluator.clone();
        mempool.revalidate_with_ledger(|entry| match entry.to_multi_era_submitted_tx() {
            Ok(tx) => scratch
                .apply_submitted_tx(&tx, tip_slot, Some(&eval))
                .is_ok(),
            Err(_) => false,
        })
    } else {
        0
    };
    (cached, removed, conflicting, purged, revalidated)
}

impl ReconnectingRunState {
    pub(super) fn new() -> Self {
        Self {
            total_blocks: 0,
            total_rollbacks: 0,
            batches_completed: 0,
            stable_block_count: 0,
            reconnect_count: 0,
            last_connected_peer_addr: None,
            consecutive_failures: 0,
        }
    }

    pub(super) fn record_session(&mut self, peer_addr: SocketAddr, had_session: &mut bool) {
        if *had_session {
            self.reconnect_count += 1;
        } else {
            *had_session = true;
        }
        self.last_connected_peer_addr = Some(peer_addr);
    }
}

/// Register a freshly-bootstrapped peer in the shared `BlockFetchPool` so the
/// pool tracks per-peer state across reconnects.  Mirrors upstream
/// `addNewFetchClient` / `bracketFetchClient` in
/// `Ouroboros.Network.BlockFetch.ClientRegistry`: every active fetch client
/// must be registered with the registry while the session is live.
pub(super) fn pool_register_peer(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
) {
    if let Some(p) = pool {
        if let Ok(mut guard) = p.lock() {
            guard.register_peer(peer_addr);
        }
    }
}

/// Update this peer's known fragment head in the shared pool after a
/// successful sync batch advances `current_point`.  The pool's scheduling
/// policy uses this to gate range assignments — a peer can only receive a
/// range whose `upper` is at or behind its known fragment head.  Mirrors
/// upstream `setFetchClientFragment` in
/// `Ouroboros.Network.BlockFetch.ClientState`.
pub(super) fn pool_update_fragment_head(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
    head: Point,
) {
    if let Some(p) = pool {
        if let Ok(mut guard) = p.lock() {
            guard.set_peer_fragment_head(peer_addr, head);
        }
    }
}

/// Returns `true` when the pool has recorded enough consecutive failures
/// from `peer_addr` to warrant proactive demotion + mux teardown.  Mirrors
/// upstream `maxFetchClientFailures` policy in
/// `Ouroboros.Network.BlockFetch.ClientState`.
pub(super) fn pool_should_demote_peer(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
) -> bool {
    if let Some(p) = pool {
        if let Ok(guard) = p.lock() {
            if let Some(state) = guard.peer_state(peer_addr) {
                return state.consecutive_failures
                    >= yggdrasil_network::blockfetch_pool::DEFAULT_FAILURE_DEMOTION_THRESHOLD;
            }
        }
    }
    false
}

/// Remove `peer_addr` from the pool when its session ends.  Preserves
/// historical counters for inspection but frees the per-peer slot so the
/// next connection re-registers cleanly.  Mirrors upstream
/// `removeFetchClient` in `Ouroboros.Network.BlockFetch.ClientRegistry`.
pub(super) fn pool_unregister_peer(
    pool: Option<&yggdrasil_network::BlockFetchInstrumentation>,
    peer_addr: SocketAddr,
) {
    if let Some(p) = pool {
        if let Ok(mut guard) = p.lock() {
            let _ = guard.remove_peer(peer_addr);
        }
    }
}

/// Round 168 — register the bootstrap sync peer in the shared `PeerRegistry`
/// as a hot peer for the duration of the verified-sync session.
///
/// The bootstrap connection (`bootstrap_with_attempt_state`) is a direct
/// outbound that bypasses the governor's normal warm→hot promotion flow,
/// so without this hook the registry never reflects the active sync peer
/// — `PeerSelectionCounters::from_registry` then reports
/// `known/established/active = 0` while sync is fully running, which
/// shows up as misleading `yggdrasil_active_peers 0` in
/// `/metrics`.  Inserting `PeerSourceBootstrap` and setting status
/// `PeerHot` mirrors upstream behaviour where every active ChainSync
/// session-bearing peer appears in the registry as hot for the duration
/// of the session.
///
/// Reference: `Ouroboros.Network.PeerSelection.Governor` —
/// `KnownPeerInfo` carries `PeerStatus = PeerHot` for in-session peers.
pub(super) fn registry_mark_bootstrap_hot(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    peer_addr: SocketAddr,
) {
    if let Some(reg) = peer_registry {
        if let Ok(mut guard) = reg.write() {
            guard.insert_source(peer_addr, PeerSource::PeerSourceBootstrap);
            guard.set_status(peer_addr, PeerStatus::PeerHot);
        }
    }
}

/// Round 168 — companion teardown for [`registry_mark_bootstrap_hot`].
///
/// Demote the bootstrap peer from `PeerHot` to `PeerCooling` when its
/// session ends so the registry no longer reports it as active in
/// `/metrics`.  We keep the entry (with `PeerSourceBootstrap`) instead of
/// removing it so a subsequent reconnect attempt can resume from the same
/// status row — matching upstream's `cooldownPeerInfo` semantics for
/// post-session bookkeeping.
pub(super) fn registry_mark_bootstrap_cooling(
    peer_registry: Option<&Arc<RwLock<PeerRegistry>>>,
    peer_addr: SocketAddr,
) {
    if let Some(reg) = peer_registry {
        if let Ok(mut guard) = reg.write() {
            guard.set_status(peer_addr, PeerStatus::PeerCooling);
        }
    }
}

// === Second `impl ReconnectingRunState` block — progress tracking + ===
// === reconnect-cycle counters + sync-step trace surfaces.            ===
//
// Splitting the impl into two blocks (constructor / lifecycle, then
// progress tracking) keeps each block focused. The previous
// `_runstate_impl_marker` module served the same purpose; replaced
// with a comment line in R286 since a marker module has the same
// visual effect without carrying a `dead_code` allow.
impl ReconnectingRunState {
    pub(super) fn record_progress(&mut self, progress: &MultiEraSyncProgress) {
        self.total_blocks += progress.fetched_blocks;
        self.total_rollbacks += progress.rollback_count;
        self.batches_completed += 1;
        // A successful batch resets the failure counter.
        self.consecutive_failures = 0;
    }

    /// Called when the inner loop breaks due to an error (reconnect).
    pub(super) fn record_reconnect_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
    }

    /// Exponential backoff delay for reconnection attempts.
    ///
    /// The first reconnection attempt (consecutive_failures == 1) proceeds
    /// immediately so that peer rotation is not penalised.  From the second
    /// consecutive failure onward the delay doubles starting at 1 s, capped
    /// at 60 s.  Upstream reference: `peerBackerOff` exponential backoff in
    /// `Ouroboros.Network.PeerSelection.Governor`.
    pub(super) fn reconnect_backoff(&self) -> std::time::Duration {
        if self.consecutive_failures <= 1 {
            return std::time::Duration::ZERO;
        }
        let exp = (self.consecutive_failures - 1).min(6); // cap at 2^6 = 64s
        let secs = 1u64
            .checked_shl(exp.saturating_sub(1))
            .unwrap_or(64)
            .min(60);
        std::time::Duration::from_secs(secs)
    }

    pub(super) fn finish(
        self,
        final_point: Point,
        nonce_state: Option<NonceEvolutionState>,
        chain_state: Option<ChainState>,
    ) -> ReconnectingSyncServiceOutcome {
        ReconnectingSyncServiceOutcome {
            final_point,
            total_blocks: self.total_blocks,
            total_rollbacks: self.total_rollbacks,
            batches_completed: self.batches_completed,
            nonce_state,
            chain_state,
            stable_block_count: self.stable_block_count,
            reconnect_count: self.reconnect_count,
            last_connected_peer_addr: self.last_connected_peer_addr,
        }
    }
}

pub(super) struct BatchTraceExtras {
    pub(super) stable_block_count: Option<usize>,
    pub(super) checkpoint_tracked: Option<bool>,
}

#[derive(Debug)]
pub(super) enum BatchErrorDisposition {
    /// Reconnect to a different peer and retry.
    Reconnect,
    /// Reconnect and additionally record that the peer sent us invalid
    /// data.  Upstream this would trigger `InvalidBlockPunishment` /
    /// `PeerSentAnInvalidBlockException` and the governor would demote
    /// the peer.
    ///
    /// Reference: `Ouroboros.Consensus.Storage.ChainDB.API.Types.InvalidBlockPunishment`
    ReconnectAndPunish,
    /// Fatal local error — stop the sync service.
    Fail,
}

pub(super) fn record_verified_batch_progress(
    from_point: &mut Point,
    run_state: &mut ReconnectingRunState,
    progress: &MultiEraSyncProgress,
    nonce_state: Option<&mut NonceEvolutionState>,
    nonce_config: Option<&NonceEvolutionConfig>,
    metrics: Option<&NodeMetrics>,
) {
    *from_point = progress.current_point;
    run_state.record_progress(progress);

    if let Some((state, nonce_cfg)) = nonce_state.zip(nonce_config) {
        apply_nonce_evolution_to_progress(state, progress, nonce_cfg);
    }

    if let Some(m) = metrics {
        m.add_blocks_synced(progress.fetched_blocks as u64);
        m.add_rollbacks(progress.rollback_count as u64);
        m.inc_batches_completed();
        if let Point::BlockPoint(slot, _) = progress.current_point {
            m.set_current_slot(slot.0);
        }
        if let Some(block_no) = progress.latest_block_number() {
            m.set_current_block_number(block_no);
        }
        // Round 170 — accumulate per-era counters across this batch's
        // RollForward blocks for `/metrics`.  Tally locally so we make
        // one fetch_add per era rather than per block.
        let mut tally = [0u64; 7];
        for step in &progress.steps {
            if let MultiEraSyncStep::RollForward { blocks, .. } = step {
                for block in blocks {
                    let ord = block.era().era_ordinal() as usize;
                    if ord < tally.len() {
                        tally[ord] += 1;
                    }
                }
            }
        }
        for (ord, count) in tally.iter().enumerate() {
            if *count > 0 {
                m.add_blocks_for_era(ord as u8, *count);
            }
        }
    }
}
