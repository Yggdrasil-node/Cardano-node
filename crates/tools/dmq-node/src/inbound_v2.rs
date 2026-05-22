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
//! functions: `update_ref_counts`, `tick_timed_txs`,
//! `split_acknowledged_tx_ids`, `filter_active_peers`,
//! `acknowledge_tx_ids`, `pick_txs_to_download`, and the
//! `make_decisions` orchestrator. The remaining `State.hs`
//! state-mutation functions (`receivedTxIdsImpl`, `collectTxsImpl`)
//! land in subsequent rounds.

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

/// Acknowledge the longest prefix of a peer's unacknowledged txids.
///
/// Mirror of upstream `acknowledgeTxIds` (`State.hs`). Returns
/// `(tx_ids_to_acknowledge, tx_ids_to_request, txs_to_mempool,
/// ref_count_diff, updated_peer)`. Txids can only be acknowledged
/// when new ones can also be requested — a `MsgRequestTxIds` for zero
/// txids is a protocol error — so a zero request count yields the
/// no-op `(0, 0, ..)` result.
pub fn acknowledge_tx_ids<P: Ord>(
    policy: &SigDecisionPolicy,
    shared: &SharedTxState<P>,
    ps: &PeerTxState,
) -> (
    NumTxIdsToAck,
    NumTxIdsToReq,
    TxsToMempool,
    RefCountDiff,
    PeerTxState,
) {
    let (tx_ids_to_request, acknowledged_tx_ids, unacknowledged_tx_ids) =
        split_acknowledged_tx_ids(policy, shared, ps);

    // Downloaded, acknowledged txs not already buffered or queued —
    // these can now be submitted to the mempool.
    let txs_to_mempool: Vec<(SigId, Sig)> = acknowledged_tx_ids
        .iter()
        .filter(|txid| {
            !shared.buffered_txs.contains_key(*txid) && !ps.to_mempool_txs.contains_key(*txid)
        })
        .filter_map(|txid| {
            ps.downloaded_txs
                .get(txid)
                .map(|tx| (txid.clone(), tx.clone()))
        })
        .collect();

    let mut to_mempool_txs = ps.to_mempool_txs.clone();
    for (txid, tx) in &txs_to_mempool {
        to_mempool_txs.insert(txid.clone(), tx.clone());
    }

    // The still-unacknowledged txids form the "live" set.
    let live_set: BTreeSet<SigId> = unacknowledged_tx_ids.iter().cloned().collect();

    // Split downloaded txs into still-live and acknowledged.
    let mut downloaded_txs = BTreeMap::new();
    let mut acked_downloaded: BTreeMap<SigId, Sig> = BTreeMap::new();
    for (txid, tx) in &ps.downloaded_txs {
        if live_set.contains(txid) {
            downloaded_txs.insert(txid.clone(), tx.clone());
        } else {
            acked_downloaded.insert(txid.clone(), tx.clone());
        }
    }
    // Late txs: acknowledged downloads already buffered (another peer
    // delivered them first) — these count against the peer's score.
    let late_count = acked_downloaded
        .keys()
        .filter(|txid| shared.buffered_txs.contains_key(*txid))
        .count();
    let score = ps.score + late_count as f64;

    let available_tx_ids: BTreeMap<SigId, u32> = ps
        .available_tx_ids
        .iter()
        .filter(|(txid, _)| live_set.contains(*txid))
        .map(|(txid, size)| (txid.clone(), *size))
        .collect();
    let unknown_txs: BTreeSet<SigId> = ps
        .unknown_txs
        .iter()
        .filter(|txid| live_set.contains(*txid))
        .cloned()
        .collect();

    // Reference-count increments — one per occurrence of each
    // acknowledged txid.
    let mut ref_counts: BTreeMap<SigId, i64> = BTreeMap::new();
    for txid in &acknowledged_tx_ids {
        *ref_counts.entry(txid.clone()).or_insert(0) += 1;
    }
    let tx_ids_to_acknowledge = NumTxIdsToAck(acknowledged_tx_ids.len() as u16);
    let txs_to_mempool = TxsToMempool {
        list_of_txs_to_mempool: txs_to_mempool,
    };

    if tx_ids_to_request.0 > 0 {
        let updated = PeerTxState {
            unacknowledged_tx_ids,
            available_tx_ids,
            requested_tx_ids_inflight: NumTxIdsToReq(
                ps.requested_tx_ids_inflight.0 + tx_ids_to_request.0,
            ),
            requested_txs_inflight_size: ps.requested_txs_inflight_size,
            requested_txs_inflight: ps.requested_txs_inflight.clone(),
            unknown_txs,
            score,
            score_ts: ps.score_ts,
            downloaded_txs,
            to_mempool_txs,
        };
        (
            tx_ids_to_acknowledge,
            tx_ids_to_request,
            txs_to_mempool,
            RefCountDiff {
                tx_ids_to_ack: ref_counts,
            },
            updated,
        )
    } else {
        let updated = PeerTxState {
            to_mempool_txs,
            ..ps.clone()
        };
        (
            NumTxIdsToAck(0),
            NumTxIdsToReq(0),
            txs_to_mempool,
            RefCountDiff::default(),
            updated,
        )
    }
}

