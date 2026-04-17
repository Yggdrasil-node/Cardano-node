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

use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use yggdrasil_ledger::TxId;

/// Maximum number of recently-known TxIds retained in the `known` ring.
///
/// Once exceeded, the oldest entries are evicted.  This prevents unbounded
/// memory growth as the node processes blocks over time.
const DEFAULT_KNOWN_CAPACITY: usize = 16_384;

/// Per-peer entry tracking which TxIds a peer has advertised and which are
/// currently being fetched from it.
///
/// Reference: `Ouroboros.Network.TxSubmission.Inbound.V2.State.PeerTxState`.
#[derive(Clone, Debug)]
pub struct PeerTxState {
    /// TxIds advertised by this peer that have not yet been acknowledged.
    pub unacknowledged: HashSet<TxId>,
    /// TxIds currently being fetched from this peer.
    pub in_flight: HashSet<TxId>,
}

impl PeerTxState {
    fn new() -> Self {
        Self {
            unacknowledged: HashSet::new(),
            in_flight: HashSet::new(),
        }
    }
}

/// Outcome of filtering a set of advertised TxIds against the shared state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilterOutcome {
    /// TxIds that should be fetched — not yet known or in flight.
    pub to_fetch: Vec<TxId>,
    /// TxIds that are already known or in flight and should be acknowledged
    /// without downloading.
    pub already_known: Vec<TxId>,
}

/// Cross-peer shared transaction state.
///
/// This is the Rust equivalent of the upstream `SharedTxState`:
///
/// - `known` — bounded FIFO of TxIds that are in the mempool or recently
///   confirmed in a block.  Prevents re-downloading transactions already seen.
/// - `in_flight` — TxIds currently being fetched from *any* peer.  Prevents
///   duplicate concurrent fetches.
/// - `peers` — per-peer tracking of advertised and in-flight TxIds.
#[derive(Debug)]
pub struct TxState {
    /// Bounded ring of recently-known TxIds (mempool + confirmed).
    known: HashSet<TxId>,
    /// FIFO eviction queue for `known` — oldest entries at the front.
    known_order: VecDeque<TxId>,
    /// Maximum number of entries retained in `known`.
    known_capacity: usize,
    /// TxIds currently being fetched from any peer.
    global_in_flight: HashSet<TxId>,
    /// Per-peer tracking keyed by remote address.
    peers: std::collections::HashMap<SocketAddr, PeerTxState>,
}

impl Default for TxState {
    fn default() -> Self {
        Self::new(DEFAULT_KNOWN_CAPACITY)
    }
}

impl TxState {
    /// Create a new `TxState` with the given known-TxId ring capacity.
    pub fn new(known_capacity: usize) -> Self {
        Self {
            known: HashSet::with_capacity(known_capacity),
            known_order: VecDeque::with_capacity(known_capacity),
            known_capacity,
            global_in_flight: HashSet::new(),
            peers: std::collections::HashMap::new(),
        }
    }

    /// Register a new peer.  Idempotent — does nothing if already tracked.
    pub fn register_peer(&mut self, addr: SocketAddr) {
        self.peers.entry(addr).or_insert_with(PeerTxState::new);
    }

    /// Unregister a peer and cancel any in-flight fetches attributed to it.
    pub fn unregister_peer(&mut self, addr: &SocketAddr) {
        if let Some(state) = self.peers.remove(addr) {
            for txid in &state.in_flight {
                self.global_in_flight.remove(txid);
            }
        }
    }

    /// Record that a peer advertised a set of TxIds.
    ///
    /// Returns a [`FilterOutcome`] indicating which TxIds should actually be
    /// fetched and which are already known or in flight.
    pub fn filter_advertised(&mut self, peer: &SocketAddr, txids: &[TxId]) -> FilterOutcome {
        let peer_state = self.peers.entry(*peer).or_insert_with(PeerTxState::new);

        let mut to_fetch = Vec::new();
        let mut already_known = Vec::new();

        for txid in txids {
            peer_state.unacknowledged.insert(*txid);

            if self.known.contains(txid) || self.global_in_flight.contains(txid) {
                already_known.push(*txid);
            } else {
                to_fetch.push(*txid);
            }
        }

        FilterOutcome {
            to_fetch,
            already_known,
        }
    }

    /// Mark a set of TxIds as in-flight (being fetched from the given peer).
    pub fn mark_in_flight(&mut self, peer: &SocketAddr, txids: &[TxId]) {
        if let Some(peer_state) = self.peers.get_mut(peer) {
            for txid in txids {
                peer_state.in_flight.insert(*txid);
                self.global_in_flight.insert(*txid);
            }
        }
    }

