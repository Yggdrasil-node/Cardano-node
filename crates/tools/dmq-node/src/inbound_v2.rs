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
//! This slice ports the foundational standalone types;
//! `PeerTxState`, `SharedTxState`, the `TxDecision` record, and the
//! governor logic land in subsequent rounds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

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
#[derive(Clone, Debug, Eq, PartialEq)]
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
