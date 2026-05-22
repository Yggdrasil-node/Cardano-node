//! dmq-node TxSubmission inbound-V2 governor types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of upstream
//! `Ouroboros.Network.TxSubmission.Inbound.V2.Types` — the inbound
//! tx-submission governor's state and decision types. The DMQ
//! `NodeKernel` (`Diffusion/NodeKernel.hs`) holds the inbound-V2
//! `SharedTxState` for `SigSubmission` (`= TxSubmission2 SigId Sig`).
//! dmq-node carries its own copy: the R732 dmq-node-local decision —
//! `crates/consensus`'s tx-submission inbound governor is concrete
//! over ledger transactions, so it cannot be reused for `SigId` /
//! `Sig`.
//!
//! Ports the inbound-V2 state surface — the foundational types,
//! `TxDecision`, `PeerTxState`, `SharedTxState` — and the governor
//! functions incrementally (`update_ref_counts`,
//! `split_acknowledged_tx_ids` so far). The remaining `Decision.hs` /
//! `State.hs` governor functions (`acknowledgeTxIds`,
//! `makeDecisions`, ...) land in subsequent rounds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use crate::policy::SigDecisionPolicy;
use crate::protocol::sig_submission::{Sig, SigId};

/// Which tx-submission inbound logic a peer connection uses.
///
/// Mirror of upstream `data TxSubmissionLogicVersion`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TxSubmissionLogicVersion {
    /// The legacy `Inbound.V1` logic.
    TxSubmissionLogicV1,
    /// The new `Inbound.V2` logic.
    TxSubmissionLogicV2,
}

impl TxSubmissionLogicVersion {
    /// Every version, low to high — upstream's `[minBound .. maxBound]`.
    pub const ALL: [TxSubmissionLogicVersion; 2] = [
        TxSubmissionLogicVersion::TxSubmissionLogicV1,
        TxSubmissionLogicVersion::TxSubmissionLogicV2,
    ];
}

/// A count of transactions processed in one governor step — how many
/// were accepted, how many rejected, and the resulting peer score.
///
/// Mirror of upstream `data ProcessedTxCount`. `PartialEq` only —
/// upstream derives `Eq` but `ptxc_score` is `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessedTxCount {
    /// How many transactions were just accepted.
    pub ptxc_accepted: i64,
    /// How many transactions were just rejected.
    pub ptxc_rejected: i64,
    /// The peer's resulting score.
    pub ptxc_score: f64,
}

/// An optional delay before tx-submission starts on a connection.
///
/// Mirror of upstream `data TxSubmissionInitDelay`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxSubmissionInitDelay {
    /// Delay tx-submission start by this duration.
    TxSubmissionInitDelay(Duration),
    /// Start tx-submission with no delay.
    NoTxSubmissionInitDelay,
}

/// The default tx-submission init delay — 60 seconds.
///
/// Mirror of upstream `defaultTxSubmissionInitDelay`.
pub const DEFAULT_TX_SUBMISSION_INIT_DELAY: TxSubmissionInitDelay =
    TxSubmissionInitDelay::TxSubmissionInitDelay(Duration::from_secs(60));

/// Number of transaction ids being acknowledged.
///
/// Mirror of upstream `newtype NumTxIdsToAck = NumTxIdsToAck Word16`
/// (`Protocol/TxSubmission2/Type`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumTxIdsToAck(pub u16);

/// Number of transaction ids being requested.
///
/// Mirror of upstream `newtype NumTxIdsToReq = NumTxIdsToReq Word16`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NumTxIdsToReq(pub u16);

/// Transactions ready to be submitted to the mempool.
///
/// Mirror of upstream `newtype TxsToMempool txid tx` — concrete over
/// the DMQ `SigId` / `Sig`. Upstream's `Semigroup` / `Monoid`
/// instances are list concatenation; `Default` is the empty list.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct TxsToMempool {
    /// The `(id, tx)` pairs to submit, in order.
    pub list_of_txs_to_mempool: Vec<(SigId, Sig)>,
}