    /// Mark TxIds as successfully received.  Moves them from in-flight to
    /// known and removes them from the peer's unacknowledged set.
    pub fn mark_received(&mut self, peer: &SocketAddr, txids: &[TxId]) {
        if let Some(peer_state) = self.peers.get_mut(peer) {
            for txid in txids {
                peer_state.in_flight.remove(txid);
                peer_state.unacknowledged.remove(txid);
                self.global_in_flight.remove(txid);
            }
        }
        for txid in txids {
            self.insert_known(*txid);
        }
    }

    /// Mark TxIds that a peer could not deliver (unknown to the peer).
    /// Removes from in-flight and peer tracking so another peer may supply
    /// them.
    pub fn mark_not_found(&mut self, peer: &SocketAddr, txids: &[TxId]) {
        if let Some(peer_state) = self.peers.get_mut(peer) {
            for txid in txids {
                peer_state.in_flight.remove(txid);
                peer_state.unacknowledged.remove(txid);
                self.global_in_flight.remove(txid);
            }
        }
    }

    /// Mark TxIds as confirmed in a block.  These are added to the known
    /// set so they are not re-requested from any peer.
    pub fn mark_confirmed(&mut self, txids: &[TxId]) {
        for txid in txids {
            self.global_in_flight.remove(txid);
            self.insert_known(*txid);
        }
        // Also clean up any per-peer tracking for these txids.
        for peer_state in self.peers.values_mut() {
            for txid in txids {
                peer_state.in_flight.remove(txid);
                peer_state.unacknowledged.remove(txid);
            }
        }
    }

    /// Check whether a TxId is already known (in mempool or confirmed).
    pub fn is_known(&self, txid: &TxId) -> bool {
        self.known.contains(txid)
    }

    /// Check whether a TxId is currently being fetched from any peer.
    pub fn is_in_flight(&self, txid: &TxId) -> bool {
        self.global_in_flight.contains(txid)
    }

    /// Number of TxIds in the known set.
    pub fn known_count(&self) -> usize {
        self.known.len()
    }

    /// Number of TxIds globally in flight.
    pub fn in_flight_count(&self) -> usize {
        self.global_in_flight.len()
    }

    /// Number of tracked peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    // -- private helpers --

    fn insert_known(&mut self, txid: TxId) {
        if self.known.insert(txid) {
            self.known_order.push_back(txid);
            // Evict oldest when over capacity.
            while self.known.len() > self.known_capacity {
                if let Some(old) = self.known_order.pop_front() {
                    self.known.remove(&old);
                }
            }
        }
    }
}

/// Thread-safe shared wrapper around [`TxState`].
///
/// Cloned handles share the same underlying state through `Arc<RwLock<_>>`.
#[derive(Clone, Debug, Default)]
pub struct SharedTxState {
    inner: Arc<RwLock<TxState>>,
}

impl SharedTxState {
    /// Create a shared state with the given known-TxId ring capacity.
    pub fn with_capacity(known_capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(TxState::new(known_capacity))),
        }
    }

    /// Register a new peer.
    pub fn register_peer(&self, addr: SocketAddr) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .register_peer(addr);
    }

    /// Unregister a peer and cancel its in-flight fetches.
    pub fn unregister_peer(&self, addr: &SocketAddr) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .unregister_peer(addr);
    }

    /// Filter advertised TxIds against known + in-flight state.
    pub fn filter_advertised(&self, peer: &SocketAddr, txids: &[TxId]) -> FilterOutcome {
        self.inner
            .write()
            .expect("tx state poisoned")
            .filter_advertised(peer, txids)
    }

    /// Mark TxIds as in-flight from the given peer.
    pub fn mark_in_flight(&self, peer: &SocketAddr, txids: &[TxId]) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .mark_in_flight(peer, txids);
    }

    /// Mark TxIds as successfully received from the given peer.
    pub fn mark_received(&self, peer: &SocketAddr, txids: &[TxId]) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .mark_received(peer, txids);
    }

    /// Mark TxIds as not found (peer couldn't deliver).
    pub fn mark_not_found(&self, peer: &SocketAddr, txids: &[TxId]) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .mark_not_found(peer, txids);
    }

    /// Mark TxIds as confirmed in a block.
    pub fn mark_confirmed(&self, txids: &[TxId]) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .mark_confirmed(txids);
    }

    /// Check whether a TxId is known.
    pub fn is_known(&self, txid: &TxId) -> bool {
        self.inner.read().expect("tx state poisoned").is_known(txid)
    }

    /// Check whether a TxId is in flight.
    pub fn is_in_flight(&self, txid: &TxId) -> bool {
        self.inner
            .read()
            .expect("tx state poisoned")
            .is_in_flight(txid)
    }

    /// Number of known TxIds.
    pub fn known_count(&self) -> usize {
        self.inner.read().expect("tx state poisoned").known_count()
    }

    /// Number of globally in-flight TxIds.
    pub fn in_flight_count(&self) -> usize {
        self.inner
            .read()
            .expect("tx state poisoned")
            .in_flight_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
