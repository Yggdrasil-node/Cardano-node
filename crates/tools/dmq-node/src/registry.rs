//! dmq-node inbound-V2 tx-submission channel registry.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of the registry types of upstream
//! `Ouroboros.Network.TxSubmission.Inbound.V2.Registry` — the
//! channels carrying decisions from the inbound-V2 governor to each
//! `SigSubmission` peer client, plus the mempool-access semaphore.
//! The DMQ `NodeKernel` (`Diffusion/NodeKernel.hs`) holds the
//! `TxChannelsVar` (`sigChannelVar`) and the `TxMempoolSem`
//! (`sigMempoolSem`). dmq-node carries its own copy — the R732
//! dmq-node-local decision.
//!
//! First slice of the Option A `run()` integration arc (see
//! `docs/COMPLETION_ROADMAP.md` A4 dmq-node entry).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::inbound_v2::TxDecision;

/// A one-slot mailbox carrying the latest [`TxDecision`] for one peer
/// from the inbound-V2 governor to that peer's `SigSubmission` client.
///
/// Mirror of upstream's per-peer `StrictMVar m (TxDecision txid tx)`.
pub type TxDecisionChannel = Arc<Mutex<Option<TxDecision>>>;

/// The registry of per-peer decision channels.
///
/// Mirror of upstream `newtype TxChannels m peeraddr txid tx`.
#[derive(Clone, Debug)]
pub struct TxChannels<PeerAddr: Ord> {
    /// One decision mailbox per registered peer.
    pub tx_channel_map: BTreeMap<PeerAddr, TxDecisionChannel>,
}

impl<PeerAddr: Ord> Default for TxChannels<PeerAddr> {
    fn default() -> Self {
        TxChannels {
            tx_channel_map: BTreeMap::new(),
        }
    }
}

impl<PeerAddr: Ord> TxChannels<PeerAddr> {
    /// An empty registry.
    pub fn new() -> TxChannels<PeerAddr> {
        TxChannels::default()
    }

    /// Register a peer, creating and storing its empty decision
    /// channel and returning a handle to it.
    pub fn register(&mut self, peer: PeerAddr) -> TxDecisionChannel {
        let channel: TxDecisionChannel = Arc::new(Mutex::new(None));
        self.tx_channel_map.insert(peer, Arc::clone(&channel));
        channel
    }

    /// The decision channel of a registered peer, if present.
    pub fn channel(&self, peer: &PeerAddr) -> Option<&TxDecisionChannel> {
        self.tx_channel_map.get(peer)
    }

    /// Remove a peer from the registry (e.g. when it disconnects).
    pub fn unregister(&mut self, peer: &PeerAddr) {
        self.tx_channel_map.remove(peer);
    }
}

/// A [`TxChannels`] registry behind a shared lock — the DMQ
/// `NodeKernel`'s `sigChannelVar`.
///
/// Mirror of upstream `type TxChannelsVar`.
pub type TxChannelsVar<PeerAddr> = Arc<Mutex<TxChannels<PeerAddr>>>;

/// Construct an empty [`TxChannelsVar`].
///
/// Mirror of upstream `newTxChannelsVar`.
pub fn new_tx_channels_var<PeerAddr: Ord>() -> TxChannelsVar<PeerAddr> {
    Arc::new(Mutex::new(TxChannels::new()))
}

/// The mempool-access semaphore — serialises signature submission to
/// the mempool.
///
/// Mirror of upstream `newtype TxMempoolSem m = TxMempoolSem (TSem m)`,
/// initialised with one permit (`newTSem 1`); a single-permit
/// counting semaphore is exclusive access, modelled here as a mutex.
#[derive(Clone, Debug, Default)]
pub struct TxMempoolSem(Arc<Mutex<()>>);

impl TxMempoolSem {
    /// A fresh mempool semaphore with one permit.
    ///
    /// Mirror of upstream `newTxMempoolSem`.
    pub fn new() -> TxMempoolSem {
        TxMempoolSem::default()
    }

    /// Acquire exclusive mempool access, blocking until the permit is
    /// free; the returned guard releases it on drop. A poisoned lock
    /// is recovered rather than propagated.
    pub fn acquire(&self) -> MutexGuard<'_, ()> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_channels_register_lookup_unregister() {
        let mut channels: TxChannels<String> = TxChannels::new();
        assert!(channels.channel(&"peer-a".to_string()).is_none());

        let handle = channels.register("peer-a".to_string());
        // The registered peer's channel is reachable and starts empty.
        assert!(channels.channel(&"peer-a".to_string()).is_some());
        assert!(handle.lock().unwrap_or_else(|e| e.into_inner()).is_none());

        // The registry handle and the stored channel are the same cell.
        *handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(TxDecision::empty());
        let stored = channels.channel(&"peer-a".to_string()).expect("registered");
        assert!(stored.lock().unwrap_or_else(|e| e.into_inner()).is_some());

        channels.unregister(&"peer-a".to_string());
        assert!(channels.channel(&"peer-a".to_string()).is_none());
    }

    #[test]
    fn tx_channels_var_starts_empty() {
        let var = new_tx_channels_var::<String>();
        let guard = var.lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.tx_channel_map.is_empty());
    }

    #[test]
    fn tx_mempool_sem_grants_exclusive_access() {
        let sem = TxMempoolSem::new();
        {
            let _guard = sem.acquire();
            // The permit is held for the guard's lifetime.
        }
        // Released on drop — a second acquire succeeds.
        let _guard = sem.acquire();
    }
}