/// A decision made by the inbound tx-submission governor for one peer.
///
/// Mirror of upstream `data TxDecision txid tx`, concrete over the DMQ
/// `SigId` / `Sig`. Upstream notes the unusual product (rather than
/// sum) shape: a peer downloads `tx`s and then requests more `txid`s
/// in the same decision, which keeps the peer non-active for longer
/// and spares the `makeDecision` computation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TxDecision {
    /// Transaction ids to acknowledge.
    pub txd_tx_ids_to_acknowledge: NumTxIdsToAck,
    /// Number of transaction ids to request.
    pub txd_tx_ids_to_request: NumTxIdsToReq,
    /// Whether to pipeline the txid request — only allowed when there
    /// are non-acknowledged txids.
    pub txd_pipeline_tx_ids: bool,
    /// The transaction ids to download, with their serialized sizes.
    pub txd_txs_to_request: BTreeMap<SigId, u32>,
    /// The transactions to submit to the mempool.
    pub txd_txs_to_mempool: TxsToMempool,
}

impl TxDecision {
    /// The empty decision — acknowledge nothing, request nothing.
    ///
    /// Mirror of upstream `emptyTxDecision`.
    pub fn empty() -> TxDecision {
        TxDecision::default()
    }
}

/// The inbound tx-submission governor's per-peer state.
///
/// Mirror of upstream `data PeerTxState txid tx`, concrete over the
/// DMQ `SigId` / `Sig`. `PartialEq` only — upstream derives `Eq` but
/// `score` is `f64`. `Default` is the empty state of a fresh peer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeerTxState {
    /// Txids the peer told us about and which we have not yet
    /// acknowledged, in the order the peer gave them — the same order
    /// we submit to the mempool and acknowledge in.
    pub unacknowledged_tx_ids: VecDeque<SigId>,
    /// Known txids requestable from this peer, with their sizes.
    pub available_tx_ids: BTreeMap<SigId, u32>,
    /// Count of txids requested but not yet replied to — tracked to
    /// keep requests within the unacknowledged-txid limit.
    pub requested_tx_ids_inflight: NumTxIdsToReq,
    /// Total size of txs requested but not yet replied to.
    pub requested_txs_inflight_size: u32,
    /// The set of requested txids.
    pub requested_txs_inflight: BTreeSet<SigId>,
    /// A subset of `unacknowledged_tx_ids` the peer did not know
    /// (requested but not received) — tracked per peer so they still
    /// get acknowledged.
    pub unknown_txs: BTreeSet<SigId>,
    /// Peer-usefulness metric — larger is less useful; it decays
    /// towards zero over time.
    pub score: f64,
    /// The time `score` was last drained — mirror of upstream `Time`
    /// (a duration since the monotonic origin).
    pub score_ts: Duration,
    /// Txs downloaded from the peer, not yet acknowledged or sent to
    /// the mempool.
    pub downloaded_txs: BTreeMap<SigId, Sig>,
    /// Txs on their way to the mempool — tracked so they can be
    /// cleaned up if the peer dies.
    pub to_mempool_txs: BTreeMap<SigId, Sig>,
}

