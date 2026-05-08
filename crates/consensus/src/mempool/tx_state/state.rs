//! Cross-peer TxId deduplication state machine.
//!
//! Mirrors upstream `Ouroboros.Network.TxSubmission.Inbound.State` —
//! the per-peer "known TxIds I have ack'd from this peer" plus the
//! "in-flight TxIds I asked for" tracking that the inbound TxSubmission2
//! mini-protocol uses to deduplicate TxIds across peers and prevent
//! double-fetches.
//!
//! Three core types:
//!
//! - `PeerTxState` — per-peer state (acks, in-flight sizes,
//!   recently-confirmed window).
//! - `FilterOutcome` — result of filtering inbound TxIds against the
//!   global state.
//! - `TxState` — global state aggregating all peers; owns the
//!   recently-confirmed bounded ring.
//!
//! Plus the `SizeInBytes` alias used throughout the inbound bookkeeping.
//!
//! Extracted from `mempool/tx_state.rs` in R273e (Phase γ §R273 fifth slice).

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;

use yggdrasil_ledger::TxId;

use super::{DEFAULT_KNOWN_CAPACITY, SizeInBytes};

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
    /// Per-in-flight TxId advertised body size, when known.  Used to
    /// derive `inflight_bytes` and to decrement totals on completion.
    ///
    /// Mirrors upstream `requestedTxsInflightSize` accounting in
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State.PeerTxState`.
    pub in_flight_sizes: HashMap<TxId, SizeInBytes>,
    /// Total advertised bytes currently in flight from this peer.
    pub inflight_bytes: u64,
}

impl PeerTxState {
    fn new() -> Self {
        Self {
            unacknowledged: HashSet::new(),
            in_flight: HashSet::new(),
            in_flight_sizes: HashMap::new(),
            inflight_bytes: 0,
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
    /// Sum of advertised body sizes of all in-flight TxIds across all peers.
    ///
    /// Mirrors upstream `inflightTxsSize` in
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State.SharedTxState`.
    inflight_bytes_total: u64,
    /// Per-peer tracking keyed by remote address.
    pub(super) peers: HashMap<SocketAddr, PeerTxState>,
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
            inflight_bytes_total: 0,
            peers: HashMap::new(),
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
            // Subtract this peer's outstanding bytes from the global total.
            self.inflight_bytes_total = self
                .inflight_bytes_total
                .saturating_sub(state.inflight_bytes);
        }
    }

    /// Record that a peer advertised a set of TxIds.
    ///
    /// Returns a [`FilterOutcome`] indicating which TxIds should actually be
    /// fetched and which are already known or in flight.
    ///
    /// Only items in the returned `to_fetch` set are added to the peer's
    /// `unacknowledged` set; items classified as `already_known` are
    /// considered immediately processed (they will be acked on the wire
    /// without entering the per-peer fetch lifecycle), so retaining them
    /// in `unacknowledged` would leak unboundedly across rounds.
    pub fn filter_advertised(&mut self, peer: &SocketAddr, txids: &[TxId]) -> FilterOutcome {
        let peer_state = self.peers.entry(*peer).or_insert_with(PeerTxState::new);

        let mut to_fetch = Vec::new();
        let mut already_known = Vec::new();

        for txid in txids {
            if self.known.contains(txid) || self.global_in_flight.contains(txid) {
                already_known.push(*txid);
            } else {
                peer_state.unacknowledged.insert(*txid);
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

    /// Mark a set of TxIds as in-flight, recording each TxId's advertised
    /// body size for per-peer and global byte accounting.
    ///
    /// Mirrors upstream `acknowledgedTxs`/`requestedTxsInflightSize` updates
    /// in `Ouroboros.Network.TxSubmission.Inbound.V2.Decision`.
    pub fn mark_in_flight_sized(&mut self, peer: &SocketAddr, sized_txids: &[(TxId, SizeInBytes)]) {
        if let Some(peer_state) = self.peers.get_mut(peer) {
            for (txid, size) in sized_txids {
                if peer_state.in_flight.insert(*txid) {
                    self.global_in_flight.insert(*txid);
                    peer_state.in_flight_sizes.insert(*txid, *size);
                    peer_state.inflight_bytes =
                        peer_state.inflight_bytes.saturating_add(*size as u64);
                    self.inflight_bytes_total =
                        self.inflight_bytes_total.saturating_add(*size as u64);
                }
            }
        }
    }

    /// Mark TxIds as successfully received.  Moves them from in-flight to
    /// known and removes them from the peer's unacknowledged set.
    pub fn mark_received(&mut self, peer: &SocketAddr, txids: &[TxId]) {
        if let Some(peer_state) = self.peers.get_mut(peer) {
            for txid in txids {
                if peer_state.in_flight.remove(txid) {
                    if let Some(size) = peer_state.in_flight_sizes.remove(txid) {
                        peer_state.inflight_bytes =
                            peer_state.inflight_bytes.saturating_sub(size as u64);
                        self.inflight_bytes_total =
                            self.inflight_bytes_total.saturating_sub(size as u64);
                    }
                }
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
                if peer_state.in_flight.remove(txid) {
                    if let Some(size) = peer_state.in_flight_sizes.remove(txid) {
                        peer_state.inflight_bytes =
                            peer_state.inflight_bytes.saturating_sub(size as u64);
                        self.inflight_bytes_total =
                            self.inflight_bytes_total.saturating_sub(size as u64);
                    }
                }
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
                if peer_state.in_flight.remove(txid) {
                    if let Some(size) = peer_state.in_flight_sizes.remove(txid) {
                        peer_state.inflight_bytes =
                            peer_state.inflight_bytes.saturating_sub(size as u64);
                        self.inflight_bytes_total =
                            self.inflight_bytes_total.saturating_sub(size as u64);
                    }
                }
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

    /// Total advertised bytes currently in flight across all peers.
    ///
    /// Mirrors upstream `inflightTxsSize` in
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State.SharedTxState`.
    pub fn inflight_bytes_total(&self) -> u64 {
        self.inflight_bytes_total
    }

    /// Total advertised bytes currently in flight from a specific peer.
    ///
    /// Mirrors upstream `requestedTxsInflightSize` in
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State.PeerTxState`.
    pub fn peer_inflight_bytes(&self, peer: &SocketAddr) -> u64 {
        self.peers.get(peer).map(|s| s.inflight_bytes).unwrap_or(0)
    }

    /// Number of TxIds the peer has advertised to us that are still
    /// being tracked through the fetch lifecycle (not yet finalized via
    /// `mark_received`/`mark_not_found`/`mark_confirmed`).
    ///
    /// Mirrors upstream `unacknowledgedTxIds` length in
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State.PeerTxState`.
    pub fn peer_unacked_count(&self, peer: &SocketAddr) -> usize {
        self.peers
            .get(peer)
            .map(|s| s.unacknowledged.len())
            .unwrap_or(0)
    }

    /// Number of TxIds currently in flight (requested via `MsgRequestTxs`
    /// but not yet received) from a specific peer.
    ///
    /// Mirrors upstream `requestedTxsInflight` set size in
    /// `Ouroboros.Network.TxSubmission.Inbound.V2.State.PeerTxState`.
    pub fn peer_inflight_count(&self, peer: &SocketAddr) -> usize {
        self.peers.get(peer).map(|s| s.in_flight.len()).unwrap_or(0)
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
