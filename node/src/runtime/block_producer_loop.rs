//! Block-producer slot loop.
//!
//! Mirrors upstream `Ouroboros.Consensus.Node.NodeKernel.forkBlockForging` —
//! the slot-by-slot leader-check + block-forging async task that runs
//! alongside the governor and verified-sync service.
//!
//! Each slot tick:
//! 1. Read the live ChainDb tip.
//! 2. Compute the live `LedgerStateJudgement` and skip if `TooOld`.
//! 3. Build a `BlockContext` (slot + tip ancestor) and skip if the slot
//!    is immutable or already-occupied.
//! 4. Read the live epoch nonce + per-pool sigma from
//!    [`super::SharedBlockProducerState`].
//! 5. Run the Praos VRF + KES leader check.
//! 6. On `ShouldForge`, assemble the body from the mempool, sign the
//!    header, self-validate, and atomically insert into volatile ChainDb.
//! 7. Trace one of `TraceNodeNotLeader` / `TraceNodeIsLeader` /
//!    `TraceForgedBlock` / `TraceAdoptedBlock` per upstream's
//!    `Ouroboros.Consensus.Node.Tracers` taxonomy.
//!
//! Extracted from `runtime.rs` in R271k.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side async slot-by-slot
//! block-producer loop. Mirrors the runtime body of upstream
//! `Ouroboros.Consensus.Node.NodeKernel.forkBlockForging`, but
//! Haskell wires the leader-check + forge thread inline inside
//! `forkBlockForging` rather than as a separate file. Yggdrasil
//! isolates the slot loop here so the runtime orchestrator stays
//! thin.

use std::future::Future;
use std::sync::{Arc, RwLock};

use serde_json::json;
use yggdrasil_consensus::mempool::SharedMempool;
use yggdrasil_ledger::{Point, SlotNo};
use yggdrasil_network::LedgerStateJudgement;
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::block_producer::{
    BlockProducerCredentials, ShouldForge, SlotClock, assemble_block_body, check_should_forge,
    forge_block, forged_block_to_storage_block, make_block_context,
};
use crate::tracer::{NodeMetrics, NodeTracer, trace_fields};

use super::block_producer_config::{RuntimeBlockProducerConfig, SharedBlockProducerState};
use super::{
    ChainTipNotify, block_producer_ledger_state_judgement, kes_expiry_warning,
    mempool_entries_for_forging, self_validate_forged_block, tip_context_from_chain_db,
};

