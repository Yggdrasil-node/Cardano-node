//! Thread-safe shared wrapper around `TxState`.
//!
//! Mirrors the runtime-facing handle that the inbound TxSubmission2
//! mini-protocol clients hold to share `TxState` updates across peer
//! tasks via `Arc<RwLock<>>`.
//!
//! Single public type: `SharedTxState`.
//!
//! Extracted from `mempool/tx_state.rs` in R273e (Phase γ §R273 fifth slice).

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use yggdrasil_ledger::TxId;

use super::{FilterOutcome, SizeInBytes, TxState};

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

    /// Mark TxIds as in-flight from the given peer with their advertised
    /// body sizes recorded for per-peer/global byte accounting.
    pub fn mark_in_flight_sized(&self, peer: &SocketAddr, sized_txids: &[(TxId, SizeInBytes)]) {
        self.inner
            .write()
            .expect("tx state poisoned")
            .mark_in_flight_sized(peer, sized_txids);
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

    /// Total advertised bytes currently in flight across all peers.
    pub fn inflight_bytes_total(&self) -> u64 {
        self.inner
            .read()
            .expect("tx state poisoned")
            .inflight_bytes_total()
    }

    /// Total advertised bytes currently in flight from a specific peer.
    pub fn peer_inflight_bytes(&self, peer: &SocketAddr) -> u64 {
        self.inner
            .read()
            .expect("tx state poisoned")
            .peer_inflight_bytes(peer)
    }

    /// Number of TxIds the peer has advertised to us that are still
    /// being tracked through the fetch lifecycle.
    pub fn peer_unacked_count(&self, peer: &SocketAddr) -> usize {
        self.inner
            .read()
            .expect("tx state poisoned")
            .peer_unacked_count(peer)
    }

    /// Number of TxIds currently in flight from a specific peer.
    pub fn peer_inflight_count(&self, peer: &SocketAddr) -> usize {
        self.inner
            .read()
            .expect("tx state poisoned")
            .peer_inflight_count(peer)
    }
}