/// Advance the governor's timed-tx timeouts to a given time.
///
/// Mirror of upstream `tickTimedTxs` (`State.hs`). Timed entries with
/// a deadline strictly before `now` have expired: their txids'
/// reference counts are decremented (entries reaching zero are
/// dropped), and `buffered_txs` is restricted to the txids that
/// still have a live reference count. The `now` entry and later
/// entries are retained.
pub fn tick_timed_txs<P: Ord + Clone>(now: Duration, st: &SharedTxState<P>) -> SharedTxState<P> {
    // Count one reference decrement for every txid in an expired
    // (deadline strictly before `now`) timed entry.
    let mut ref_diff: BTreeMap<SigId, i64> = BTreeMap::new();
    for (_deadline, txids) in st.timed_txs.range(..now) {
        for txid in txids {
            *ref_diff.entry(txid.clone()).or_insert(0) += 1;
        }
    }
    let timed_txs: BTreeMap<Duration, Vec<SigId>> = st
        .timed_txs
        .range(now..)
        .map(|(deadline, txids)| (*deadline, txids.clone()))
        .collect();
    let reference_counts = update_ref_counts(
        &st.reference_counts,
        &RefCountDiff {
            tx_ids_to_ack: ref_diff,
        },
    );
    let buffered_txs: BTreeMap<SigId, Option<Sig>> = st
        .buffered_txs
        .iter()
        .filter(|(txid, _)| reference_counts.contains_key(*txid))
        .map(|(txid, tx)| (txid.clone(), tx.clone()))
        .collect();
    SharedTxState {
        timed_txs,
        reference_counts,
        buffered_txs,
        ..st.clone()
    }
}

/// Internal accumulator state threaded through [`pick_peer_step`] by
/// the (forthcoming) `pick_txs_to_download` fold.
///
/// Mirror of upstream `data St peeraddr txid tx` (`Decision.hs`).
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct PickTxsState {
    /// Txids in-flight, with multiplicity.
    pub st_inflight: BTreeMap<SigId, i64>,
    /// Acknowledged txids with multiplicity — used to update the
    /// shared reference counts.
    pub st_acknowledged: BTreeMap<SigId, i64>,
    /// Txs on their way to the mempool — blocks new fetch requests
    /// for them.
    pub st_in_submission_to_mempool_txs: BTreeSet<SigId>,
}

