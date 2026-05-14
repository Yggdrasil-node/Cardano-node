//! Ledger-derived fallback peer assembly.
//!
//! Mirrors upstream `Ouroboros.Network.PeerSelection.LedgerPeers` — at
//! startup we resolve a `LedgerPeerSnapshot` from
//! (a) the operator-supplied `peer_snapshot_file` overlay (always
//! eligible per R250) and (b) the live ledger state once the
//! `useLedgerAfterSlot` gate is open. Both feeds are merged for
//! observability but emitted with split-gate eligibility so initial
//! sync gets multi-peer fanout immediately.
//!
//! Reference: <https://github.com/IntersectMBO/ouroboros-network/blob/master/ouroboros-network/src/Ouroboros/Network/PeerSelection/LedgerPeers.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side runtime hookup that
//! feeds ledger-derived peer information into the network
//! crate's peer registry. Mirrors upstream
//! `Cardano.Node.Diffusion.Configuration.LedgerPeers` glue.
//! Upstream wires this inline; Yggdrasil isolates the runtime-
//! side peer-source bridge here.

use std::net::SocketAddr;

use serde_json::json;

use yggdrasil_ledger::{LedgerState, Point, PoolRelayAccessPoint};
use yggdrasil_network::{
    LedgerPeerSnapshot, LedgerPeerUseDecision, LedgerStateJudgement, PeerAccessPoint,
    merge_ledger_peer_snapshots, resolve_peer_access_points,
};
use yggdrasil_node_config::{NodeConfigFile, load_peer_snapshot_file};
use yggdrasil_node_tracer::{NodeTracer, trace_fields};

/// Project the slot number of an `Ouroboros.Network.Block.Point`,
/// returning `None` for `Origin` (mirrors upstream `pointSlot`).
pub(crate) fn point_slot(point: &Point) -> Option<u64> {
    match point {
        Point::Origin => None,
        Point::BlockPoint(slot, _) => Some(slot.0),
    }
}

fn extend_unique_peers(target: &mut Vec<SocketAddr>, peers: impl IntoIterator<Item = SocketAddr>) {
    for peer in peers {
        if !target.contains(&peer) {
            target.push(peer);
        }
    }
}

fn extend_unique_ledger_peers(
    target: &mut Vec<SocketAddr>,
    access_points: impl IntoIterator<Item = PoolRelayAccessPoint>,
) {
    for access_point in access_points {
        let peer_access_point = PeerAccessPoint {
            address: access_point.address,
            port: access_point.port,
        };
        extend_unique_peers(target, resolve_peer_access_points(&peer_access_point));
    }
}

/// Build a `LedgerPeerSnapshot` from the live ledger state by walking
/// every registered pool's relay access points and DNS-resolving each
/// to one or more concrete `SocketAddr`s. The big-ledger-peer slot
/// stays empty here — only fully-resolved relay endpoints land in the
/// `ledger_peers` slot.
pub(crate) fn ledger_peer_snapshot_from_ledger_state(
    ledger_state: &LedgerState,
) -> LedgerPeerSnapshot {
    let mut ledger_peers = Vec::new();
    extend_unique_ledger_peers(
        &mut ledger_peers,
        ledger_state.pool_state().relay_access_points(),
    );
    LedgerPeerSnapshot::new(ledger_peers, Vec::new())
}

