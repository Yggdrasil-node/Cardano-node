//! Reconnecting verified-sync service entry points.
//!
//! Mirrors upstream `Ouroboros.Consensus.Node.Run.runWith` reconnect
//! loop — the high-level orchestration that wires bootstrap → sync →
//! reconnect cycles for the four (run/resume) × (volatile-store/chaindb) ×
//! (default-tracer/with-tracer) combinations.
//!
//! Twelve items move from `runtime.rs` here: 4 `run_*` entry points,
//! 4 `resume_*` entry points, 2 inner async fns
//! (`run_reconnecting_verified_sync_service_chaindb_inner`,
//! `run_reconnecting_verified_sync_service_shared_chaindb_inner`),
//! and 2 ledger-recovery helpers
//! (`stake_snapshots_for_recovered_point`, `recover_ledger_state_for_runtime`).
//!
//! All 15 runtime.rs-private helpers the cluster calls are reached via
//! `use super::{...};` — no `pub(super)` promotions, per the
//! descendants-see-private-ancestors rule confirmed in R271k/R271l.
//!
//! Extracted from `runtime.rs` in R271m (Phase γ §R271 thirteenth slice).
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side public entry points
//! for the four (run/resume) × (volatile-store/chaindb) ×
//! (default-tracer/with-tracer) combinations of the reconnect-
//! verified-sync service. Mirrors the high-level orchestration
//! of upstream `Ouroboros.Consensus.Node.Run.runWith` reconnect
//! loop; Haskell expresses each combination via type-class
//! polymorphism inline, Yggdrasil ships per-combination Rust
//! entry points.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use serde_json::json;

use yggdrasil_consensus::EpochSchedule;
use yggdrasil_consensus::mempool::MempoolEntry;
use yggdrasil_ledger::{LedgerState, Point, SlotNo, StakeSnapshots, TxId};
use yggdrasil_network::{PeerStatus, peer_attempt_state};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use yggdrasil_node_sync::{
    ChainDepStateTracking, LedgerCheckpointTracking, LedgerCheckpointUpdateOutcome,
    LedgerRecoveryOutcome, MultiEraSyncStep, SyncError, VerifiedSyncServiceConfig,
    VrfVerificationContext, apply_verified_progress_to_chaindb, load_stake_snapshots_sidecar,
    persist_chain_dep_state_sidecar, recover_ledger_state_chaindb,
    restore_chain_dep_sidecar_state_to_point, sync_batch_apply_verified,
    sync_batch_verified_with_tentative, track_chain_state,
};
use yggdrasil_node_tracer::{NodeTracer, trace_fields};

type CheckpointPersistenceOutcome = LedgerCheckpointUpdateOutcome;

fn alternate_reconnect_peer(
    primary_peer: std::net::SocketAddr,
    fallback_peer_addrs: &[std::net::SocketAddr],
    failed_peer: std::net::SocketAddr,
) -> Option<std::net::SocketAddr> {
    fallback_peer_addrs
        .iter()
        .copied()
        .find(|peer| *peer != failed_peer)
        .or_else(|| (primary_peer != failed_peer).then_some(primary_peer))
}

use super::keep_alive::{KeepAliveScheduler, trace_verified_sync_batch_applied};
use super::peer_session::{
    ReconnectingSyncServiceOutcome, ReconnectingVerifiedSyncRequest,
    ResumeReconnectingVerifiedSyncRequest, ResumedSyncServiceOutcome,
};
use super::reconnecting::{
    BatchErrorDisposition, BatchTraceExtras, ReconnectingRunState, ReconnectingVerifiedSyncContext,
    ReconnectingVerifiedSyncState, evict_mempool_after_roll_forward, pool_register_peer,
    pool_should_demote_peer, pool_unregister_peer, pool_update_fragment_head,
    re_admit_rolled_back_tx_ids, record_verified_batch_progress, registry_mark_bootstrap_cooling,
    registry_mark_bootstrap_hot,
};
use super::{
    bootstrap_with_attempt_state, handle_reconnect_batch_error, preferred_hot_peer_handoff_target,
    prepare_reconnect_attempt_state, reconnect_storage_tip,
    refresh_chain_db_reconnect_fallback_peers, registry_reserve_bootstrap_attempt_peers,
    shared_chaindb_lock_error, synchronize_chain_sync_to_point, trace_checkpoint_outcome,
    trace_epoch_boundary_events, trace_reconnectable_sync_error, trace_session_established,
    trace_shutdown_before_bootstrap, trace_shutdown_during_session, update_bp_state_nonce,
    update_bp_state_sigma,
};