/// Run the local block-producer loop until shutdown.
///
/// The loop advances a relative slot clock, evaluates Praos leadership using
/// loaded block-producer credentials, assembles a block body from the current
/// fee-ordered mempool snapshot, forges/signs a header, and inserts the new
/// block into volatile ChainDb storage.
#[allow(clippy::too_many_arguments)]
pub async fn run_block_producer_loop<I, V, L, F>(
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    mut credentials: BlockProducerCredentials,
    config: RuntimeBlockProducerConfig,
    _tip_notify: Option<ChainTipNotify>,
    bp_state: Option<Arc<RwLock<SharedBlockProducerState>>>,
    tracer: NodeTracer,
    metrics: Option<Arc<NodeMetrics>>,
    shutdown: F,
) where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
    F: Future<Output = ()>,
{
    let (tip_slot, _, _) = {
        let db = chain_db.read().expect("chain db lock poisoned");
        tip_context_from_chain_db(&db)
    };
    let anchor_slot = tip_slot
        .map(|slot| SlotNo(slot.0.saturating_add(1)))
        .unwrap_or(SlotNo(0));
    let (slot_clock, clock_mode) = match config.system_start_unix_secs {
        Some(system_start) => (
            SlotClock::from_system_start(system_start, config.slot_length),
            "system-start",
        ),
        None => (
            SlotClock::new(anchor_slot, config.slot_length),
            "relative-anchor",
        ),
    };

    let mut interval = tokio::time::interval(config.slot_length);
    let mut last_checked_slot: Option<SlotNo> = None;
    let mut last_kes_warning_period: Option<u64> = None;
    let mut last_ledger_judgement: Option<LedgerStateJudgement> = None;
    let mut waiting_for_live_ledger_view_reported = false;
    tokio::pin!(shutdown);

    tracer.trace_runtime(
        "Node.BlockProduction",
        "Notice",
        "block producer loop started",
        trace_fields([
            ("anchorSlot", json!(anchor_slot.0)),
            ("slotLengthSecs", json!(config.slot_length.as_secs())),
            ("clockMode", json!(clock_mode)),
        ]),
    );

    loop {
        tokio::select! {
            biased;

            () = &mut shutdown => {
                tracer.trace_runtime(
                    "Node.BlockProduction",
                    "Notice",
                    "block producer loop stopped",
                    std::collections::BTreeMap::new(),
                );
                return;
            }

            _ = interval.tick() => {
                let current_slot = slot_clock.current_slot();
                if last_checked_slot
                    .map(|last| current_slot <= last)
                    .unwrap_or(false)
                {
                    continue;
                }
                last_checked_slot = Some(current_slot);

                if let Some(kes) = kes_expiry_warning(&credentials, current_slot) {
                    if last_kes_warning_period != Some(kes.current_period) {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "operational certificate nearing KES expiry",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("currentKesPeriod", json!(kes.current_period)),
                                ("certStartKesPeriod", json!(kes.cert_start_period)),
                                ("certEndKesPeriod", json!(kes.cert_end_period)),
                                ("remainingKesPeriods", json!(kes.remaining_periods)),
                                ("remainingKesSlots", json!(kes.remaining_slots)),
                            ]),
                        );
                        last_kes_warning_period = Some(kes.current_period);
                    }
                }

                let (tip_slot, tip_block_no, tip_hash) = {
                    let db = chain_db.read().expect("chain db lock poisoned");
                    tip_context_from_chain_db(&db)
                };

                let ledger_judgement = block_producer_ledger_state_judgement(tip_slot, &config);
                if ledger_judgement != LedgerStateJudgement::YoungEnough {
                    if last_ledger_judgement != Some(ledger_judgement) {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "ledger state is not recent enough for block production",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("tipSlot", json!(tip_slot.map(|s| s.0))),
                                ("judgement", json!(format!("{ledger_judgement:?}"))),
                            ]),
                        );
                        last_ledger_judgement = Some(ledger_judgement);
                    }
                    continue;
                }
                last_ledger_judgement = Some(ledger_judgement);

                let Some(context) = make_block_context(
                    current_slot,
                    tip_slot,
                    tip_block_no,
                    tip_hash,
                ) else {
                    // Upstream: TraceSlotIsImmutable — emitted when the
                    // current slot is not strictly ahead of the chain tip
                    // slot, meaning forging would target an immutable or
                    // already-occupied slot. The forge loop must skip this
                    // slot rather than silently dropping it from the trace
                    // record.
                    //
                    // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                    // `TraceSlotIsImmutable` and `NodeKernel.forkBlockForging`
                    // (`mkCurrentBlockContext` returning `Left ImmutableSlot`).
                    tracer.trace_runtime(
                        "Node.BlockProduction",
                        "Warning",
                        "slot is immutable",
                        trace_fields([
                            ("slot", json!(current_slot.0)),
                            ("tipSlot", json!(tip_slot.map(|s| s.0))),
                        ]),
                    );
                    continue;
                };

                // Upstream: TraceStartLeadershipCheck — emitted at the start
                // of every slot's leadership check, before the VRF/KES
                // evaluation. Operators rely on this event for per-slot
                // forge-loop liveness monitoring.
                //
                // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                // `TraceStartLeadershipCheck` and `NodeKernel.forkBlockForging`
                // (`traceWith tracer (TraceStartLeadershipCheck currentSlot)`).
                tracer.trace_runtime(
                    "Node.BlockProduction",
                    "Debug",
                    "starting leadership check",
                    trace_fields([
                        ("slot", json!(current_slot.0)),
                        ("blockNo", json!(context.block_number.0)),
                    ]),
                );

                // Read live epoch nonce and sigma from the shared state
                // updated by the sync pipeline, falling back to the static
                // startup values in config when unavailable.
                //
                // Reference: upstream `forkBlockForging` re-reads the ledger
                // view's epoch nonce and per-pool relative stake each slot.
                let (live_nonce, live_sigma_num, live_sigma_den) = if let Some(bp) = bp_state.as_ref() {
                    let bp_snapshot = bp.read().ok().map(|st| st.clone());
                    match bp_snapshot.and_then(|snapshot| {
                        Some((snapshot.epoch_nonce?, snapshot.sigma?))
                    }) {
                        Some((nonce, (sn, sd))) => {
                            waiting_for_live_ledger_view_reported = false;
                            (nonce, sn, sd)
                        }
                        None => {
                            if !waiting_for_live_ledger_view_reported {
                                tracer.trace_runtime(
                                    "Node.BlockProduction",
                                    "Info",
                                    "waiting for live nonce and stake distribution before forging",
                                    trace_fields([
                                        ("slot", json!(current_slot.0)),
                                        ("tipSlot", json!(tip_slot.map(|s| s.0))),
                                    ]),
                                );
                                waiting_for_live_ledger_view_reported = true;
                            }
                            continue;
                        }
                    }
                } else {
                    (config.epoch_nonce, config.sigma_num, config.sigma_den)
                };

                let should_forge = check_should_forge(
                    &mut credentials,
                    current_slot,
                    live_nonce,
                    live_sigma_num,
                    live_sigma_den,
                    &config.active_slot_coeff,
                );

                let election = match should_forge {
                    ShouldForge::NotLeader => {
                        // Upstream: TraceNodeNotLeader — emitted whenever
                        // the slot leadership check determined the node is
                        // not the elected leader for this slot. Kept at
                        // Debug severity to match upstream's high-frequency
                        // per-slot tracing.
                        //
                        // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                        // `TraceNodeNotLeader` and `NodeKernel.forkBlockForging`.
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Debug",
                            "not slot leader",
                            trace_fields([("slot", json!(current_slot.0))]),
                        );
                        continue;
                    }
                    ShouldForge::ForgeStateUpdateError(err) => {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "forge-state update failed",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("error", json!(err)),
                            ]),
                        );
                        continue;
                    }
                    ShouldForge::CannotForge(err) => {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "cannot forge in elected slot",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("error", json!(err)),
                            ]),
                        );
                        continue;
                    }
                    ShouldForge::ShouldForge(election) => election,
                };

                // Upstream: TraceNodeIsLeader — emitted once leader election
                // has succeeded for this slot and before block construction
                // begins. Operators rely on this event to count elected
                // slots and reconcile against `TraceForgedBlock` /
                // `TraceAdoptedBlock`.
                //
                // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                // `TraceNodeIsLeader` and `NodeKernel.forkBlockForging`.
                tracer.trace_runtime(
                    "Node.BlockProduction",
                    "Notice",
                    "elected as slot leader",
                    trace_fields([
                        ("slot", json!(current_slot.0)),
                        ("blockNo", json!(context.block_number.0)),
                    ]),
                );

                let entries = mempool_entries_for_forging(&mempool);
                let (selected_preview, selected_size) =
                    assemble_block_body(entries.iter(), config.max_block_body_size);
                let selected_count = selected_preview.len();

                let issuer_vkey = credentials.issuer_vkey.clone();

                let forged = match forge_block(
                    &credentials,
                    &election,
                    &context,
                    current_slot,
                    &entries,
                    config.max_block_body_size,
                    issuer_vkey,
                    config.protocol_version,
                ) {
                    Ok(forged) => forged,
                    Err(err) => {
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Error",
                            "failed to forge block",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("blockNo", json!(context.block_number.0)),
                                ("error", json!(err.to_string())),
                            ]),
                        );
                        continue;
                    }
                };

                if let Err(err) = self_validate_forged_block(&forged) {
                    // Upstream: TraceForgedInvalidBlock — emitted at
                    // Critical severity when a locally forged block fails
                    // self-validation (protocol-version, body-hash,
                    // body-size, or header-identity check). This is more
                    // serious than a peer's invalid block: it indicates a
                    // local mempool/validation inconsistency that produced
                    // a malformed block, and operators must investigate.
                    //
                    // Reference: cardano-node `Ouroboros.Consensus.Node.Tracers`
                    // `TraceForgedInvalidBlock` and `NodeKernel.forkBlockForging`
                    // (post-forge `getIsInvalidBlock` check).
                    tracer.trace_runtime(
                        "Node.BlockProduction",
                        "Critical",
                        "forged invalid block (self-validation failed)",
                        trace_fields([
                            ("slot", json!(forged.slot.0)),
                            ("blockNo", json!(forged.block_number.0)),
                            ("headerHash", json!(hex::encode(forged.header_hash.0))),
                            ("error", json!(err.to_string())),
                        ]),
                    );
                    continue;
                }

                let storage_block = forged_block_to_storage_block(&forged);
                let add_result = {
                    let mut db = chain_db.write().expect("chain db lock poisoned");
                    db.add_volatile_block(storage_block)
                };

                match add_result {
                    Ok(()) => {
                        // Upstream: TraceForgedBlock — always emitted after
                        // successful Block.forgeBlock.
                        // Reference: NodeKernel.hs forkBlockForging ~line 735
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Notice",
                            "forged local block",
                            trace_fields([
                                ("slot", json!(forged.slot.0)),
                                ("blockNo", json!(forged.block_number.0)),
                                ("txCount", json!(selected_count)),
                                ("bodySize", json!(selected_size)),
                                ("headerHash", json!(hex::encode(forged.header_hash.0))),
                            ]),
                        );

                        // -- Post-forge adoption check --
                        // Upstream: NodeKernel.hs ~lines 746-785
                        // After adding the block, check whether our block
                        // became the new tip of the chain.
                        //
                        // In upstream Haskell this uses addBlockAsync +
                        // blockProcessed (STM wait) + getIsInvalidBlock.
                        // Our storage is synchronous so we can check
                        // immediately after add_volatile_block().
                        let adopted = {
                            let db = chain_db.read().expect("chain db lock poisoned");
                            match db.tip() {
                                Point::BlockPoint(tip_s, tip_h) => {
                                    tip_s == forged.slot && tip_h == forged.header_hash
                                }
                                Point::Origin => false,
                            }
                        };

                        if adopted {
                            // Upstream: TraceAdoptedBlock — block adopted
                            // successfully, normal path.
                            let confirmed_ids = forged
                                .transactions
                                .iter()
                                .map(|tx| tx.tx_id)
                                .collect::<Vec<_>>();
                            let removed = if confirmed_ids.is_empty() {
                                0
                            } else {
                                mempool.remove_confirmed(&confirmed_ids)
                            };

                            if let Some(ref m) = metrics {
                                m.add_blocks_synced(1);
                                m.set_current_slot(forged.slot.0);
                                m.set_current_block_number(forged.block_number.0);
                                m.set_mempool_gauges(mempool.len() as u64, mempool.size_bytes() as u64);
                            }

                            tracer.trace_runtime(
                                "Node.BlockProduction",
                                "Notice",
                                "adopted forged block",
                                trace_fields([
                                    ("slot", json!(forged.slot.0)),
                                    ("blockNo", json!(forged.block_number.0)),
                                    ("txCount", json!(selected_count)),
                                    ("mempoolEvicted", json!(removed)),
                                    ("headerHash", json!(hex::encode(forged.header_hash.0))),
                                ]),
                            );

                            // Wake ChainSync servers so they can push the
                            // new header to connected peers immediately
                            // without busy-polling.
                            if let Some(ref notify) = _tip_notify {
                                notify.notify_waiters();
                            }
                        } else {
                            // Upstream: TraceDidntAdoptBlock — block was
                            // valid but not adopted (another leader's block
                            // was preferred by chain selection).
                            //
                            // This is a warning-level event: it means a
                            // competing slot leader's block was adopted
                            // instead.  If our storage had an invalid-block
                            // set we would also check getIsInvalidBlock and
                            // emit TraceForgedInvalidBlock (critical) for
                            // mempool/validation inconsistencies.
                            tracer.trace_runtime(
                                "Node.BlockProduction",
                                "Warning",
                                "did not adopt forged block",
                                trace_fields([
                                    ("slot", json!(forged.slot.0)),
                                    ("blockNo", json!(forged.block_number.0)),
                                    ("headerHash", json!(hex::encode(forged.header_hash.0))),
                                ]),
                            );
                        }
                    }
                    Err(err) => {
                        // Upstream: FailedToAddBlock — the block could not
                        // be added to ChainDB at all.
                        tracer.trace_runtime(
                            "Node.BlockProduction",
                            "Warning",
                            "failed to persist forged block",
                            trace_fields([
                                ("slot", json!(current_slot.0)),
                                ("blockNo", json!(context.block_number.0)),
                                ("error", json!(err.to_string())),
                            ]),
                        );
                    }
                }
            }
        }
    }
}