/// The inbound tx-submission governor's state, shared across all
/// peers.
///
/// Mirror of upstream `data SharedTxState peeraddr txid tx`, concrete
/// over the DMQ `SigId` / `Sig` and generic over the peer-address
/// key. `PartialEq` only — it holds [`PeerTxState`], which is
/// `PartialEq`-only via its `f64` score.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedTxState<PeerAddr: Ord> {
    /// Per-peer governor state. Invariant: every peer registered via
    /// `withPeer` has an entry, even with an empty txid set.
    pub peer_tx_states: BTreeMap<PeerAddr, PeerTxState>,
    /// In-flight (already-requested) txids, each with its
    /// multiplicity — the number of peers it is currently in-flight
    /// from.
    pub inflight_txs: BTreeMap<SigId, i64>,
    /// Downloaded txs: `Some(tx)` once downloaded, `None` once it is
    /// already in the mempool. Only live txids are kept.
    pub buffered_txs: BTreeMap<SigId, Option<Sig>>,
    /// Reference counts of every unacknowledged / timed txid — a txid
    /// is dropped from `buffered_txs` when its count reaches zero.
    pub reference_counts: BTreeMap<SigId, i64>,
    /// Short timeouts for txids buffered after mempool insertion,
    /// avoiding immediate re-download. Keyed by deadline (upstream
    /// `Time`, modelled as a duration since the monotonic origin).
    pub timed_txs: BTreeMap<Duration, Vec<SigId>>,
    /// Txids downloaded and on their way to the mempool, with a
    /// counter — no further fetch requests are issued while in this
    /// state.
    pub in_submission_to_mempool_txs: BTreeMap<SigId, i64>,
    /// PRNG state used to randomly order peers. Stands in for
    /// upstream's `StdGen`; the peer-ordering RNG is governor logic
    /// landing in a later round, so it is modelled here as the
    /// seed/state value to keep `SharedTxState` a plain comparable
    /// data structure.
    pub peer_rng: u64,
}

impl<PeerAddr: Ord> Default for SharedTxState<PeerAddr> {
    fn default() -> Self {
        SharedTxState {
            peer_tx_states: BTreeMap::new(),
            inflight_txs: BTreeMap::new(),
            buffered_txs: BTreeMap::new(),
            reference_counts: BTreeMap::new(),
            timed_txs: BTreeMap::new(),
            in_submission_to_mempool_txs: BTreeMap::new(),
            peer_rng: 0,
        }
    }
}

/// A set of reference-count decrements to apply to a
/// [`SharedTxState`]'s `reference_counts`.
///
/// Mirror of upstream `newtype RefCountDiff txid`.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct RefCountDiff {
    /// The per-txid decrement amounts.
    pub tx_ids_to_ack: BTreeMap<SigId, i64>,
}

/// Apply a [`RefCountDiff`] to a reference-count map.
///
/// Mirror of upstream `updateRefCounts`: each entry of
/// `reference_counts` is decremented by the matching `diff` amount;
/// an entry whose count reaches zero is removed; entries absent from
/// `diff` are carried through unchanged; entries present only in
/// `diff` are ignored.
pub fn update_ref_counts(
    reference_counts: &BTreeMap<SigId, i64>,
    diff: &RefCountDiff,
) -> BTreeMap<SigId, i64> {
    let mut result = BTreeMap::new();
    for (txid, &x) in reference_counts {
        match diff.tx_ids_to_ack.get(txid) {
            Some(&y) => {
                debug_assert!(x >= y, "updateRefCounts: reference-count underflow");
                if x > y {
                    result.insert(txid.clone(), x - y);
                }
                // x == y → the count reached zero, drop the entry.
            }
            None => {
                result.insert(txid.clone(), x);
            }
        }
    }
    result
}

/// Split a peer's unacknowledged txids into the longest
/// acknowledgeable prefix and the txids still unacknowledged, and
/// compute how many new txids to request.
///
/// Mirror of upstream `splitAcknowledgedTxIds` (`State.hs`). A txid
/// is acknowledgeable when it is not in-flight and is either
/// downloaded, unknown to the peer, or already buffered. Returns
/// `(num_to_request, acknowledged, still_unacknowledged)`.
pub fn split_acknowledged_tx_ids<P: Ord>(
    policy: &SigDecisionPolicy,
    shared: &SharedTxState<P>,
    peer: &PeerTxState,
) -> (NumTxIdsToReq, VecDeque<SigId>, VecDeque<SigId>) {
    let split = peer
        .unacknowledged_tx_ids
        .iter()
        .position(|txid| {
            !(!peer.requested_txs_inflight.contains(txid)
                && (peer.downloaded_txs.contains_key(txid)
                    || peer.unknown_txs.contains(txid)
                    || shared.buffered_txs.contains_key(txid)))
        })
        .unwrap_or(peer.unacknowledged_tx_ids.len());
    let acknowledged: VecDeque<SigId> = peer
        .unacknowledged_tx_ids
        .iter()
        .take(split)
        .cloned()
        .collect();
    let still_unacknowledged: VecDeque<SigId> = peer
        .unacknowledged_tx_ids
        .iter()
        .skip(split)
        .cloned()
        .collect();

    let num_unacked = peer.unacknowledged_tx_ids.len() as i64;
    let num_acked = split as i64;
    let requested_inflight = i64::from(peer.requested_tx_ids_inflight.0);
    let unacked_and_requested = num_unacked + requested_inflight;
    let max_unacked = policy.max_unacknowledged_tx_ids as i64;
    let max_req = policy.max_num_tx_ids_to_request as i64;
    debug_assert!(
        unacked_and_requested <= max_unacked,
        "splitAcknowledgedTxIds: unacked + requested over the limit"
    );
    debug_assert!(
        requested_inflight <= max_req,
        "splitAcknowledgedTxIds: requested over the limit"
    );
    let num_to_request =
        (max_unacked - unacked_and_requested + num_acked).min(max_req - requested_inflight);
    (
        NumTxIdsToReq(num_to_request as u16),
        acknowledged,
        still_unacknowledged,
    )
}