/// Pick the txs one peer should download, acknowledge its txids, and
/// advance the cross-peer [`PickTxsState`] accumulator.
///
/// Mirror of the per-peer `accumFn` of upstream `pickTxsToDownload`
/// (`Decision.hs`). Txs are picked from the peer's available txids
/// (in txid order) until the per-peer in-flight size limit is reached
/// (it may be exceeded by one tx) or a txid is already in-flight from
/// the maximum number of peers. Returns
/// `(updated_state, updated_peer, decision)`.
pub fn pick_peer_step<P: Ord>(
    policy: &SigDecisionPolicy,
    shared: &SharedTxState<P>,
    st: PickTxsState,
    peer: &PeerTxState,
) -> (PickTxsState, PeerTxState, TxDecision) {
    // Pick a prefix of the peer's available txids — skipping those
    // buffered / in-flight / unknown / in submission to the mempool —
    // until the size or multiplicity limit stops us.
    let mut size_inflight = u64::from(peer.requested_txs_inflight_size);
    let mut txs_to_request: BTreeMap<SigId, u32> = BTreeMap::new();
    for (txid, &tx_size) in &peer.available_tx_ids {
        if shared.buffered_txs.contains_key(txid)
            || peer.requested_txs_inflight.contains(txid)
            || peer.unknown_txs.contains(txid)
            || st.st_in_submission_to_mempool_txs.contains(txid)
        {
            continue;
        }
        let multiplicity = st.st_inflight.get(txid).copied().unwrap_or(0);
        if size_inflight <= policy.txs_size_inflight_per_peer
            && multiplicity < policy.tx_inflight_multiplicity as i64
        {
            size_inflight += u64::from(tx_size);
            txs_to_request.insert(txid.clone(), tx_size);
        } else {
            break;
        }
    }
    let txs_to_request_set: BTreeSet<SigId> = txs_to_request.keys().cloned().collect();

    // Record the new in-flight requests on the peer, then acknowledge.
    let mut requested_txs_inflight = peer.requested_txs_inflight.clone();
    requested_txs_inflight.extend(txs_to_request_set.iter().cloned());
    let peer_prime = PeerTxState {
        requested_txs_inflight_size: size_inflight as u32,
        requested_txs_inflight,
        ..peer.clone()
    };
    let (num_ids_to_ack, num_ids_to_req, txs_to_mempool, ref_diff, peer_dprime) =
        acknowledge_tx_ids(policy, shared, &peer_prime);

    // Thread the accumulator: acknowledged-txid multiplicities, the
    // newly in-flight txids, and the to-mempool txids.
    let mut st_acknowledged = st.st_acknowledged.clone();
    for (txid, count) in &ref_diff.tx_ids_to_ack {
        *st_acknowledged.entry(txid.clone()).or_insert(0) += count;
    }
    let mut st_inflight = st.st_inflight.clone();
    for txid in &txs_to_request_set {
        *st_inflight.entry(txid.clone()).or_insert(0) += 1;
    }
    let mut st_in_submission = st.st_in_submission_to_mempool_txs.clone();
    for (txid, _) in &txs_to_mempool.list_of_txs_to_mempool {
        st_in_submission.insert(txid.clone());
    }

    if peer_dprime.requested_tx_ids_inflight.0 > 0 {
        let decision = TxDecision {
            txd_tx_ids_to_acknowledge: num_ids_to_ack,
            txd_tx_ids_to_request: num_ids_to_req,
            txd_pipeline_tx_ids: !peer_dprime.unacknowledged_tx_ids.is_empty(),
            txd_txs_to_request: txs_to_request,
            txd_txs_to_mempool: txs_to_mempool,
        };
        (
            PickTxsState {
                st_inflight,
                st_acknowledged,
                st_in_submission_to_mempool_txs: st_in_submission,
            },
            peer_dprime,
            decision,
        )
    } else {
        // No txids to request — only txs. `st_acknowledged` is
        // unchanged from the incoming accumulator.
        let decision = TxDecision {
            txd_txs_to_request: txs_to_request,
            ..TxDecision::empty()
        };
        (
            PickTxsState {
                st_inflight,
                st_acknowledged: st.st_acknowledged,
                st_in_submission_to_mempool_txs: st_in_submission,
            },
            peer_dprime,
            decision,
        )
    }
}