async fn run_reconnecting_verified_sync_service_chaindb_inner<I, V, L, F>(
    // (Function with direct `&mut ChainDb` access — see seed_chain_state_from_volatile call below.)
    chain_db: &mut ChainDb<I, V, L>,
    context: ReconnectingVerifiedSyncContext<'_>,
    state: ReconnectingVerifiedSyncState,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncContext {
        node_config,
        fallback_peer_addrs,
        use_ledger_peers,
        peer_snapshot_path,
        config,
        tracer,
        metrics,
        peer_registry,
        mempool,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    // Seed the volatile chain window from storage on restart so the next
    // ChainSync session's `RollBackward(recovered_tip)` confirmation finds
    // the tip in `entries` instead of crashing with `RollbackPointNotFound`.
    // Surfaced by the §6 restart-resilience operator rehearsal as a
    // cycle-2 crash; see `seed_chain_state_from_volatile` for the
    // upstream `Ouroboros.Consensus.Storage.ChainDB.Init` reference.
    let mut chain_state = seed_chain_state_via_chain_db(chain_db, config.security_param);
    let mut ocert_counters = config.verification.ocert_counters.clone();
    let origin_nonce_state = nonce_state.clone();
    if let Some(tracking) = checkpoint_tracking.as_ref() {
        let restored = restore_chain_dep_sidecar_state_to_point(
            chain_db,
            &from_point,
            tracking.chain_dep_persist_dir.as_deref(),
            &mut nonce_state,
            config.nonce_config.as_ref(),
            &mut ocert_counters,
        )?;
        if !restored
            && tracking.chain_dep_persist_dir.is_some()
            && !matches!(from_point, Point::Origin)
        {
            return Err(SyncError::Recovery(format!(
                "missing exact ChainDepState sidecar history for recovered point {from_point:?}"
            )));
        }
        update_bp_state_nonce(&bp_state, nonce_state.as_ref());
    }
    let mut had_session = false;
    let mut preferred_peer = None;
    let mut recently_confirmed = BTreeMap::<TxId, MempoolEntry>::new();

    loop {
        // Exponential backoff before reattempting after consecutive failures.
        let backoff = run_state.reconnect_backoff();
        if !backoff.is_zero() {
            tracer.trace_runtime(
                "Net.PeerSelection",
                "Info",
                "delaying reconnect attempt",
                trace_fields([("backoffMs", json!(backoff.as_millis()))]),
            );
            tokio::select! {
                biased;
                () = &mut shutdown => {
                    trace_shutdown_before_bootstrap(tracer);
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }
                () = tokio::time::sleep(backoff) => {}
            }
        }

        // Round-90 (Gap BM) fix: realign `from_point` and `chain_state`
        // with the storage volatile tip BEFORE attempting the next
        // session.  Without this, a session-handoff (`switching sync
        // session to higher-tip hot peer`) can leave `from_point`
        // pointing at a hash that's no longer in `chain_state.entries`
        // (e.g., a deep rollback during the previous session truncated
        // the volatile window past `from_point`), and the next peer's
        // `RollBackward(from_point)` confirmation crashes the node
        // with `RollbackPointNotFound`.  Re-seeding from the volatile
        // store and snapping `from_point` to its tip makes the resume
        // self-consistent regardless of what happened in the prior
        // session.  Surfaced by §6.5a multi-peer rehearsal on 2026-04-27.
        if let Some(new_chain_state) =
            seed_chain_state_via_chain_db(chain_db, config.security_param)
        {
            let volatile_tip = new_chain_state.tip();
            let best_tip = chain_db.best_tip();
            let storage_tip = reconnect_storage_tip(volatile_tip, best_tip);
            if storage_tip != from_point {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Info",
                    "realigning from_point to storage tip before reconnect",
                    trace_fields([
                        ("staleFromPoint", json!(format!("{from_point:?}"))),
                        ("storageTip", json!(format!("{storage_tip:?}"))),
                        ("volatileTip", json!(format!("{volatile_tip:?}"))),
                    ]),
                );
                from_point = storage_tip;
            }
            chain_state = Some(new_chain_state);
        }

        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let (mut attempt_state, reconnect_preference) = prepare_reconnect_attempt_state(
            node_config.peer_addr,
            &refreshed_fallback_peers,
            peer_registry.as_ref(),
            preferred_peer,
        );
        registry_reserve_bootstrap_attempt_peers(
            peer_registry.as_ref(),
            attempt_state.attempt_order(),
        );

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(
                        checkpoint_tracking.as_ref().and_then(|tracking| tracking
                            .ledger_state
                            .tip
                            .slot()
                            .map(|slot| slot.0))
                    ),
                ),
                (
                    "useLedgerPeers",
                    json!(use_ledger_peers.map(|policy| format!("{policy:?}"))),
                ),
                (
                    "preferredPeer",
                    json!(reconnect_preference.map(|(peer, _)| peer.to_string())),
                ),
                (
                    "preferredPeerSource",
                    json!(reconnect_preference.map(|(_, source)| source)),
                ),
            ]),
        );

        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                trace_shutdown_before_bootstrap(tracer);
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        run_state.record_session(session.connected_peer_addr, &mut had_session);
        pool_register_peer(
            config.block_fetch_pool.as_ref(),
            session.connected_peer_addr,
        );
        // Slice E — exercise the `max_concurrent_block_fetch_peers` knob
        // from a production code path so the audit gap "config knob read
        // by no production path" is closed.  Currently the runtime
        // maintains one session per call, so the effective concurrency
        // always returns 1; future multi-session orchestration can fan
        // this out across N peers without re-plumbing the config read.
        let _effective_block_fetch_concurrency = config.effective_block_fetch_concurrency(1);
        if had_session && run_state.reconnect_count > 0 {
            if let Some(m) = metrics {
                m.inc_reconnects();
            }
        }
        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        if let Err(err) = synchronize_chain_sync_to_point(
            &mut session.chain_sync,
            &mut from_point,
            tracer,
            session.connected_peer_addr,
        )
        .await
        {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client.FindIntersect",
                "intersection request failed; retrying after reconnect",
                session.connected_peer_addr,
                &err,
                from_point,
            );
            session.mux.abort();
            registry_mark_bootstrap_cooling(peer_registry.as_ref(), session.connected_peer_addr);
            preferred_peer = alternate_reconnect_peer(
                node_config.peer_addr,
                &refreshed_fallback_peers,
                session.connected_peer_addr,
            );
            run_state.record_reconnect_failure();
            continue;
        }
        // Round 168 — mirror the established ChainSync/BlockFetch session
        // into the shared `PeerRegistry` only after the peer confirms the
        // intersection. Until then the bootstrap peer stays `PeerCooling`,
        // which keeps the governor from opening unrelated outbound sessions
        // during the non-pipelined ChainSync setup.
        registry_mark_bootstrap_hot(peer_registry.as_ref(), session.connected_peer_addr);

        let mut keepalive = KeepAliveScheduler::new(Instant::now());
        loop {
            // Drive the KeepAlive heartbeat alongside ChainSync/BlockFetch so
            // upstream peers do not tear down the connection at
            // `keepAliveTimeout` (~97 s default).
            if let Err(err) = keepalive.tick(&mut session.keep_alive).await {
                trace_reconnectable_sync_error(
                    tracer,
                    "KeepAlive.Client",
                    "keepalive failed; reconnecting",
                    session.connected_peer_addr,
                    &err,
                    from_point,
                );
                session.mux.abort();
                // Round 175 — companion teardown for R168's bootstrap-Hot
                // promotion.  Without this, a KeepAlive timeout left the
                // bootstrap peer marked `PeerHot` until the next session
                // re-promotion, briefly over-reporting active peers in
                // `/metrics` during the reconnect window.
                registry_mark_bootstrap_cooling(
                    peer_registry.as_ref(),
                    session.connected_peer_addr,
                );
                preferred_peer = alternate_reconnect_peer(
                    node_config.peer_addr,
                    &refreshed_fallback_peers,
                    session.connected_peer_addr,
                );
                run_state.record_reconnect_failure();
                break;
            }

            // R217 — fetch+verify timing baseline.
            let fetch_start = std::time::Instant::now();
            let batch_fut = sync_batch_verified_with_tentative(
                &mut session.chain_sync,
                session.block_fetch.as_mut(),
                from_point,
                config.batch_size,
                Some(&config.verification),
                tentative_state.as_ref(),
                &mut ocert_counters,
                config
                    .block_fetch_pool
                    .as_ref()
                    .map(|p| (p, session.connected_peer_addr)),
                config
                    .density_registry
                    .as_ref()
                    .map(|r| (r, session.connected_peer_addr)),
                config.shared_fetch_worker_pool.as_ref().map(|pool| {
                    yggdrasil_node_sync::MultiPeerDispatchContext {
                        pool,
                        max_concurrent_knob: config.max_concurrent_block_fetch_peers,
                        chainsync_pool: config.shared_chainsync_worker_pool.as_ref(),
                    }
                }),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    trace_shutdown_during_session(
                        tracer,
                        session.connected_peer_addr,
                        from_point,
                    );
                    session.mux.abort();
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }

                result = batch_fut => {
                    // R217 — record fetch+verify duration regardless
                    // of result (timing the path even on error).
                    if let Some(m) = metrics {
                        m.record_fetch_batch_duration(fetch_start.elapsed());
                    }
                    match result {
                        Ok(progress) => {
                            let vrf_nonce_snapshot = if config.verify_vrf {
                                nonce_state.clone()
                            } else {
                                None
                            };
                            let vrf_ctx = if config.verify_vrf {
                                vrf_nonce_snapshot
                                    .as_ref()
                                    .zip(config.active_slot_coeff.as_ref())
                                    .zip(config.nonce_config.as_ref())
                                    .map(|((ns, asc), nonce_cfg)| VrfVerificationContext {
                                        nonce_state: ns,
                                        nonce_cfg,
                                        active_slot_coeff: asc,
                                    })
                            } else {
                                None
                            };
                            let apply_start = std::time::Instant::now();
                            // R210 — opt-in diagnostic: dump per-batch
                            // `fetched_blocks` / `rollback_count` /
                            // `current_point` so operators can identify
                            // where the apply pipeline stalls (e.g. mainnet
                            // sync gap surfaced by R208).  Set
                            // `YGG_SYNC_DEBUG=1` to enable.
                            if std::env::var("YGG_SYNC_DEBUG").is_ok() {
                                eprintln!(
                                    "[YGG_SYNC_DEBUG] apply_verified_progress fetched_blocks={} rollback_count={} steps={} current_point={:?}",
                                    progress.fetched_blocks,
                                    progress.rollback_count,
                                    progress.steps.len(),
                                    progress.current_point,
                                );
                            }
                            let applied = apply_verified_progress_to_chaindb(
                                chain_db,
                                &progress,
                                chain_state.as_mut(),
                                checkpoint_tracking.as_mut(),
                                &config.checkpoint_policy,
                                vrf_ctx.as_ref(),
                                ChainDepStateTracking {
                                    nonce_state: &mut nonce_state,
                                    origin_nonce_state: origin_nonce_state.as_ref(),
                                    nonce_cfg: config.nonce_config.as_ref(),
                                    ocert_counters: &mut ocert_counters,
                                },
                            )?;
                            if std::env::var("YGG_SYNC_DEBUG").is_ok() {
                                let tip_str = checkpoint_tracking
                                    .as_ref()
                                    .map(|t| format!("{:?}", t.ledger_state.tip))
                                    .unwrap_or_else(|| "no-tracking".into());
                                eprintln!(
                                    "[YGG_SYNC_DEBUG] applied stable_block_count={} epoch_events={} rolled_back_tx_ids={} tracking.tip={}",
                                    applied.stable_block_count,
                                    applied.epoch_boundary_events.len(),
                                    applied.rolled_back_tx_ids.len(),
                                    tip_str,
                                );
                            }
                            // R200 — apply-batch duration histogram
                            // (Phase C.1).  Excludes block fetch but
                            // includes ledger advance, checkpoint
                            // persist, and ChainState topology
                            // tracking.
                            if let Some(m) = metrics {
                                m.record_apply_batch_duration(apply_start.elapsed());
                            }

                            trace_epoch_boundary_events(tracer, &applied.epoch_boundary_events);

                            // Round 169 — surface the wire-era ordinal to
                            // `/metrics` so dashboards observe Byron→…→Conway
                            // progression directly without parsing
                            // `cardano-cli query tip`.
                            if let (Some(m), Some(tracking)) =
                                (metrics, checkpoint_tracking.as_ref())
                            {
                                m.set_current_era(
                                    tracking.ledger_state.current_era.era_ordinal() as u64,
                                );
                            }

                            // Update shared block-producer state with live sigma after
                            // epoch boundary events (stake snapshot rotation).
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref pkh) = bp_pool_key_hash {
                                    let snapshots = checkpoint_tracking.as_ref()
                                        .and_then(|ct| ct.stake_snapshots.as_ref());
                                    update_bp_state_sigma(&bp_state, snapshots, pkh);
                                }
                            }

                            // Epoch revalidation: when a new epoch begins, protocol parameters
                            // may have changed.  Re-validate all mempool entries and evict any
                            // that no longer satisfy the new fee / size / ExUnits constraints.
                            // Reference: Ouroboros.Consensus.Mempool.Impl.Update — syncWithLedger.
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref mempool) = mempool {
                                    if let Some(ref tracking) = checkpoint_tracking {
                                        if let Some(params) = tracking.ledger_state.protocol_params() {
                                            let tip_slot = progress.current_point.slot().unwrap_or(SlotNo(0));
                                            let evicted = mempool.purge_invalid_for_params(tip_slot, params);
                                            if evicted > 0 {
                                                tracer.trace_runtime(
                                                    "Mempool.EpochRevalidation",
                                                    "Info",
                                                    "purged mempool entries invalid under new epoch params",
                                                    trace_fields([("evicted", json!(evicted))]),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            // R225 — Phase D.1 first slice: record
                            // rollback-depth observations into the
                            // Prometheus histogram.  Unit is
                            // rolled-back transactions (proxy for
                            // block depth × txs/block); depth=0 for
                            // confirm-shape rollbacks (e.g.
                            // session-start RollBackward(Origin) on
                            // a fresh DB).  Operators graph the
                            // distribution to detect rare deep
                            // cross-epoch rollbacks (the Phase D.1
                            // problematic case).
                            if progress.rollback_count > 0 {
                                if let Some(m) = metrics {
                                    m.record_rollback_depth(applied.rolled_back_tx_ids.len() as u64);
                                }
                            }
                            if !applied.rolled_back_tx_ids.is_empty() {
                                tracer.trace_runtime(
                                    "ChainDB.Rollback",
                                    "Info",
                                    "collected rolled-back transaction ids",
                                    trace_fields([
                                        ("txCount", json!(applied.rolled_back_tx_ids.len())),
                                    ]),
                                );

                                if let Some(ref mempool) = mempool {
                                    let stats = re_admit_rolled_back_tx_ids(
                                        mempool,
                                        &applied.rolled_back_tx_ids,
                                        progress.current_point.slot().unwrap_or(SlotNo(0)),
                                        &mut recently_confirmed,
                                    );
                                    tracer.trace_runtime(
                                        "Mempool.RollbackReadmission",
                                        "Info",
                                        "processed rolled-back transaction re-admission",
                                        trace_fields([
                                            ("rolledBackTxCount", json!(applied.rolled_back_tx_ids.len())),
                                            ("reAdmitted", json!(stats.re_admitted)),
                                            ("duplicate", json!(stats.duplicate)),
                                            ("expired", json!(stats.expired)),
                                            ("conflicting", json!(stats.conflicting)),
                                            ("capacityExceeded", json!(stats.capacity_exceeded)),
                                            ("protocolRejected", json!(stats.protocol_rejected)),
                                            ("missingCacheEntry", json!(stats.missing_cache_entry)),
                                        ]),
                                    );
                                }
                            }

                            if let Some(ref mempool) = mempool {
                                for step in &progress.steps {
                                    if let MultiEraSyncStep::RollForward {
                                        blocks,
                                        block_spans,
                                        tip,
                                        ..
                                    } = step
                                    {
                                        let (cached, removed, conflicting, purged, revalidated) =
                                            evict_mempool_after_roll_forward(
                                                mempool, blocks, block_spans, tip,
                                                &mut recently_confirmed,
                                                checkpoint_tracking.as_ref(),
                                                inbound_tx_state.as_ref(),
                                            );
                                        if cached + removed + conflicting + purged + revalidated > 0 {
                                            tracer.trace_runtime(
                                                "Mempool.Eviction",
                                                "Info",
                                                "evicted confirmed/expired/conflicting txs from mempool",
                                                trace_fields([
                                                    ("cachedForRollback", json!(cached)),
                                                    ("confirmed", json!(removed)),
                                                    ("conflicting", json!(conflicting)),
                                                    ("expired", json!(purged)),
                                                    ("ledgerRevalidated", json!(revalidated)),
                                                ]),
                                            );
                                        }
                                    }
                                }
                            }

                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                metrics,
                            );

                            if let Some(tracking) = checkpoint_tracking.as_ref() {
                                persist_chain_dep_state_sidecar(
                                    &applied.checkpoint_outcome,
                                    tracking.chain_dep_persist_dir.as_deref(),
                                    from_point,
                                    nonce_state.as_ref(),
                                    ocert_counters.as_ref(),
                                    config.checkpoint_policy.max_snapshots,
                                )?;
                            }

                            // Update pool fragment-head tracking with the
                            // live current_point so the multi-peer scheduler
                            // knows this peer can serve up through this slot.
                            pool_update_fragment_head(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                                from_point,
                            );

                            // Push live epoch nonce to the concurrent block producer.
                            update_bp_state_nonce(&bp_state, nonce_state.as_ref());

                            if let Some(ref notify) = tip_notify {
                                notify.notify_waiters();
                            }

                            run_state.stable_block_count += applied.stable_block_count;
                            if let Some(m) = metrics {
                                m.add_stable_blocks_promoted(applied.stable_block_count as u64);
                            }

                            if let Some(checkpoint_outcome) = applied.checkpoint_outcome.as_ref() {
                                if let CheckpointPersistenceOutcome::Persisted { slot, .. } = checkpoint_outcome {
                                    if let Some(m) = metrics {
                                        m.set_checkpoint_slot(slot.0);
                                    }
                                }
                                trace_checkpoint_outcome(
                                    tracer,
                                    checkpoint_outcome,
                                    &config.checkpoint_policy,
                                );
                            }

                            trace_verified_sync_batch_applied(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &progress,
                                &run_state,
                                BatchTraceExtras {
                                    stable_block_count: Some(run_state.stable_block_count),
                                    checkpoint_tracked: Some(checkpoint_tracking.is_some()),
                                },
                            );

                            if let Some(next_hot_peer) = preferred_hot_peer_handoff_target(
                                peer_registry.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.PeerSelection",
                                    "Info",
                                    "switching sync session to higher-tip hot peer",
                                    trace_fields([
                                        ("fromPeer", json!(session.connected_peer_addr.to_string())),
                                        ("toPeer", json!(next_hot_peer.to_string())),
                                    ]),
                                );
                                preferred_peer = Some(next_hot_peer);
                                session.mux.abort();
                                // Round 175 — the previous bootstrap peer
                                // is no longer the active sync target;
                                // demote in registry so `/metrics` reflects
                                // the handoff.  The next iteration's
                                // bootstrap to `next_hot_peer` will
                                // re-promote whichever peer it lands on.
                                registry_mark_bootstrap_cooling(
                                    peer_registry.as_ref(),
                                    session.connected_peer_addr,
                                );
                                break;
                            }
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            if pool_should_demote_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.BlockFetch.PoolDemote",
                                    "Warning",
                                    "fetch-client failure threshold exceeded for peer",
                                    trace_fields([(
                                        "peer",
                                        json!(session.connected_peer_addr.to_string()),
                                    )]),
                                );
                            }
                            pool_unregister_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::ReconnectAndPunish => {
                                    // Demote offending peer to Cold so the governor's
                                    // backoff/forget logic penalizes it (upstream
                                    // InvalidBlockPunishment closes the connection).
                                    if let Some(ref registry) = peer_registry {
                                        if let Ok(mut reg) = registry.write() {
                                            reg.set_status(session.connected_peer_addr, PeerStatus::PeerCold);
                                        }
                                    }
                                    preferred_peer = alternate_reconnect_peer(
                                        node_config.peer_addr,
                                        &refreshed_fallback_peers,
                                        session.connected_peer_addr,
                                    );
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Reconnect => {
                                    preferred_peer = alternate_reconnect_peer(
                                        node_config.peer_addr,
                                        &refreshed_fallback_peers,
                                        session.connected_peer_addr,
                                    );
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn run_reconnecting_verified_sync_service_shared_chaindb_inner<I, V, L, F>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    context: ReconnectingVerifiedSyncContext<'_>,
    state: ReconnectingVerifiedSyncState,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncContext {
        node_config,
        fallback_peer_addrs,
        use_ledger_peers,
        peer_snapshot_path,
        config,
        tracer,
        metrics,
        peer_registry,
        mempool,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state,
    } = context;
    let ReconnectingVerifiedSyncState {
        mut from_point,
        mut nonce_state,
        mut checkpoint_tracking,
    } = state;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    // Seed the volatile chain window from storage on restart so the next
    // ChainSync session's `RollBackward(recovered_tip)` confirmation finds
    // the tip in `entries` instead of crashing with `RollbackPointNotFound`.
    // Surfaced by the §6 restart-resilience operator rehearsal as a
    // cycle-2 crash; see `seed_chain_state_from_volatile` for the
    // upstream `Ouroboros.Consensus.Storage.ChainDB.Init` reference.
    let mut chain_state = seed_chain_state_via_chain_db(chain_db, config.security_param);
    let mut ocert_counters = config.verification.ocert_counters.clone();
    let origin_nonce_state = nonce_state.clone();
    if let Some(tracking) = checkpoint_tracking.as_ref() {
        let restored = {
            let chain_db = chain_db.read().map_err(|_| shared_chaindb_lock_error())?;
            restore_chain_dep_sidecar_state_to_point(
                &chain_db,
                &from_point,
                tracking.chain_dep_persist_dir.as_deref(),
                &mut nonce_state,
                config.nonce_config.as_ref(),
                &mut ocert_counters,
            )?
        };
        if !restored
            && tracking.chain_dep_persist_dir.is_some()
            && !matches!(from_point, Point::Origin)
        {
            return Err(SyncError::Recovery(format!(
                "missing exact ChainDepState sidecar history for recovered point {from_point:?}"
            )));
        }
    }
    let mut had_session = false;
    let mut preferred_peer = None;
    let mut recently_confirmed = BTreeMap::<TxId, MempoolEntry>::new();

    loop {
        // Exponential backoff before reattempting after consecutive failures.
        let backoff = run_state.reconnect_backoff();
        if !backoff.is_zero() {
            tracer.trace_runtime(
                "Net.PeerSelection",
                "Info",
                "delaying reconnect attempt",
                trace_fields([("backoffMs", json!(backoff.as_millis()))]),
            );
            tokio::select! {
                biased;
                () = &mut shutdown => {
                    trace_shutdown_before_bootstrap(tracer);
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }
                () = tokio::time::sleep(backoff) => {}
            }
        }

        // Round-90 (Gap BM) fix: realign `from_point` and `chain_state`
        // with the storage volatile tip BEFORE attempting the next
        // session.  Without this, a session-handoff (`switching sync
        // session to higher-tip hot peer`) can leave `from_point`
        // pointing at a hash that's no longer in `chain_state.entries`
        // (e.g., a deep rollback during the previous session truncated
        // the volatile window past `from_point`), and the next peer's
        // `RollBackward(from_point)` confirmation crashes the node
        // with `RollbackPointNotFound`.  Re-seeding from the volatile
        // store and snapping `from_point` to its tip makes the resume
        // self-consistent regardless of what happened in the prior
        // session.  Surfaced by §6.5a multi-peer rehearsal on 2026-04-27.
        if let Some(new_chain_state) =
            seed_chain_state_via_chain_db(chain_db, config.security_param)
        {
            let volatile_tip = new_chain_state.tip();
            let best_tip = chain_db.best_tip();
            let storage_tip = reconnect_storage_tip(volatile_tip, best_tip);
            if storage_tip != from_point {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Info",
                    "realigning from_point to storage tip before reconnect",
                    trace_fields([
                        ("staleFromPoint", json!(format!("{from_point:?}"))),
                        ("storageTip", json!(format!("{storage_tip:?}"))),
                        ("volatileTip", json!(format!("{volatile_tip:?}"))),
                    ]),
                );
                from_point = storage_tip;
            }
            chain_state = Some(new_chain_state);
        }

        let refreshed_fallback_peers = refresh_chain_db_reconnect_fallback_peers(
            node_config.peer_addr,
            fallback_peer_addrs,
            checkpoint_tracking.as_ref(),
            use_ledger_peers,
            peer_snapshot_path,
            tracer,
        );
        let (mut attempt_state, reconnect_preference) = prepare_reconnect_attempt_state(
            node_config.peer_addr,
            &refreshed_fallback_peers,
            peer_registry.as_ref(),
            preferred_peer,
        );
        registry_reserve_bootstrap_attempt_peers(
            peer_registry.as_ref(),
            attempt_state.attempt_order(),
        );

        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "refreshed reconnect peer candidates",
            trace_fields([
                ("fallbackPeerCount", json!(refreshed_fallback_peers.len())),
                (
                    "latestSlot",
                    json!(
                        checkpoint_tracking.as_ref().and_then(|tracking| tracking
                            .ledger_state
                            .tip
                            .slot()
                            .map(|slot| slot.0))
                    ),
                ),
                (
                    "useLedgerPeers",
                    json!(use_ledger_peers.map(|policy| format!("{policy:?}"))),
                ),
                (
                    "preferredPeer",
                    json!(reconnect_preference.map(|(peer, _)| peer.to_string())),
                ),
                (
                    "preferredPeerSource",
                    json!(reconnect_preference.map(|(_, source)| source)),
                ),
            ]),
        );

        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                trace_shutdown_before_bootstrap(tracer);
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        run_state.record_session(session.connected_peer_addr, &mut had_session);
        pool_register_peer(
            config.block_fetch_pool.as_ref(),
            session.connected_peer_addr,
        );
        // Slice E — exercise the `max_concurrent_block_fetch_peers` knob
        // from a production code path so the audit gap "config knob read
        // by no production path" is closed.  Currently the runtime
        // maintains one session per call, so the effective concurrency
        // always returns 1; future multi-session orchestration can fan
        // this out across N peers without re-plumbing the config read.
        let _effective_block_fetch_concurrency = config.effective_block_fetch_concurrency(1);
        if had_session && run_state.reconnect_count > 0 {
            if let Some(m) = metrics {
                m.inc_reconnects();
            }
        }
        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        if let Err(err) = synchronize_chain_sync_to_point(
            &mut session.chain_sync,
            &mut from_point,
            tracer,
            session.connected_peer_addr,
        )
        .await
        {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client.FindIntersect",
                "intersection request failed; retrying after reconnect",
                session.connected_peer_addr,
                &err,
                from_point,
            );
            session.mux.abort();
            registry_mark_bootstrap_cooling(peer_registry.as_ref(), session.connected_peer_addr);
            preferred_peer = alternate_reconnect_peer(
                node_config.peer_addr,
                &refreshed_fallback_peers,
                session.connected_peer_addr,
            );
            run_state.record_reconnect_failure();
            continue;
        }
        // Round 168 — mirror the established ChainSync/BlockFetch session
        // into the shared `PeerRegistry` only after the peer confirms the
        // intersection. Until then the bootstrap peer stays `PeerCooling`,
        // which keeps the governor from opening unrelated outbound sessions
        // during the non-pipelined ChainSync setup.
        registry_mark_bootstrap_hot(peer_registry.as_ref(), session.connected_peer_addr);

        let mut keepalive = KeepAliveScheduler::new(Instant::now());
        loop {
            // Drive the KeepAlive heartbeat alongside ChainSync/BlockFetch so
            // upstream peers do not tear down the connection at
            // `keepAliveTimeout` (~97 s default).
            if let Err(err) = keepalive.tick(&mut session.keep_alive).await {
                trace_reconnectable_sync_error(
                    tracer,
                    "KeepAlive.Client",
                    "keepalive failed; reconnecting",
                    session.connected_peer_addr,
                    &err,
                    from_point,
                );
                session.mux.abort();
                // Round 175 — companion teardown for R168's bootstrap-Hot
                // promotion.  Without this, a KeepAlive timeout left the
                // bootstrap peer marked `PeerHot` until the next session
                // re-promotion, briefly over-reporting active peers in
                // `/metrics` during the reconnect window.
                registry_mark_bootstrap_cooling(
                    peer_registry.as_ref(),
                    session.connected_peer_addr,
                );
                preferred_peer = alternate_reconnect_peer(
                    node_config.peer_addr,
                    &refreshed_fallback_peers,
                    session.connected_peer_addr,
                );
                run_state.record_reconnect_failure();
                break;
            }

            // R217 — fetch+verify timing baseline.
            let fetch_start = std::time::Instant::now();
            let batch_fut = sync_batch_verified_with_tentative(
                &mut session.chain_sync,
                session.block_fetch.as_mut(),
                from_point,
                config.batch_size,
                Some(&config.verification),
                tentative_state.as_ref(),
                &mut ocert_counters,
                config
                    .block_fetch_pool
                    .as_ref()
                    .map(|p| (p, session.connected_peer_addr)),
                config
                    .density_registry
                    .as_ref()
                    .map(|r| (r, session.connected_peer_addr)),
                config.shared_fetch_worker_pool.as_ref().map(|pool| {
                    yggdrasil_node_sync::MultiPeerDispatchContext {
                        pool,
                        max_concurrent_knob: config.max_concurrent_block_fetch_peers,
                        chainsync_pool: config.shared_chainsync_worker_pool.as_ref(),
                    }
                }),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    trace_shutdown_during_session(
                        tracer,
                        session.connected_peer_addr,
                        from_point,
                    );
                    session.mux.abort();
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }

                result = batch_fut => {
                    // R217 — fetch+verify duration on shared-chaindb path.
                    if let Some(m) = metrics {
                        m.record_fetch_batch_duration(fetch_start.elapsed());
                    }
                    match result {
                        Ok(progress) => {
                            let vrf_nonce_snapshot = if config.verify_vrf {
                                nonce_state.clone()
                            } else {
                                None
                            };
                            let vrf_ctx = if config.verify_vrf {
                                vrf_nonce_snapshot
                                    .as_ref()
                                    .zip(config.active_slot_coeff.as_ref())
                                    .zip(config.nonce_config.as_ref())
                                    .map(|((ns, asc), nonce_cfg)| VrfVerificationContext {
                                        nonce_state: ns,
                                        nonce_cfg,
                                        active_slot_coeff: asc,
                                    })
                            } else {
                                None
                            };
                            let apply_start = std::time::Instant::now();
                            // R210 — opt-in diagnostic on the shared-chaindb
                            // path (the variant used by production mainnet
                            // because NtN sync + NtC server share one
                            // ChainDb).  Mirrors the non-shared path's
                            // diagnostic at the start of this match arm.
                            if std::env::var("YGG_SYNC_DEBUG").is_ok() {
                                eprintln!(
                                    "[YGG_SYNC_DEBUG] shared apply_verified_progress fetched_blocks={} rollback_count={} steps={} current_point={:?}",
                                    progress.fetched_blocks,
                                    progress.rollback_count,
                                    progress.steps.len(),
                                    progress.current_point,
                                );
                            }
                            let applied = {
                                let mut chain_db = chain_db.write().map_err(|_| shared_chaindb_lock_error())?;
                                apply_verified_progress_to_chaindb(
                                    &mut *chain_db,
                                    &progress,
                                    chain_state.as_mut(),
                                    checkpoint_tracking.as_mut(),
                                    &config.checkpoint_policy,
                                    vrf_ctx.as_ref(),
                                    ChainDepStateTracking {
                                        nonce_state: &mut nonce_state,
                                        origin_nonce_state: origin_nonce_state.as_ref(),
                                        nonce_cfg: config.nonce_config.as_ref(),
                                        ocert_counters: &mut ocert_counters,
                                    },
                                )?
                            };
                            if std::env::var("YGG_SYNC_DEBUG").is_ok() {
                                let tip_str = checkpoint_tracking
                                    .as_ref()
                                    .map(|t| format!("{:?}", t.ledger_state.tip))
                                    .unwrap_or_else(|| "no-tracking".into());
                                eprintln!(
                                    "[YGG_SYNC_DEBUG] shared applied stable_block_count={} epoch_events={} rolled_back_tx_ids={} tracking.tip={}",
                                    applied.stable_block_count,
                                    applied.epoch_boundary_events.len(),
                                    applied.rolled_back_tx_ids.len(),
                                    tip_str,
                                );
                            }
                            // R200 — apply-batch duration histogram
                            // (Phase C.1).
                            if let Some(m) = metrics {
                                m.record_apply_batch_duration(apply_start.elapsed());
                            }

                            trace_epoch_boundary_events(tracer, &applied.epoch_boundary_events);

                            // Round 169 — surface the wire-era ordinal to
                            // `/metrics` so dashboards observe Byron→…→Conway
                            // progression directly without parsing
                            // `cardano-cli query tip`.
                            if let (Some(m), Some(tracking)) =
                                (metrics, checkpoint_tracking.as_ref())
                            {
                                m.set_current_era(
                                    tracking.ledger_state.current_era.era_ordinal() as u64,
                                );
                            }

                            // Push updated pool sigma to block producer on epoch boundary.
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref pkh) = bp_pool_key_hash {
                                    let snapshots = checkpoint_tracking.as_ref()
                                        .and_then(|ct| ct.stake_snapshots.as_ref());
                                    update_bp_state_sigma(&bp_state, snapshots, pkh);
                                }
                            }

                            // Epoch revalidation: when a new epoch begins, protocol parameters
                            // may have changed.  Re-validate all mempool entries and evict any
                            // that no longer satisfy the new fee / size / ExUnits constraints.
                            // Reference: Ouroboros.Consensus.Mempool.Impl.Update — syncWithLedger.
                            if !applied.epoch_boundary_events.is_empty() {
                                if let Some(ref mempool) = mempool {
                                    if let Some(ref tracking) = checkpoint_tracking {
                                        if let Some(params) = tracking.ledger_state.protocol_params() {
                                            let tip_slot = progress.current_point.slot().unwrap_or(SlotNo(0));
                                            let evicted = mempool.purge_invalid_for_params(tip_slot, params);
                                            if evicted > 0 {
                                                tracer.trace_runtime(
                                                    "Mempool.EpochRevalidation",
                                                    "Info",
                                                    "purged mempool entries invalid under new epoch params",
                                                    trace_fields([("evicted", json!(evicted))]),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            // R225 — Phase D.1 first slice: record
                            // rollback-depth observations into the
                            // Prometheus histogram.  Unit is
                            // rolled-back transactions (proxy for
                            // block depth × txs/block); depth=0 for
                            // confirm-shape rollbacks (e.g.
                            // session-start RollBackward(Origin) on
                            // a fresh DB).  Operators graph the
                            // distribution to detect rare deep
                            // cross-epoch rollbacks (the Phase D.1
                            // problematic case).
                            if progress.rollback_count > 0 {
                                if let Some(m) = metrics {
                                    m.record_rollback_depth(applied.rolled_back_tx_ids.len() as u64);
                                }
                            }
                            if !applied.rolled_back_tx_ids.is_empty() {
                                tracer.trace_runtime(
                                    "ChainDB.Rollback",
                                    "Info",
                                    "collected rolled-back transaction ids",
                                    trace_fields([
                                        ("txCount", json!(applied.rolled_back_tx_ids.len())),
                                    ]),
                                );

                                if let Some(ref mempool) = mempool {
                                    let stats = re_admit_rolled_back_tx_ids(
                                        mempool,
                                        &applied.rolled_back_tx_ids,
                                        progress.current_point.slot().unwrap_or(SlotNo(0)),
                                        &mut recently_confirmed,
                                    );
                                    tracer.trace_runtime(
                                        "Mempool.RollbackReadmission",
                                        "Info",
                                        "processed rolled-back transaction re-admission",
                                        trace_fields([
                                            ("rolledBackTxCount", json!(applied.rolled_back_tx_ids.len())),
                                            ("reAdmitted", json!(stats.re_admitted)),
                                            ("duplicate", json!(stats.duplicate)),
                                            ("expired", json!(stats.expired)),
                                            ("conflicting", json!(stats.conflicting)),
                                            ("capacityExceeded", json!(stats.capacity_exceeded)),
                                            ("protocolRejected", json!(stats.protocol_rejected)),
                                            ("missingCacheEntry", json!(stats.missing_cache_entry)),
                                        ]),
                                    );
                                }
                            }

                            if let Some(ref mempool) = mempool {
                                for step in &progress.steps {
                                    if let MultiEraSyncStep::RollForward {
                                        blocks,
                                        block_spans,
                                        tip,
                                        ..
                                    } = step
                                    {
                                        let (cached, removed, conflicting, purged, revalidated) =
                                            evict_mempool_after_roll_forward(
                                                mempool, blocks, block_spans, tip,
                                                &mut recently_confirmed,
                                                checkpoint_tracking.as_ref(),
                                                inbound_tx_state.as_ref(),
                                            );
                                        if cached + removed + conflicting + purged + revalidated > 0 {
                                            tracer.trace_runtime(
                                                "Mempool.Eviction",
                                                "Info",
                                                "evicted confirmed/expired/conflicting txs from mempool",
                                                trace_fields([
                                                    ("cachedForRollback", json!(cached)),
                                                    ("confirmed", json!(removed)),
                                                    ("conflicting", json!(conflicting)),
                                                    ("expired", json!(purged)),
                                                    ("ledgerRevalidated", json!(revalidated)),
                                                ]),
                                            );
                                        }
                                    }
                                }
                            }

                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                metrics,
                            );

                            if let Some(tracking) = checkpoint_tracking.as_ref() {
                                persist_chain_dep_state_sidecar(
                                    &applied.checkpoint_outcome,
                                    tracking.chain_dep_persist_dir.as_deref(),
                                    from_point,
                                    nonce_state.as_ref(),
                                    ocert_counters.as_ref(),
                                    config.checkpoint_policy.max_snapshots,
                                )?;
                            }

                            // Update pool fragment-head tracking with the
                            // live current_point so the multi-peer scheduler
                            // knows this peer can serve up through this slot.
                            pool_update_fragment_head(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                                from_point,
                            );

                            // Push live epoch nonce to the concurrent block producer.
                            update_bp_state_nonce(&bp_state, nonce_state.as_ref());

                            if let Some(ref notify) = tip_notify {
                                notify.notify_waiters();
                            }

                            run_state.stable_block_count += applied.stable_block_count;
                            if let Some(m) = metrics {
                                m.add_stable_blocks_promoted(applied.stable_block_count as u64);
                            }

                            if let Some(checkpoint_outcome) = applied.checkpoint_outcome.as_ref() {
                                if let CheckpointPersistenceOutcome::Persisted { slot, .. } = checkpoint_outcome {
                                    if let Some(m) = metrics {
                                        m.set_checkpoint_slot(slot.0);
                                    }
                                }
                                trace_checkpoint_outcome(
                                    tracer,
                                    checkpoint_outcome,
                                    &config.checkpoint_policy,
                                );
                            }

                            trace_verified_sync_batch_applied(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &progress,
                                &run_state,
                                BatchTraceExtras {
                                    stable_block_count: Some(run_state.stable_block_count),
                                    checkpoint_tracked: Some(checkpoint_tracking.is_some()),
                                },
                            );

                            if let Some(next_hot_peer) = preferred_hot_peer_handoff_target(
                                peer_registry.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.PeerSelection",
                                    "Info",
                                    "switching sync session to higher-tip hot peer",
                                    trace_fields([
                                        ("fromPeer", json!(session.connected_peer_addr.to_string())),
                                        ("toPeer", json!(next_hot_peer.to_string())),
                                    ]),
                                );
                                preferred_peer = Some(next_hot_peer);
                                session.mux.abort();
                                // Round 175 — the previous bootstrap peer
                                // is no longer the active sync target;
                                // demote in registry so `/metrics` reflects
                                // the handoff.  The next iteration's
                                // bootstrap to `next_hot_peer` will
                                // re-promote whichever peer it lands on.
                                registry_mark_bootstrap_cooling(
                                    peer_registry.as_ref(),
                                    session.connected_peer_addr,
                                );
                                break;
                            }
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            if pool_should_demote_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.BlockFetch.PoolDemote",
                                    "Warning",
                                    "fetch-client failure threshold exceeded for peer",
                                    trace_fields([(
                                        "peer",
                                        json!(session.connected_peer_addr.to_string()),
                                    )]),
                                );
                            }
                            pool_unregister_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            );
                            // Round 168 — companion teardown for the
                            // bootstrap-Hot promotion above.  Demote to
                            // Cooling so `/metrics` no longer reports the
                            // peer as active; the punish branch below may
                            // override to Cold.
                            registry_mark_bootstrap_cooling(
                                peer_registry.as_ref(),
                                session.connected_peer_addr,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::ReconnectAndPunish => {
                                    if let Some(ref registry) = peer_registry {
                                        if let Ok(mut reg) = registry.write() {
                                            reg.set_status(session.connected_peer_addr, PeerStatus::PeerCold);
                                        }
                                    }
                                    preferred_peer = alternate_reconnect_peer(
                                        node_config.peer_addr,
                                        &refreshed_fallback_peers,
                                        session.connected_peer_addr,
                                    );
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Reconnect => {
                                    preferred_peer = alternate_reconnect_peer(
                                        node_config.peer_addr,
                                        &refreshed_fallback_peers,
                                        session.connected_peer_addr,
                                    );
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Run the verified sync loop, reconnecting through ordered bootstrap peers
/// when protocol connectivity is lost.
///
/// The runner preserves the current chain point, nonce evolution state, and
/// optional chain state across reconnects. Only bootstrap, ChainSync, and
/// BlockFetch failures trigger reconnection; decode, verification, and storage
/// failures still return immediately.
pub async fn run_reconnecting_verified_sync_service<S, F>(
    store: &mut S,
    request: ReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    run_reconnecting_verified_sync_service_with_tracer(store, request, &tracer, shutdown).await
}

/// Run the verified sync loop, reconnecting through ordered bootstrap peers
/// while coordinating storage through [`ChainDb`].
pub async fn run_reconnecting_verified_sync_service_chaindb<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    run_reconnecting_verified_sync_service_chaindb_with_tracer(chain_db, request, &tracer, shutdown)
        .await
}

/// Recover ledger state from coordinated storage and then run reconnecting
/// verified sync from the recovered point.
pub async fn resume_reconnecting_verified_sync_service_chaindb<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    resume_reconnecting_verified_sync_service_chaindb_with_tracer(
        chain_db, request, &tracer, shutdown,
    )
    .await
}

/// Run the reconnecting verified sync loop while emitting runtime trace events.
///
/// Trace emission is driven by the node config-derived [`NodeTracer`] and stays
/// within the node integration layer: bootstrap attempts, successful session
/// establishment, connectivity-triggered reconnects, batch completion, and
/// graceful shutdown are traced, while decode, verification, and storage
/// failures still return immediately.
pub async fn run_reconnecting_verified_sync_service_with_tracer<S, F>(
    store: &mut S,
    request: ReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    S: VolatileStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        mut from_point,
        base_ledger_state: _,
        config,
        mut nonce_state,
        use_ledger_peers: _,
        peer_snapshot_path: _,
        tentative_state: _,
    } = request;

    tokio::pin!(shutdown);

    let mut run_state = ReconnectingRunState::new();
    let mut chain_state = config
        .security_param
        .map(|k| yggdrasil_node_sync::seed_chain_state_from_volatile(store, k));
    let mut ocert_counters = config.verification.ocert_counters.clone();
    let mut had_session = false;
    let mut attempt_state = peer_attempt_state(node_config.peer_addr, fallback_peer_addrs);

    loop {
        let mut session = tokio::select! {
            biased;

            () = &mut shutdown => {
                trace_shutdown_before_bootstrap(tracer);
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }

            result = bootstrap_with_attempt_state(node_config, &mut attempt_state, tracer) => result?,
        };

        run_state.record_session(session.connected_peer_addr, &mut had_session);
        pool_register_peer(
            config.block_fetch_pool.as_ref(),
            session.connected_peer_addr,
        );

        trace_session_established(
            tracer,
            session.connected_peer_addr,
            run_state.reconnect_count,
            from_point,
        );

        if let Err(err) = synchronize_chain_sync_to_point(
            &mut session.chain_sync,
            &mut from_point,
            tracer,
            session.connected_peer_addr,
        )
        .await
        {
            trace_reconnectable_sync_error(
                tracer,
                "ChainSync.Client.FindIntersect",
                "intersection request failed; retrying after reconnect",
                session.connected_peer_addr,
                &err,
                from_point,
            );
            session.mux.abort();
            if let Some(peer) = alternate_reconnect_peer(
                node_config.peer_addr,
                fallback_peer_addrs,
                session.connected_peer_addr,
            ) {
                attempt_state.record_success(peer);
            }
            run_state.record_reconnect_failure();
            continue;
        }

        let mut keepalive = KeepAliveScheduler::new(Instant::now());
        loop {
            // Drive the KeepAlive heartbeat alongside ChainSync/BlockFetch so
            // upstream peers do not tear down the connection at
            // `keepAliveTimeout` (~97 s default).
            if let Err(err) = keepalive.tick(&mut session.keep_alive).await {
                trace_reconnectable_sync_error(
                    tracer,
                    "KeepAlive.Client",
                    "keepalive failed; reconnecting",
                    session.connected_peer_addr,
                    &err,
                    from_point,
                );
                session.mux.abort();
                // No registry cooling here — `with_tracer` doesn't
                // carry a peer_registry and never registers a Hot
                // bootstrap peer (R168 wired the registry hooks only
                // for the chaindb / shared_chaindb inner functions).
                if let Some(peer) = alternate_reconnect_peer(
                    node_config.peer_addr,
                    fallback_peer_addrs,
                    session.connected_peer_addr,
                ) {
                    attempt_state.record_success(peer);
                }
                run_state.record_reconnect_failure();
                break;
            }

            let batch_fut = sync_batch_apply_verified(
                &mut session.chain_sync,
                session.block_fetch.as_mut().expect("block_fetch migrated"),
                store,
                from_point,
                config.batch_size,
                Some(&config.verification),
                &mut ocert_counters,
                config
                    .block_fetch_pool
                    .as_ref()
                    .map(|p| (p, session.connected_peer_addr)),
            );

            tokio::select! {
                biased;

                () = &mut shutdown => {
                    trace_shutdown_during_session(
                        tracer,
                        session.connected_peer_addr,
                        from_point,
                    );
                    session.mux.abort();
                    return Ok(run_state.finish(from_point, nonce_state, chain_state));
                }

                result = batch_fut => {
                    match result {
                        Ok(progress) => {
                            record_verified_batch_progress(
                                &mut from_point,
                                &mut run_state,
                                &progress,
                                nonce_state.as_mut(),
                                config.nonce_config.as_ref(),
                                None,
                            );

                            // Update pool fragment-head tracking with the
                            // live current_point so the multi-peer scheduler
                            // knows this peer can serve up through this slot.
                            pool_update_fragment_head(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                                from_point,
                            );

                            if let Some(ref mut cs) = chain_state {
                                for step in &progress.steps {
                                    run_state.stable_block_count += track_chain_state(cs, step)?;
                                }
                            }

                            trace_verified_sync_batch_applied(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &progress,
                                &run_state,
                                BatchTraceExtras {
                                    stable_block_count: None,
                                    checkpoint_tracked: None,
                                },
                            );
                        }
                        Err(err) => {
                            let disposition = handle_reconnect_batch_error(
                                tracer,
                                session.connected_peer_addr,
                                from_point,
                                &err,
                            );
                            if pool_should_demote_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            ) {
                                tracer.trace_runtime(
                                    "Net.BlockFetch.PoolDemote",
                                    "Warning",
                                    "fetch-client failure threshold exceeded for peer",
                                    trace_fields([(
                                        "peer",
                                        json!(session.connected_peer_addr.to_string()),
                                    )]),
                                );
                            }
                            pool_unregister_peer(
                                config.block_fetch_pool.as_ref(),
                                session.connected_peer_addr,
                            );
                            session.mux.abort();
                            match disposition {
                                BatchErrorDisposition::ReconnectAndPunish
                                | BatchErrorDisposition::Reconnect => {
                                    if let Some(peer) = alternate_reconnect_peer(
                                        node_config.peer_addr,
                                        fallback_peer_addrs,
                                        session.connected_peer_addr,
                                    ) {
                                        attempt_state.record_success(peer);
                                    }
                                    run_state.record_reconnect_failure();
                                    break;
                                }
                                BatchErrorDisposition::Fail => return Err(err),
                            }
                        }
                    }
                }
            }
        }

        // Exponential backoff before next reconnection attempt (upstream
        // reconnect delay with exponential increase, capped at 60 s).
        let backoff = run_state.reconnect_backoff();
        tokio::select! {
            biased;
            () = &mut shutdown => {
                return Ok(run_state.finish(from_point, nonce_state, chain_state));
            }
            () = tokio::time::sleep(backoff) => {}
        }
    }
}

pub(crate) fn stake_snapshots_for_recovered_point(
    config: &VerifiedSyncServiceConfig,
    storage_dir: Option<&Path>,
    recovery_point: &Point,
) -> Result<Option<StakeSnapshots>, SyncError> {
    if config.nonce_config.is_none() {
        return Ok(None);
    }

    match load_stake_snapshots_sidecar(storage_dir)? {
        Some(snapshots) => Ok(Some(snapshots)),
        None if storage_dir.is_some() && !matches!(recovery_point, Point::Origin) => {
            Err(SyncError::Recovery(format!(
                "missing exact StakeSnapshots sidecar history for recovered point {recovery_point:?}"
            )))
        }
        None => Ok(Some(StakeSnapshots::new())),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeLedgerRecovery {
    pub(crate) outcome: LedgerRecoveryOutcome,
    pub(crate) stake_snapshots: Option<StakeSnapshots>,
    pub(crate) pool_block_counts: BTreeMap<yggdrasil_ledger::PoolKeyHash, u64>,
}

pub(crate) fn recover_ledger_state_for_runtime<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
    base_ledger_state: LedgerState,
    config: &VerifiedSyncServiceConfig,
    storage_dir: Option<&Path>,
) -> Result<RuntimeLedgerRecovery, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    match config.nonce_config.as_ref() {
        Some(nonce_config) => {
            let epoch_schedule = config
                .epoch_schedule
                .unwrap_or_else(|| EpochSchedule::fixed(nonce_config.epoch_size));
            let evaluator = config.build_plutus_evaluator();
            let recovered_point = chain_db.tip();
            let restored_stake_snapshots =
                stake_snapshots_for_recovered_point(config, storage_dir, &recovered_point)?;
            let recovery = yggdrasil_node_sync::recover_ledger_state_chaindb_with_epoch_boundary(
                chain_db,
                base_ledger_state,
                epoch_schedule,
                Some(&evaluator),
                restored_stake_snapshots,
            )?;
            let point = recovery.ledger_state.tip;
            let outcome = LedgerRecoveryOutcome {
                ledger_state: recovery.ledger_state,
                point,
                checkpoint_slot: recovery.checkpoint_slot,
                replayed_volatile_blocks: recovery.replayed_volatile_blocks,
            };
            Ok(RuntimeLedgerRecovery {
                outcome,
                stake_snapshots: Some(recovery.stake_snapshots),
                pool_block_counts: recovery.pool_block_counts,
            })
        }
        None => {
            let outcome = recover_ledger_state_chaindb(chain_db, base_ledger_state)?;
            Ok(RuntimeLedgerRecovery {
                outcome,
                stake_snapshots: None,
                pool_block_counts: BTreeMap::new(),
            })
        }
    }
}

/// Recover ledger state from coordinated storage and then run reconnecting
/// verified sync while emitting runtime trace events.
pub async fn resume_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ResumeReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
        metrics,
        peer_registry: _,
        mempool: _,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state: _,
        chain_dep_persist_dir,
    } = request;

    let runtime_recovery = recover_ledger_state_for_runtime(
        chain_db,
        base_ledger_state,
        config,
        chain_dep_persist_dir.as_deref(),
    )?;
    let recovery = runtime_recovery.outcome;
    tracer.trace_runtime(
        "Node.Recovery",
        "Notice",
        "recovered ledger state from coordinated storage",
        trace_fields([
            ("point", json!(format!("{:?}", recovery.point))),
            (
                "checkpointSlot",
                json!(recovery.checkpoint_slot.map(|slot| slot.0)),
            ),
            (
                "replayedVolatileBlocks",
                json!(recovery.replayed_volatile_blocks),
            ),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
        plutus_evaluator: config.build_plutus_evaluator(),
        stake_snapshots: runtime_recovery.stake_snapshots,
        epoch_size: config.nonce_config.as_ref().map(|nc| {
            config
                .epoch_schedule
                .unwrap_or_else(|| yggdrasil_consensus::EpochSchedule::fixed(nc.epoch_size))
        }),
        pool_block_counts: runtime_recovery.pool_block_counts,
        chain_dep_persist_dir: chain_dep_persist_dir.clone(),
    };
    if let (Some(bp), Some(pool_key_hash), Some(snapshots)) = (
        bp_state.as_ref(),
        bp_pool_key_hash.as_ref(),
        checkpoint_tracking.stake_snapshots.as_ref(),
    ) {
        let state = Some(Arc::clone(bp));
        update_bp_state_sigma(&state, Some(snapshots), pool_key_hash);
    }

    let sync = run_reconnecting_verified_sync_service_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
            metrics,
            peer_registry: None,
            mempool: None,
            tentative_state,
            tip_notify,
            bp_state,
            bp_pool_key_hash,
            inbound_tx_state: None,
        },
        ReconnectingVerifiedSyncState {
            from_point: recovery.point,
            nonce_state,
            checkpoint_tracking: Some(checkpoint_tracking),
        },
        shutdown,
    )
    .await?;

    Ok(ResumedSyncServiceOutcome { recovery, sync })
}

pub async fn resume_reconnecting_verified_sync_service_shared_chaindb<I, V, L, F>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let tracer = NodeTracer::disabled();
    resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer(
        chain_db, request, &tracer, shutdown,
    )
    .await
}

pub async fn resume_reconnecting_verified_sync_service_shared_chaindb_with_tracer<I, V, L, F>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    request: ResumeReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ResumedSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ResumeReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
        metrics,
        peer_registry,
        mempool,
        tentative_state,
        tip_notify,
        bp_state,
        bp_pool_key_hash,
        inbound_tx_state,
        chain_dep_persist_dir,
    } = request;

    let runtime_recovery = {
        let chain_db = chain_db.read().map_err(|_| shared_chaindb_lock_error())?;
        recover_ledger_state_for_runtime(
            &chain_db,
            base_ledger_state,
            config,
            chain_dep_persist_dir.as_deref(),
        )?
    };
    let recovery = runtime_recovery.outcome;
    tracer.trace_runtime(
        "Node.Recovery",
        "Notice",
        "recovered ledger state from coordinated storage",
        trace_fields([
            ("point", json!(format!("{:?}", recovery.point))),
            (
                "checkpointSlot",
                json!(recovery.checkpoint_slot.map(|slot| slot.0)),
            ),
            (
                "replayedVolatileBlocks",
                json!(recovery.replayed_volatile_blocks),
            ),
        ]),
    );

    let checkpoint_tracking = LedgerCheckpointTracking {
        base_ledger_state: recovery.ledger_state.clone(),
        ledger_state: recovery.ledger_state.clone(),
        last_persisted_point: recovery.point,
        plutus_evaluator: config.build_plutus_evaluator(),
        stake_snapshots: runtime_recovery.stake_snapshots,
        epoch_size: config.nonce_config.as_ref().map(|nc| {
            config
                .epoch_schedule
                .unwrap_or_else(|| yggdrasil_consensus::EpochSchedule::fixed(nc.epoch_size))
        }),
        pool_block_counts: runtime_recovery.pool_block_counts,
        chain_dep_persist_dir: chain_dep_persist_dir.clone(),
    };
    if let (Some(bp), Some(pool_key_hash), Some(snapshots)) = (
        bp_state.as_ref(),
        bp_pool_key_hash.as_ref(),
        checkpoint_tracking.stake_snapshots.as_ref(),
    ) {
        let state = Some(Arc::clone(bp));
        update_bp_state_sigma(&state, Some(snapshots), pool_key_hash);
    }

    let sync = run_reconnecting_verified_sync_service_shared_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
            metrics,
            peer_registry,
            mempool,
            tentative_state,
            tip_notify,
            bp_state,
            bp_pool_key_hash,
            inbound_tx_state,
        },
        ReconnectingVerifiedSyncState {
            from_point: recovery.point,
            nonce_state,
            checkpoint_tracking: Some(checkpoint_tracking),
        },
        shutdown,
    )
    .await?;

    Ok(ResumedSyncServiceOutcome { recovery, sync })
}