/// Filter the governor's peers to those that can currently either
/// download a `tx` or acknowledge `txid`s.
///
/// Mirror of upstream `filterActivePeers` (`Decision.hs`). A peer is
/// active when it can request more txids (no txid request in flight,
/// the unacknowledged count is under the limit, and there is request
/// capacity) or it can download a tx (under the per-peer in-flight
/// size limit and with at least one requestable available txid).
pub fn filter_active_peers<P: Ord + Clone>(
    policy: &SigDecisionPolicy,
    shared: &SharedTxState<P>,
) -> BTreeMap<P, PeerTxState> {
    // Txids that cannot be requested: already in-flight from at least
    // `tx_inflight_multiplicity` peers, or already buffered.
    let mut unrequestable: BTreeSet<SigId> = shared
        .inflight_txs
        .iter()
        .filter(|(_, count)| **count >= policy.tx_inflight_multiplicity as i64)
        .map(|(id, _)| id.clone())
        .collect();
    unrequestable.extend(shared.buffered_txs.keys().cloned());

    shared
        .peer_tx_states
        .iter()
        .filter(|(_, peer)| {
            let num_of_unacked = peer.unacknowledged_tx_ids.len() as i64;
            let requested_inflight = i64::from(peer.requested_tx_ids_inflight.0);
            let (tx_ids_to_request, _, _) = split_acknowledged_tx_ids(policy, shared, peer);
            let can_request_ids = requested_inflight == 0
                && requested_inflight + num_of_unacked <= policy.max_unacknowledged_tx_ids as i64
                && tx_ids_to_request.0 > 0;
            let under_size_limit =
                u64::from(peer.requested_txs_inflight_size) <= policy.txs_size_inflight_per_peer;
            let has_downloadable = peer.available_tx_ids.keys().any(|id| {
                !peer.requested_txs_inflight.contains(id)
                    && !peer.unknown_txs.contains(id)
                    && !unrequestable.contains(id)
                    && !shared.in_submission_to_mempool_txs.contains_key(id)
            });
            can_request_ids || (under_size_limit && has_downloadable)
        })
        .map(|(addr, peer)| (addr.clone(), peer.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logic_version_all_is_ordered() {
        assert_eq!(
            TxSubmissionLogicVersion::ALL,
            [
                TxSubmissionLogicVersion::TxSubmissionLogicV1,
                TxSubmissionLogicVersion::TxSubmissionLogicV2,
            ]
        );
        assert!(
            TxSubmissionLogicVersion::TxSubmissionLogicV1
                < TxSubmissionLogicVersion::TxSubmissionLogicV2
        );
    }

    #[test]
    fn processed_tx_count_construct() {
        let c = ProcessedTxCount {
            ptxc_accepted: 7,
            ptxc_rejected: 2,
            ptxc_score: 0.5,
        };
        assert_eq!(c.ptxc_accepted, 7);
        assert_eq!(c.ptxc_rejected, 2);
        assert_eq!(c.ptxc_score, 0.5);
    }

    #[test]
    fn tx_decision_construct_and_compare() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let mut to_request = BTreeMap::new();
        to_request.insert(SigId(SigHash(vec![0x01])), 2800u32);
        let decision = TxDecision {
            txd_tx_ids_to_acknowledge: NumTxIdsToAck(3),
            txd_tx_ids_to_request: NumTxIdsToReq(5),
            txd_pipeline_tx_ids: true,
            txd_txs_to_request: to_request,
            txd_txs_to_mempool: TxsToMempool::default(),
        };
        assert_eq!(decision.txd_tx_ids_to_acknowledge, NumTxIdsToAck(3));
        assert_eq!(decision.txd_txs_to_request.len(), 1);
        assert!(decision.txd_pipeline_tx_ids);
        assert!(
            decision
                .txd_txs_to_mempool
                .list_of_txs_to_mempool
                .is_empty()
        );
        assert_eq!(decision.clone(), decision);
    }

    #[test]
    fn count_newtypes_wrap_word16() {
        assert_eq!(NumTxIdsToAck(7).0, 7);
        assert_eq!(NumTxIdsToReq::default(), NumTxIdsToReq(0));
    }

    #[test]
    fn peer_tx_state_default_is_empty() {
        let p = PeerTxState::default();
        assert!(p.unacknowledged_tx_ids.is_empty());
        assert!(p.available_tx_ids.is_empty());
        assert_eq!(p.requested_tx_ids_inflight, NumTxIdsToReq(0));
        assert_eq!(p.requested_txs_inflight_size, 0);
        assert_eq!(p.score, 0.0);
    }

    #[test]
    fn peer_tx_state_tracks_offered_and_inflight() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let id = SigId(SigHash(vec![0xAB]));
        let mut p = PeerTxState::default();
        p.unacknowledged_tx_ids.push_back(id.clone());
        p.available_tx_ids.insert(id.clone(), 1500);
        p.requested_txs_inflight.insert(id.clone());
        p.requested_tx_ids_inflight = NumTxIdsToReq(1);
        assert_eq!(p.unacknowledged_tx_ids.len(), 1);
        assert_eq!(p.available_tx_ids.get(&id), Some(&1500));
        assert!(p.requested_txs_inflight.contains(&id));
    }

    #[test]
    fn empty_tx_decision_is_all_zero() {
        let d = TxDecision::empty();
        assert_eq!(d.txd_tx_ids_to_acknowledge, NumTxIdsToAck(0));
        assert_eq!(d.txd_tx_ids_to_request, NumTxIdsToReq(0));
        assert!(!d.txd_pipeline_tx_ids);
        assert!(d.txd_txs_to_request.is_empty());
        assert!(d.txd_txs_to_mempool.list_of_txs_to_mempool.is_empty());
        assert_eq!(d, TxDecision::default());
    }

    #[test]
    fn update_ref_counts_decrements_carries_and_drops() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let a = SigId(SigHash(vec![0x0a]));
        let b = SigId(SigHash(vec![0x0b]));
        let c = SigId(SigHash(vec![0x0c]));
        let mut counts = BTreeMap::new();
        counts.insert(a.clone(), 3);
        counts.insert(b.clone(), 2);
        counts.insert(c.clone(), 5);
        let mut diff = RefCountDiff::default();
        diff.tx_ids_to_ack.insert(a.clone(), 1); // 3 - 1 = 2 (kept)
        diff.tx_ids_to_ack.insert(b.clone(), 2); // 2 - 2 = 0 (dropped)
        // c is absent from diff -> carried through unchanged.
        let updated = update_ref_counts(&counts, &diff);
        assert_eq!(updated.get(&a), Some(&2));
        assert_eq!(updated.get(&b), None);
        assert_eq!(updated.get(&c), Some(&5));
    }

    #[test]
    fn split_acknowledged_tx_ids_takes_the_known_prefix() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let a = SigId(SigHash(vec![0x0a]));
        let b = SigId(SigHash(vec![0x0b]));
        let c = SigId(SigHash(vec![0x0c]));
        let mut peer = PeerTxState::default();
        peer.unacknowledged_tx_ids.push_back(a.clone());
        peer.unacknowledged_tx_ids.push_back(b.clone());
        peer.unacknowledged_tx_ids.push_back(c.clone());
        // a and b are unknown-to-the-peer (acknowledgeable); c is not.
        peer.unknown_txs.insert(a.clone());
        peer.unknown_txs.insert(b.clone());
        let shared: SharedTxState<String> = SharedTxState::default();
        let (num_to_request, acked, unacked) =
            split_acknowledged_tx_ids(&sig_decision_policy(), &shared, &peer);
        assert_eq!(acked, VecDeque::from(vec![a, b]));
        assert_eq!(unacked, VecDeque::from(vec![c]));
        // Some capacity to request more is available.
        assert!(num_to_request.0 > 0);
    }

    #[test]
    fn split_acknowledged_tx_ids_stops_at_an_inflight_txid() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let a = SigId(SigHash(vec![0x01]));
        let mut peer = PeerTxState::default();
        peer.unacknowledged_tx_ids.push_back(a.clone());
        peer.unknown_txs.insert(a.clone());
        // `a` is still in-flight, so it cannot be acknowledged.
        peer.requested_txs_inflight.insert(a.clone());
        let shared: SharedTxState<String> = SharedTxState::default();
        let (_, acked, unacked) = split_acknowledged_tx_ids(&sig_decision_policy(), &shared, &peer);
        assert!(acked.is_empty());
        assert_eq!(unacked, VecDeque::from(vec![a]));
    }

    #[test]
    fn filter_active_peers_keeps_only_the_active_ones() {
        use crate::policy::sig_decision_policy;
        let mut shared: SharedTxState<String> = SharedTxState::default();
        // A fresh peer with no requests in flight can request txids —
        // it is active.
        shared
            .peer_tx_states
            .insert("active".to_string(), PeerTxState::default());
        // An idle peer with a txid request already in flight and
        // nothing available cannot do anything — it is inactive.
        let idle = PeerTxState {
            requested_tx_ids_inflight: NumTxIdsToReq(5),
            ..Default::default()
        };
        shared.peer_tx_states.insert("idle".to_string(), idle);
        let active = filter_active_peers(&sig_decision_policy(), &shared);
        assert_eq!(active.len(), 1);
        assert!(active.contains_key("active"));
        assert!(!active.contains_key("idle"));
    }

    #[test]
    fn shared_tx_state_default_is_empty() {
        let s: SharedTxState<String> = SharedTxState::default();
        assert!(s.peer_tx_states.is_empty());
        assert!(s.inflight_txs.is_empty());
        assert!(s.buffered_txs.is_empty());
        assert_eq!(s.peer_rng, 0);
    }

    #[test]
    fn shared_tx_state_registers_a_peer_and_buffers_txs() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let id = SigId(SigHash(vec![0x07]));
        let mut s: SharedTxState<String> = SharedTxState::default();
        s.peer_tx_states
            .insert("peer-a".to_string(), PeerTxState::default());
        s.inflight_txs.insert(id.clone(), 2);
        s.buffered_txs.insert(id.clone(), None);
        s.reference_counts.insert(id.clone(), 1);
        assert_eq!(s.peer_tx_states.len(), 1);
        assert_eq!(s.inflight_txs.get(&id), Some(&2));
        assert_eq!(s.buffered_txs.get(&id), Some(&None));
    }

    #[test]
    fn default_init_delay_is_sixty_seconds() {
        assert_eq!(
            DEFAULT_TX_SUBMISSION_INIT_DELAY,
            TxSubmissionInitDelay::TxSubmissionInitDelay(Duration::from_secs(60))
        );
        assert_ne!(
            DEFAULT_TX_SUBMISSION_INIT_DELAY,
            TxSubmissionInitDelay::NoTxSubmissionInitDelay
        );
    }
}
