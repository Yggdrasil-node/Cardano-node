//! dmq-node keepalive registry.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of upstream
//! `Ouroboros.Network.KeepAlive.Registry` — the registry that holds
//! the per-peer `PeerGsv` latency measurements taken by the
//! `KeepAlive` mini-protocol. The DMQ `NodeKernel`'s
//! `fetchClientRegistry` carries it; dmq-node carries its own copy
//! (the R732 dmq-node-local decision).
//!
//! Slice of the Option A `run()` integration arc (see the
//! `docs/COMPLETION_ROADMAP.md` A4 dmq-node entry).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::delta_q::PeerGsv;

/// A registry of per-peer keepalive state — the `PeerGsv` latency
/// measurements plus the block-fetch coordination bookkeeping.
///
/// Mirror of upstream `data KeepAliveRegistry peer m`.
#[derive(Clone, Debug)]
pub struct KeepAliveRegistry<Peer: Ord> {
    /// Per-peer GSV latency measurements from the keepalive protocol
    /// (`dqRegistry`).
    pub dq_registry: Arc<Mutex<BTreeMap<Peer, PeerGsv>>>,
    /// Per-peer block-fetch-client teardown handles (`keepRegistry`).
    /// Upstream the value is `(ThreadId, TMVar ())` — the fetch
    /// client's cancellation target and exit signal. dmq-node runs no
    /// block-fetch clients, so this registry is never populated; its
    /// value is the unit type.
    pub keep_registry: Arc<Mutex<BTreeMap<Peer, ()>>>,
    /// Peers whose keepalive client is being torn down
    /// (`dyingRegistry`).
    pub dying_registry: Arc<Mutex<BTreeSet<Peer>>>,
}

impl<Peer: Ord> Default for KeepAliveRegistry<Peer> {
    fn default() -> Self {
        KeepAliveRegistry {
            dq_registry: Arc::new(Mutex::new(BTreeMap::new())),
            keep_registry: Arc::new(Mutex::new(BTreeMap::new())),
            dying_registry: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
}

/// Construct an empty [`KeepAliveRegistry`].
///
/// Mirror of upstream `newKeepAliveRegistry`.
pub fn new_keep_alive_registry<Peer: Ord>() -> KeepAliveRegistry<Peer> {
    KeepAliveRegistry::default()
}

impl<Peer: Ord + Clone> KeepAliveRegistry<Peer> {
    /// The `PeerGsv`s of the currently-hot peers — those with both a
    /// keepalive measurement and a registered block-fetch client.
    ///
    /// Mirror of upstream `readPeerGSVs` (the `dqRegistry` /
    /// `keepRegistry` map intersection).
    pub fn read_peer_gsvs(&self) -> BTreeMap<Peer, PeerGsv> {
        let dq = self.dq_registry.lock().unwrap_or_else(|e| e.into_inner());
        let keep = self.keep_registry.lock().unwrap_or_else(|e| e.into_inner());
        dq.iter()
            .filter(|(peer, _)| keep.contains_key(*peer))
            .map(|(peer, gsv)| (peer.clone(), *gsv))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta_q::default_gsv;

    #[test]
    fn new_keep_alive_registry_is_empty() {
        let reg: KeepAliveRegistry<String> = new_keep_alive_registry();
        assert!(
            reg.dq_registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
        assert!(reg.read_peer_gsvs().is_empty());
    }

    #[test]
    fn read_peer_gsvs_intersects_dq_and_keep_registries() {
        let reg: KeepAliveRegistry<String> = new_keep_alive_registry();
        // A peer with a GSV measurement but no fetch client is not
        // "hot" — it does not appear in `read_peer_gsvs`.
        reg.dq_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert("measured-only".to_string(), default_gsv());
        assert!(reg.read_peer_gsvs().is_empty());

        // Once it also has a fetch-client entry, it becomes hot.
        reg.keep_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert("measured-only".to_string(), ());
        let hot = reg.read_peer_gsvs();
        assert_eq!(hot.len(), 1);
        assert!(hot.contains_key("measured-only"));
    }
}