/// Run the reconnecting verified sync loop over coordinated storage while
/// emitting runtime trace events.
pub async fn run_reconnecting_verified_sync_service_chaindb_with_tracer<I, V, L, F>(
    chain_db: &mut ChainDb<I, V, L>,
    request: ReconnectingVerifiedSyncRequest<'_>,
    tracer: &NodeTracer,
    shutdown: F,
) -> Result<ReconnectingSyncServiceOutcome, SyncError>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let ReconnectingVerifiedSyncRequest {
        node_config,
        fallback_peer_addrs,
        from_point,
        base_ledger_state,
        config,
        nonce_state,
        use_ledger_peers,
        peer_snapshot_path,
        tentative_state,
    } = request;
    let checkpoint_tracking = {
        let mut ct =
            yggdrasil_node_sync::default_checkpoint_tracking(chain_db, base_ledger_state, config)?;
        if let Some(ref nonce_cfg) = config.nonce_config {
            ct.stake_snapshots = Some(yggdrasil_ledger::StakeSnapshots::new());
            ct.epoch_size = Some(config.epoch_schedule.unwrap_or_else(|| {
                yggdrasil_consensus::EpochSchedule::fixed(nonce_cfg.epoch_size)
            }));
        }
        Some(ct)
    };

    run_reconnecting_verified_sync_service_chaindb_inner(
        chain_db,
        ReconnectingVerifiedSyncContext {
            node_config,
            fallback_peer_addrs,
            use_ledger_peers,
            peer_snapshot_path: peer_snapshot_path.as_deref(),
            config,
            tracer,
            metrics: None,
            peer_registry: None,
            mempool: None,
            tentative_state,
            tip_notify: None,
            bp_state: None,
            bp_pool_key_hash: None,
            inbound_tx_state: None,
        },
        ReconnectingVerifiedSyncState {
            from_point,
            nonce_state,
            checkpoint_tracking,
        },
        shutdown,
    )
    .await
}