/// Distribute txs to download among the given peers, advancing the
/// shared governor state.
///
/// Mirror of upstream `pickTxsToDownload` (`Decision.hs`). Peers are
/// considered in the given order; each [`pick_peer_step`] threads the
/// cross-peer accumulator. Returns the updated [`SharedTxState`] and
/// the per-peer [`TxDecision`]s, with fully-empty decisions excluded.
pub fn pick_txs_to_download<P: Ord + Clone>(
    policy: &SigDecisionPolicy,
    shared: &SharedTxState<P>,
    peers: &[(P, PeerTxState)],
) -> (SharedTxState<P>, Vec<(P, TxDecision)>) {
    let mut st = PickTxsState {
        st_inflight: shared.inflight_txs.clone(),
        st_acknowledged: BTreeMap::new(),
        st_in_submission_to_mempool_txs: shared
            .in_submission_to_mempool_txs
            .keys()
            .cloned()
            .collect(),
    };
    // `mapAccumR` — fold the peers right-to-left, threading `st`; the
    // decisions are then restored to the input order.
    let mut stepped: Vec<(P, PeerTxState, TxDecision)> = Vec::with_capacity(peers.len());
    for (addr, peer) in peers.iter().rev() {
        let (st_next, peer_updated, decision) = pick_peer_step(policy, shared, st, peer);
        st = st_next;
        stepped.push((addr.clone(), peer_updated, decision));
    }
    stepped.reverse();

    // `gn` — finalize the shared state.
    let mut peer_tx_states = shared.peer_tx_states.clone();
    for (addr, peer_updated, _) in &stepped {
        peer_tx_states.insert(addr.clone(), peer_updated.clone());
    }
    let reference_counts = update_ref_counts(
        &shared.reference_counts,
        &RefCountDiff {
            tx_ids_to_ack: st.st_acknowledged,
        },
    );
    let buffered_txs: BTreeMap<SigId, Option<Sig>> = shared
        .buffered_txs
        .iter()
        .filter(|(txid, _)| reference_counts.contains_key(*txid))
        .map(|(txid, tx)| (txid.clone(), tx.clone()))
        .collect();
    let mut in_submission_to_mempool_txs = shared.in_submission_to_mempool_txs.clone();
    for (_, _, decision) in &stepped {
        for (txid, _) in &decision.txd_txs_to_mempool.list_of_txs_to_mempool {
            *in_submission_to_mempool_txs
                .entry(txid.clone())
                .or_insert(0) += 1;
        }
    }
    let updated_shared = SharedTxState {
        peer_tx_states,
        inflight_txs: st.st_inflight,
        buffered_txs,
        reference_counts,
        in_submission_to_mempool_txs,
        ..shared.clone()
    };
    // Exclude fully-empty decisions.
    let decisions: Vec<(P, TxDecision)> = stepped
        .into_iter()
        .filter_map(|(addr, _, decision)| {
            let is_empty = decision.txd_tx_ids_to_acknowledge.0 == 0
                && decision.txd_tx_ids_to_request.0 == 0
                && decision.txd_txs_to_request.is_empty()
                && decision
                    .txd_txs_to_mempool
                    .list_of_txs_to_mempool
                    .is_empty();
            if is_empty {
                None
            } else {
                Some((addr, decision))
            }
        })
        .collect();
    (updated_shared, decisions)
}

/// A salted hash of a peer address, used as the tie-breaker in
/// [`order_by_rejections`].
fn hash_with_salt<P: std::hash::Hash>(salt: u64, peer: &P) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    salt.hash(&mut hasher);
    peer.hash(&mut hasher);
    hasher.finish()
}

/// Order peers by how useful their delivered txs have been — the
/// lower-`score` (more useful) peers first — with a salted
/// `peeraddr` hash as the tie-breaker.
///
/// Mirror of upstream `orderByRejections` (`Decision.hs`).
pub fn order_by_rejections<P: Ord + Clone + std::hash::Hash>(
    salt: u64,
    peers: &BTreeMap<P, PeerTxState>,
) -> Vec<(P, PeerTxState)> {
    let mut ordered: Vec<(P, PeerTxState)> =
        peers.iter().map(|(a, p)| (a.clone(), p.clone())).collect();
    ordered.sort_by(|(a_addr, a_ps), (b_addr, b_ps)| {
        a_ps.score
            .total_cmp(&b_ps.score)
            .then_with(|| hash_with_salt(salt, a_addr).cmp(&hash_with_salt(salt, b_addr)))
    });
    ordered
}