/// Produce the operator-side fallback peer list for the diffusion
/// governor at startup, layering ordered config peers + snapshot
/// overlay (always eligible) + live-ledger peers (gated by
/// `useLedgerAfterSlot`).
///
/// R250 split: snapshot peers populate `bigLedgerPeers` immediately so
/// initial sync is multi-peer from genesis; live-ledger-derived peers
/// keep waiting for the gate to open. Mirrors upstream
/// `Ouroboros.Network.PeerSelection.LedgerPeers`.
pub(crate) fn configured_fallback_peers(
    file_cfg: &NodeConfigFile,
    config_base_dir: Option<&std::path::Path>,
    ledger_snapshot: &LedgerPeerSnapshot,
    latest_slot: Option<u64>,
    ledger_state_judgement: LedgerStateJudgement,
    tracer: &NodeTracer,
) -> Vec<SocketAddr> {
    let mut fallback_peers = file_cfg.ordered_fallback_peers();

    let mut snapshot_slot = None;
    let mut snapshot_available = file_cfg.peer_snapshot_file.is_none();
    let mut snapshot_path = None;
    let mut snapshot_file = None;

    if let Some(peer_snapshot_file) = file_cfg.peer_snapshot_file.as_deref() {
        let peer_snapshot_path =
            crate::resolve_config_path(std::path::Path::new(peer_snapshot_file), config_base_dir);
        snapshot_path = Some(peer_snapshot_path.clone());

        match load_peer_snapshot_file(&peer_snapshot_path) {
            Ok(loaded_snapshot) => {
                snapshot_slot = loaded_snapshot.slot;
                snapshot_available = true;
                snapshot_file = Some(loaded_snapshot.snapshot);
            }
            Err(err) => {
                let freshness = file_cfg.peer_snapshot_freshness(None, latest_slot, false);
                let (decision, _) = file_cfg.eligible_ledger_fallback_peers(
                    ledger_snapshot,
                    latest_slot,
                    ledger_state_judgement,
                    freshness,
                );

                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to load peer snapshot fallbacks",
                    trace_fields([
                        ("decision", json!(format!("{decision:?}"))),
                        ("latestSlot", json!(latest_slot)),
                        (
                            "snapshotPath",
                            json!(peer_snapshot_path.display().to_string()),
                        ),
                        ("error", json!(err.to_string())),
                    ]),
                );
            }
        }
    }

    // R250 — split snapshot-overlay path from live-ledger path so snapshot
    // peers (loaded from `peerSnapshotFile`) are eligible immediately at
    // startup, while live-ledger-derived peers continue to wait for the
    // `useLedgerAfterSlot` gate. Upstream
    // `Ouroboros.Network.PeerSelection.LedgerPeers` does the same: only LIVE
    // chain-derived peers wait for the gate; snapshot peers populate
    // `bigLedgerPeers` immediately. Pre-R250 Yggdrasil merged the two and
    // gated everything, keeping initial sync single-peer until preview slot
    // ~102 M (the dominant 3.2x perf gap surfaced by the R249 side-by-side
    // soak vs reference Haskell).
    //
    // We still build `combined_snapshot` to preserve observability counts in
    // the trace, but the split eligibility takes precedence for actual peer
    // emission.
    let snapshot_overlay_for_always = snapshot_file.clone();
    let combined_snapshot = merge_ledger_peer_snapshots(ledger_snapshot, snapshot_file);
    let freshness =
        file_cfg.peer_snapshot_freshness(snapshot_slot, latest_slot, snapshot_available);

    // Live-ledger eligibility (gated by useLedgerAfterSlot).
    let (decision, live_eligible_peers) = file_cfg.eligible_ledger_fallback_peers(
        ledger_snapshot, // live-only - overlay handled separately below
        latest_slot,
        ledger_state_judgement,
        freshness,
    );

    // Snapshot-overlay eligibility (always, no gate).
    let snapshot_eligible_peers =
        file_cfg.always_eligible_snapshot_fallbacks(snapshot_overlay_for_always.as_ref());

    // Always emit snapshot peers; emit live peers only when the gate is open.
    let snapshot_eligible_count = snapshot_eligible_peers.len();
    extend_unique_peers(&mut fallback_peers, snapshot_eligible_peers);
    let live_eligible_count = if decision == LedgerPeerUseDecision::Eligible {
        let n = live_eligible_peers.len();
        extend_unique_peers(&mut fallback_peers, live_eligible_peers);
        n
    } else {
        0
    };

    tracer.trace_runtime(
        "Net.PeerSelection",
        "Info",
        "evaluated ledger-derived startup fallbacks",
        trace_fields([
            ("decision", json!(format!("{decision:?}"))),
            ("latestSlot", json!(latest_slot)),
            ("snapshotSlot", json!(snapshot_slot)),
            (
                "snapshotPath",
                json!(snapshot_path.map(|path| path.display().to_string())),
            ),
            (
                "ledgerPeerCount",
                json!(combined_snapshot.ledger_peers.len()),
            ),
            (
                "bigLedgerPeerCount",
                json!(combined_snapshot.big_ledger_peers.len()),
            ),
            (
                "eligiblePeerCount",
                json!(snapshot_eligible_count + live_eligible_count),
            ),
            ("snapshotEligibleCount", json!(snapshot_eligible_count)),
            ("liveLedgerEligibleCount", json!(live_eligible_count)),
        ]),
    );

    fallback_peers
}
