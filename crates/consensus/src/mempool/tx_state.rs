//! Cross-peer shared transaction state for TxId deduplication.
//!
//! When multiple peers advertise the same transaction, only one download is
//! needed.  [`SharedTxState`] tracks which TxIds are currently in flight
//! (being fetched from a specific peer), which have already been delivered to
//! the mempool, and which were recently confirmed in a block.  The inbound
//! TxSubmission server consults this state before requesting transactions
//! so that the same TxId is never fetched from two peers simultaneously.
//!
//! Reference: `Ouroboros.Network.TxSubmission.Inbound.V2.State` —
//! `SharedTxState`, `PeerTxState`, `TxDecision`.

/// Maximum number of recently-known TxIds retained in the `known` ring.
///
/// Once exceeded, the oldest entries are evicted.  This prevents unbounded
/// memory growth as the node processes blocks over time.
pub(super) const DEFAULT_KNOWN_CAPACITY: usize = 16_384;

/// Advertised body size, in bytes, of a transaction.
///
/// Mirrors upstream `SizeInBytes` from
/// `Ouroboros.Network.TxSubmission.Inbound.V2.State`.
pub type SizeInBytes = u32;

pub mod shared;
pub mod state;

pub use shared::SharedTxState;
pub use state::{FilterOutcome, PeerTxState, TxState};

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use yggdrasil_ledger::TxId;

    fn txid(b: u8) -> TxId {
        TxId([b; 32])
    }

    fn peer(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn filter_new_txids_returns_all_when_empty() {
        let mut state = TxState::default();
        let p = peer(1000);
        state.register_peer(p);

        let outcome = state.filter_advertised(&p, &[txid(1), txid(2)]);
        assert_eq!(outcome.to_fetch.len(), 2);
        assert!(outcome.already_known.is_empty());
    }

    #[test]
    fn filter_skips_known_txids() {
        let mut state = TxState::default();
        let p = peer(1000);
        state.register_peer(p);

        state.mark_confirmed(&[txid(1)]);
        let outcome = state.filter_advertised(&p, &[txid(1), txid(2)]);
        assert_eq!(outcome.to_fetch, vec![txid(2)]);
        assert_eq!(outcome.already_known, vec![txid(1)]);
    }

    #[test]
    fn filter_skips_in_flight_txids() {
        let mut state = TxState::default();
        let p1 = peer(1000);
        let p2 = peer(2000);
        state.register_peer(p1);
        state.register_peer(p2);

        // Peer 1 advertises and starts fetching tx 1.
        let _ = state.filter_advertised(&p1, &[txid(1)]);
        state.mark_in_flight(&p1, &[txid(1)]);

        // Peer 2 advertises the same tx — should be flagged as already known.
        let outcome = state.filter_advertised(&p2, &[txid(1), txid(3)]);
        assert_eq!(outcome.to_fetch, vec![txid(3)]);
        assert_eq!(outcome.already_known, vec![txid(1)]);
    }

    #[test]
    fn mark_received_moves_to_known() {
        let mut state = TxState::default();
        let p = peer(1000);
        state.register_peer(p);

        let _ = state.filter_advertised(&p, &[txid(1)]);
        state.mark_in_flight(&p, &[txid(1)]);
        assert!(state.is_in_flight(&txid(1)));

        state.mark_received(&p, &[txid(1)]);
        assert!(!state.is_in_flight(&txid(1)));
        assert!(state.is_known(&txid(1)));
    }

    #[test]
    fn mark_not_found_frees_for_another_peer() {
        let mut state = TxState::default();
        let p1 = peer(1000);
        let p2 = peer(2000);
        state.register_peer(p1);
        state.register_peer(p2);

        let _ = state.filter_advertised(&p1, &[txid(1)]);
        state.mark_in_flight(&p1, &[txid(1)]);
        state.mark_not_found(&p1, &[txid(1)]);

        // Now peer 2 should be able to fetch it.
        let outcome = state.filter_advertised(&p2, &[txid(1)]);
        assert_eq!(outcome.to_fetch, vec![txid(1)]);
    }

    #[test]
    fn unregister_peer_cancels_in_flight() {
        let mut state = TxState::default();
        let p = peer(1000);
        state.register_peer(p);

        let _ = state.filter_advertised(&p, &[txid(1)]);
        state.mark_in_flight(&p, &[txid(1)]);
        assert!(state.is_in_flight(&txid(1)));

        state.unregister_peer(&p);
        assert!(!state.is_in_flight(&txid(1)));
        assert_eq!(state.peer_count(), 0);
    }

    #[test]
    fn known_ring_evicts_oldest() {
        let mut state = TxState::new(3);
        state.mark_confirmed(&[txid(1), txid(2), txid(3)]);
        assert_eq!(state.known_count(), 3);

        // Adding a 4th should evict txid(1).
        state.mark_confirmed(&[txid(4)]);
        assert_eq!(state.known_count(), 3);
        assert!(!state.is_known(&txid(1)));
        assert!(state.is_known(&txid(2)));
        assert!(state.is_known(&txid(3)));
        assert!(state.is_known(&txid(4)));
    }

    #[test]
    fn mark_confirmed_cleans_peer_state() {
        let mut state = TxState::default();
        let p = peer(1000);
        state.register_peer(p);

        let _ = state.filter_advertised(&p, &[txid(1)]);
        state.mark_in_flight(&p, &[txid(1)]);
        state.mark_confirmed(&[txid(1)]);

        assert!(!state.is_in_flight(&txid(1)));
        assert!(state.is_known(&txid(1)));
        let ps = state.peers.get(&p).unwrap();
        assert!(ps.in_flight.is_empty());
        assert!(ps.unacknowledged.is_empty());
    }

    #[test]
    fn shared_tx_state_concurrent_filter() {
        let shared = SharedTxState::default();
        let p1 = peer(1000);
        let p2 = peer(2000);
        shared.register_peer(p1);
        shared.register_peer(p2);

        let outcome1 = shared.filter_advertised(&p1, &[txid(1), txid(2)]);
        assert_eq!(outcome1.to_fetch.len(), 2);

        shared.mark_in_flight(&p1, &[txid(1), txid(2)]);

        let outcome2 = shared.filter_advertised(&p2, &[txid(1), txid(3)]);
        assert_eq!(outcome2.to_fetch, vec![txid(3)]);
        assert_eq!(outcome2.already_known, vec![txid(1)]);
    }

    #[test]
    fn sized_in_flight_tracks_per_peer_and_global_bytes() {
        let mut state = TxState::default();
        let p1 = peer(1000);
        let p2 = peer(2000);
        state.register_peer(p1);
        state.register_peer(p2);

        let _ = state.filter_advertised(&p1, &[txid(1), txid(2)]);
        state.mark_in_flight_sized(&p1, &[(txid(1), 100), (txid(2), 250)]);
        assert_eq!(state.peer_inflight_bytes(&p1), 350);
        assert_eq!(state.inflight_bytes_total(), 350);

        let _ = state.filter_advertised(&p2, &[txid(3)]);
        state.mark_in_flight_sized(&p2, &[(txid(3), 1000)]);
        assert_eq!(state.peer_inflight_bytes(&p2), 1000);
        assert_eq!(state.inflight_bytes_total(), 1350);

        // Receive completes — bytes decrement.
        state.mark_received(&p1, &[txid(1)]);
        assert_eq!(state.peer_inflight_bytes(&p1), 250);
        assert_eq!(state.inflight_bytes_total(), 1250);

        // Not-found drops remaining bytes for that tx.
        state.mark_not_found(&p1, &[txid(2)]);
        assert_eq!(state.peer_inflight_bytes(&p1), 0);
        assert_eq!(state.inflight_bytes_total(), 1000);

        // Confirmed-in-block drops the rest.
        state.mark_confirmed(&[txid(3)]);
        assert_eq!(state.peer_inflight_bytes(&p2), 0);
        assert_eq!(state.inflight_bytes_total(), 0);
    }

    #[test]
    fn unregister_peer_subtracts_inflight_bytes() {
        let mut state = TxState::default();
        let p1 = peer(1000);
        state.register_peer(p1);

        let _ = state.filter_advertised(&p1, &[txid(1), txid(2)]);
        state.mark_in_flight_sized(&p1, &[(txid(1), 500), (txid(2), 700)]);
        assert_eq!(state.inflight_bytes_total(), 1200);

        state.unregister_peer(&p1);
        assert_eq!(state.inflight_bytes_total(), 0);
        assert_eq!(state.peer_inflight_bytes(&p1), 0);
    }

    #[test]
    fn shared_tx_state_sized_round_trip() {
        let shared = SharedTxState::default();
        let p = peer(1000);
        shared.register_peer(p);

        let _ = shared.filter_advertised(&p, &[txid(1)]);
        shared.mark_in_flight_sized(&p, &[(txid(1), 4096)]);
        assert_eq!(shared.peer_inflight_bytes(&p), 4096);
        assert_eq!(shared.inflight_bytes_total(), 4096);

        shared.mark_received(&p, &[txid(1)]);
        assert_eq!(shared.peer_inflight_bytes(&p), 0);
        assert_eq!(shared.inflight_bytes_total(), 0);
    }

    #[test]
    fn already_known_advertisements_do_not_leak_into_unacknowledged() {
        // Regression: previously `filter_advertised` inserted EVERY advertised
        // TxId into `peer_state.unacknowledged`, including items immediately
        // classified as `already_known`.  Those items are acked on the wire
        // and never enter the fetch lifecycle, so they were never removed
        // from `unacknowledged` — the set grew unboundedly across rounds.
        // After the fix, only `to_fetch` items enter `unacknowledged`.
        let mut state = TxState::default();
        let p1 = peer(1000);
        let p2 = peer(2000);
        state.register_peer(p1);
        state.register_peer(p2);

        // p1 advertises and we fetch.
        let _ = state.filter_advertised(&p1, &[txid(1)]);
        state.mark_in_flight(&p1, &[txid(1)]);
        state.mark_confirmed(&[txid(1)]);
        assert_eq!(state.peer_unacked_count(&p1), 0);

        // p2 then advertises the same TxId — already_known.  It must NOT
        // accumulate in p2's unacknowledged set.
        let outcome = state.filter_advertised(&p2, &[txid(1)]);
        assert_eq!(outcome.to_fetch.len(), 0);
        assert_eq!(outcome.already_known.len(), 1);
        assert_eq!(state.peer_unacked_count(&p2), 0);

        // Repeat advertisement also stays clean.
        let _ = state.filter_advertised(&p2, &[txid(1)]);
        let _ = state.filter_advertised(&p2, &[txid(1)]);
        assert_eq!(state.peer_unacked_count(&p2), 0);
    }

    #[test]
    fn peer_inflight_count_tracks_request_lifecycle() {
        // Mirrors upstream `requestedTxsInflight` set size:
        // increments on `mark_in_flight*`, decrements on
        // `mark_received` / `mark_not_found` / `mark_confirmed`,
        // and `unregister_peer` drops it entirely.
        let mut state = TxState::default();
        let p = peer(3000);
        state.register_peer(p);
        assert_eq!(state.peer_inflight_count(&p), 0);

        let _ = state.filter_advertised(&p, &[txid(1), txid(2), txid(3)]);
        state.mark_in_flight_sized(&p, &[(txid(1), 100), (txid(2), 200), (txid(3), 300)]);
        assert_eq!(state.peer_inflight_count(&p), 3);

        state.mark_received(&p, &[txid(1)]);
        assert_eq!(state.peer_inflight_count(&p), 2);

        state.mark_not_found(&p, &[txid(2)]);
        assert_eq!(state.peer_inflight_count(&p), 1);

        // Unrelated confirm does nothing for this peer's count.
        state.mark_confirmed(&[txid(3)]);
        assert_eq!(state.peer_inflight_count(&p), 0);

        // Unknown peer reads as 0.
        assert_eq!(state.peer_inflight_count(&peer(9999)), 0);

        // Shared wrapper exposes the same accessor.
        let shared = SharedTxState::default();
        let p2 = peer(4000);
        shared.register_peer(p2);
        let _ = shared.filter_advertised(&p2, &[txid(7)]);
        shared.mark_in_flight_sized(&p2, &[(txid(7), 50)]);
        assert_eq!(shared.peer_inflight_count(&p2), 1);
        shared.unregister_peer(&p2);
        assert_eq!(shared.peer_inflight_count(&p2), 0);
    }
}