/// Make download decisions for a set of active peers.
///
/// Mirror of upstream `makeDecisions` (`Decision.hs`): draws a salt
/// from the governor PRNG (advancing it), orders the peers by
/// usefulness via [`order_by_rejections`], runs
/// [`pick_txs_to_download`], and collects the per-peer decisions into
/// a map. The `peer_rng` step is a yggdrasil-side splitmix increment
/// — not byte-identical to upstream's `StdGen`; the salt only
/// randomises tie-breaking and has no wire effect.
pub fn make_decisions<P: Ord + Clone + std::hash::Hash>(
    policy: &SigDecisionPolicy,
    st: &SharedTxState<P>,
    peers: &BTreeMap<P, PeerTxState>,
) -> (SharedTxState<P>, BTreeMap<P, TxDecision>) {
    let salt = st.peer_rng;
    let next_rng = st.peer_rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let st_with_rng = SharedTxState {
        peer_rng: next_rng,
        ..st.clone()
    };
    let ordered = order_by_rejections(salt, peers);
    let (updated_shared, decisions) = pick_txs_to_download(policy, &st_with_rng, &ordered);
    (updated_shared, decisions.into_iter().collect())
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
    fn acknowledge_tx_ids_acks_the_known_prefix() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let a = SigId(SigHash(vec![0xa1]));
        let b = SigId(SigHash(vec![0xb2]));
        let c = SigId(SigHash(vec![0xc3]));
        let mut peer = PeerTxState::default();
        peer.unacknowledged_tx_ids.push_back(a.clone());
        peer.unacknowledged_tx_ids.push_back(b.clone());
        peer.unacknowledged_tx_ids.push_back(c.clone());
        // a and b are unknown-to-the-peer (acknowledgeable); c is not.
        peer.unknown_txs.insert(a.clone());
        peer.unknown_txs.insert(b.clone());
        let shared: SharedTxState<String> = SharedTxState::default();
        let (ack, req, to_mempool, ref_diff, updated) =
            acknowledge_tx_ids(&sig_decision_policy(), &shared, &peer);
        assert_eq!(ack, NumTxIdsToAck(2));
        assert!(req.0 > 0);
        assert!(to_mempool.list_of_txs_to_mempool.is_empty());
        // ref-count diff has one increment for each acknowledged txid.
        assert_eq!(ref_diff.tx_ids_to_ack.get(&a), Some(&1));
        assert_eq!(ref_diff.tx_ids_to_ack.get(&b), Some(&1));
        // the updated peer keeps only the still-unacknowledged `c`.
        assert_eq!(updated.unacknowledged_tx_ids, VecDeque::from(vec![c]));
        // `unknown_txs` is restricted to the live set (now empty).
        assert!(updated.unknown_txs.is_empty());
        assert_eq!(updated.requested_tx_ids_inflight, NumTxIdsToReq(req.0));
    }

    #[test]
    fn tick_timed_txs_expires_past_deadlines() {
        use crate::protocol::sig_submission::{SigHash, SigId};
        let old = SigId(SigHash(vec![0x01]));
        let fresh = SigId(SigHash(vec![0x02]));
        let mut st: SharedTxState<String> = SharedTxState::default();
        // `old` times out at t=5s; `fresh` at t=20s.
        st.timed_txs
            .insert(Duration::from_secs(5), vec![old.clone()]);
        st.timed_txs
            .insert(Duration::from_secs(20), vec![fresh.clone()]);
        st.reference_counts.insert(old.clone(), 1);
        st.reference_counts.insert(fresh.clone(), 1);
        st.buffered_txs.insert(old.clone(), None);
        st.buffered_txs.insert(fresh.clone(), None);
        // Tick to t=10s — `old`'s 5 s deadline has passed.
        let ticked = tick_timed_txs(Duration::from_secs(10), &st);
        // `old` expired: ref count 1 - 1 = 0 -> dropped; buffered dropped.
        assert!(!ticked.reference_counts.contains_key(&old));
        assert!(!ticked.buffered_txs.contains_key(&old));
        assert!(!ticked.timed_txs.values().flatten().any(|t| *t == old));
        // `fresh` survives — its deadline is still in the future.
        assert_eq!(ticked.reference_counts.get(&fresh), Some(&1));
        assert!(ticked.buffered_txs.contains_key(&fresh));
    }

    #[test]
    fn pick_peer_step_picks_available_txs_under_the_limits() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let a = SigId(SigHash(vec![0xa1]));
        let b = SigId(SigHash(vec![0xb2]));
        let peer = PeerTxState {
            available_tx_ids: BTreeMap::from([(a.clone(), 100u32), (b.clone(), 200u32)]),
            ..Default::default()
        };
        let shared: SharedTxState<String> = SharedTxState::default();
        let (st, _peer, decision) = pick_peer_step(
            &sig_decision_policy(),
            &shared,
            PickTxsState::default(),
            &peer,
        );
        // Both available txs are picked into the request map.
        assert_eq!(decision.txd_txs_to_request.get(&a), Some(&100));
        assert_eq!(decision.txd_txs_to_request.get(&b), Some(&200));
        // The accumulator records them as newly in-flight.
        assert_eq!(st.st_inflight.get(&a), Some(&1));
        assert_eq!(st.st_inflight.get(&b), Some(&1));
    }

    #[test]
    fn pick_peer_step_skips_buffered_and_inflight_txs() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let buffered = SigId(SigHash(vec![0x01]));
        let inflight = SigId(SigHash(vec![0x02]));
        let pickable = SigId(SigHash(vec![0x03]));
        let peer = PeerTxState {
            available_tx_ids: BTreeMap::from([
                (buffered.clone(), 50u32),
                (inflight.clone(), 50u32),
                (pickable.clone(), 50u32),
            ]),
            requested_txs_inflight: BTreeSet::from([inflight.clone()]),
            ..Default::default()
        };
        let mut shared: SharedTxState<String> = SharedTxState::default();
        shared.buffered_txs.insert(buffered.clone(), None);
        let (_st, _peer, decision) = pick_peer_step(
            &sig_decision_policy(),
            &shared,
            PickTxsState::default(),
            &peer,
        );
        // Only the un-excluded txid is requested.
        assert_eq!(decision.txd_txs_to_request.len(), 1);
        assert!(decision.txd_txs_to_request.contains_key(&pickable));
    }

    #[test]
    fn pick_txs_to_download_decides_per_peer_and_drops_empty() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let x = SigId(SigHash(vec![0x11]));
        // One peer has a tx available; one peer has nothing.
        let busy = PeerTxState {
            available_tx_ids: BTreeMap::from([(x.clone(), 300u32)]),
            ..Default::default()
        };
        // The idle peer already has the maximum txid requests in
        // flight (33 = MAX_SIGS_INFLIGHT), so it has no capacity to
        // request more and nothing available to download.
        let idle = PeerTxState {
            requested_tx_ids_inflight: NumTxIdsToReq(33),
            ..Default::default()
        };
        let shared: SharedTxState<String> = SharedTxState::default();
        let peers = vec![("busy".to_string(), busy), ("idle".to_string(), idle)];
        let (updated, decisions) = pick_txs_to_download(&sig_decision_policy(), &shared, &peers);
        // The idle peer's fully-empty decision is excluded.
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].0, "busy");
        assert!(decisions[0].1.txd_txs_to_request.contains_key(&x));
        // The shared state retains both peers and records the request.
        assert_eq!(updated.peer_tx_states.len(), 2);
        assert_eq!(updated.inflight_txs.get(&x), Some(&1));
    }

    #[test]
    fn order_by_rejections_sorts_lower_score_first() {
        let high = PeerTxState {
            score: 9.0,
            ..Default::default()
        };
        let low = PeerTxState {
            score: 1.0,
            ..Default::default()
        };
        let peers = BTreeMap::from([("high".to_string(), high), ("low".to_string(), low)]);
        let ordered = order_by_rejections(42, &peers);
        assert_eq!(ordered[0].0, "low");
        assert_eq!(ordered[1].0, "high");
    }

    #[test]
    fn make_decisions_advances_rng_and_decides() {
        use crate::policy::sig_decision_policy;
        use crate::protocol::sig_submission::{SigHash, SigId};
        let x = SigId(SigHash(vec![0x42]));
        let peer = PeerTxState {
            available_tx_ids: BTreeMap::from([(x.clone(), 250u32)]),
            ..Default::default()
        };
        let st: SharedTxState<String> = SharedTxState::default();
        let peers = BTreeMap::from([("p".to_string(), peer)]);
        let (updated, decisions) = make_decisions(&sig_decision_policy(), &st, &peers);
        // The peer-ordering PRNG advanced.
        assert_ne!(updated.peer_rng, st.peer_rng);
        // The peer received a decision requesting its available tx.
        assert!(decisions.contains_key("p"));
        assert!(decisions["p"].txd_txs_to_request.contains_key(&x));
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