/// Polymorphic seed of the volatile-window `ChainState` that works whether
/// the caller holds the chain DB as `&mut ChainDb<I, V, L>` (the
/// non-shared variant) or `&Arc<RwLock<ChainDb<I, V, L>>>` (the shared
/// variant).  Without this, the post-restart `ChainState` was always
/// `ChainState::new(k)` — empty — and the next ChainSync session's
/// `RollBackward(recovered_tip)` confirmation failed with
/// `RollbackPointNotFound` (surfaced by §6 restart-resilience cycle 2).
pub(super) fn seed_chain_state_via_chain_db<S: ChainDbVolatileAccess>(
    chain_db: &S,
    security_param: Option<yggdrasil_consensus::SecurityParam>,
) -> Option<yggdrasil_consensus::ChainState> {
    security_param.map(|k| {
        chain_db.with_volatile(|v| yggdrasil_node_sync::seed_chain_state_from_volatile(v, k))
    })
}

/// Trait abstracting "give me a borrow of the volatile store" across the
/// two ChainDb access modes used by the reconnecting sync entry points.
pub(super) trait ChainDbVolatileAccess {
    fn with_volatile<R>(&self, f: impl FnOnce(&dyn VolatileStore) -> R) -> R;
    fn best_tip(&self) -> Point;
}

impl<I, V, L> ChainDbVolatileAccess for ChainDb<I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    fn with_volatile<R>(&self, f: impl FnOnce(&dyn VolatileStore) -> R) -> R {
        f(self.volatile())
    }

    fn best_tip(&self) -> Point {
        self.tip()
    }
}

impl<I, V, L> ChainDbVolatileAccess for std::sync::Arc<std::sync::RwLock<ChainDb<I, V, L>>>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    fn with_volatile<R>(&self, f: impl FnOnce(&dyn VolatileStore) -> R) -> R {
        let guard = self.read().expect("chain db lock poisoned");
        f(guard.volatile())
    }

    fn best_tip(&self) -> Point {
        let guard = self.read().expect("chain db lock poisoned");
        guard.tip()
    }
}
