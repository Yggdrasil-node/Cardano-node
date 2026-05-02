//! Peer registry state for the networking layer.
//!
//! This is the first step toward an upstream-style peer registry. It tracks
//! peer source and peer status independently from node orchestration, and it
//! can reconcile the current root-provider snapshot into canonical root-peer
//! sources.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;

use crate::root_peers::RootPeerProviders;

/// Where a peer was discovered.
///
/// This stays close to the upstream `PeerSource` naming while leaving room for
/// Cardano-specific public-root categories such as bootstrap and ledger peers.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PeerSource {
    /// Peer discovered from a local root group.
    PeerSourceLocalRoot,
    /// Peer discovered from configured public roots.
    PeerSourcePublicRoot,
    /// Peer discovered from configured bootstrap peers.
    PeerSourceBootstrap,
    /// Peer discovered from ledger peers.
    PeerSourceLedger,
    /// Peer discovered from a big-ledger peer source.
    PeerSourceBigLedger,
    /// Peer discovered via peer sharing.
    PeerSourcePeerShare,
}

/// Current status of a peer connection candidate.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PeerStatus {
    /// No active connection exists.
    PeerCold,
    /// The peer is being demoted to cold.
    PeerCooling,
    /// An established connection exists.
    PeerWarm,
    /// The peer is active and running hot protocols.
    PeerHot,
}

/// Registry entry for a peer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerRegistryEntry {
    /// All currently known sources for the peer.
    pub sources: BTreeSet<PeerSource>,
    /// Current status of the peer.
    pub status: PeerStatus,
    /// Last known tip slot for hot-peer selection.
    pub hot_tip_slot: Option<u64>,
    /// Indicates if the peer was hot but then got demoted.
    ///
    /// Upstream: `knownPeerTepid` in `KnownPeerInfo`.
    /// Set on hot→warm transition, cleared on cold→warm transition.
    /// Used by promotion policies to deprioritize recently-demoted peers.
    pub tepid: bool,
}

impl PeerRegistryEntry {
    fn new(source: PeerSource) -> Self {
        Self {
            sources: BTreeSet::from([source]),
            status: PeerStatus::PeerCold,
            hot_tip_slot: None,
            tepid: false,
        }
    }

    /// Returns `true` when the peer is currently rooted.
    pub fn is_root_peer(&self) -> bool {
        self.sources.iter().any(|source| {
            matches!(
                source,
                PeerSource::PeerSourceLocalRoot
                    | PeerSource::PeerSourcePublicRoot
                    | PeerSource::PeerSourceBootstrap
                    | PeerSource::PeerSourceLedger
                    | PeerSource::PeerSourceBigLedger
            )
        })
    }
}

/// Registry of known peers and their source/status information.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeerRegistry {
    peers: BTreeMap<SocketAddr, PeerRegistryEntry>,
}

impl PeerRegistry {
    /// Returns the number of known peers.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Returns `true` when the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Returns a registry entry for the given peer, if present.
    pub fn get(&self, peer: &SocketAddr) -> Option<&PeerRegistryEntry> {
        self.peers.get(peer)
    }

    /// Iterate over all known peers.
    pub fn iter(&self) -> impl Iterator<Item = (&SocketAddr, &PeerRegistryEntry)> {
        self.peers.iter()
    }

    /// Insert a source for a peer, creating a cold entry when needed.
    pub fn insert_source(&mut self, peer: SocketAddr, source: PeerSource) -> bool {
        match self.peers.get_mut(&peer) {
            Some(entry) => entry.sources.insert(source),
            None => {
                self.peers.insert(peer, PeerRegistryEntry::new(source));
                true
            }
        }
    }

    /// Remove a source from a peer. If no sources remain, the peer is removed.
    pub fn remove_source(&mut self, peer: SocketAddr, source: PeerSource) -> bool {
        let mut changed = false;
        let mut should_remove = false;

        if let Some(entry) = self.peers.get_mut(&peer) {
            changed = entry.sources.remove(&source);
            should_remove = entry.sources.is_empty();
        }

        if should_remove {
            self.peers.remove(&peer);
        }

        changed
    }

    /// Remove a peer entirely from the registry.
    ///
    /// Returns `true` if the peer was present and removed.
    pub fn remove(&mut self, peer: &SocketAddr) -> bool {
        self.peers.remove(peer).is_some()
    }

    /// Set the status of an existing peer.
    ///
    /// Tracks the upstream `knownPeerTepid` flag: set on hot→warm
    /// demotion, cleared on cold→warm promotion.
    pub fn set_status(&mut self, peer: SocketAddr, status: PeerStatus) -> bool {
        match self.peers.get_mut(&peer) {
            Some(entry) if entry.status != status => {
                // Upstream: tepid is set on Hot→Warm, cleared on Cold→Warm.
                match (entry.status, status) {
                    (PeerStatus::PeerHot, PeerStatus::PeerWarm) => {
                        entry.tepid = true;
                    }
                    (PeerStatus::PeerCold | PeerStatus::PeerCooling, PeerStatus::PeerWarm) => {
                        entry.tepid = false;
                    }
                    _ => {}
                }
                entry.status = status;
                if status != PeerStatus::PeerHot {
                    entry.hot_tip_slot = None;
                }
                true
            }
            _ => false,
        }
    }

    /// Set the last known tip slot for an existing peer.
    ///
    /// Returns `true` when the value changed.
    pub fn set_hot_tip_slot(&mut self, peer: SocketAddr, hot_tip_slot: Option<u64>) -> bool {
        match self.peers.get_mut(&peer) {
            Some(entry)
                if entry.status == PeerStatus::PeerHot && entry.hot_tip_slot != hot_tip_slot =>
            {
                entry.hot_tip_slot = hot_tip_slot;
                true
            }
            _ => false,
        }
    }

    /// Return the preferred hot peer for reconnect attempts.
    ///
    /// Prefers hot peers with the highest known tip slot. If no hot peer has a
    /// known slot, returns the first hot peer in stable address order.
    pub fn preferred_hot_peer(&self) -> Option<SocketAddr> {
        let mut best_with_tip: Option<(SocketAddr, u64)> = None;
        let mut first_hot: Option<SocketAddr> = None;

        for (addr, entry) in &self.peers {
            if entry.status != PeerStatus::PeerHot {
                continue;
            }

            if first_hot.is_none() {
                first_hot = Some(*addr);
            }

            if let Some(slot) = entry.hot_tip_slot {
                match best_with_tip {
                    Some((_, best_slot)) if slot <= best_slot => {}
                    _ => best_with_tip = Some((*addr, slot)),
                }
            }
        }

        best_with_tip.map(|(addr, _)| addr).or(first_hot)
    }

    /// Return all hot peers ordered for reconnect attempts.
    ///
    /// Peers with known tip slots are ordered first by descending tip slot.
    /// Ties and peers without known tip slots are ordered by stable address.
    pub fn hot_peers_by_reconnect_priority(&self) -> Vec<SocketAddr> {
        let mut peers = self
            .peers
            .iter()
            .filter_map(|(addr, entry)| {
                (entry.status == PeerStatus::PeerHot).then_some((*addr, entry.hot_tip_slot))
            })
            .collect::<Vec<_>>();

        peers.sort_by(|(addr_a, tip_a), (addr_b, tip_b)| match (tip_a, tip_b) {
            (Some(a), Some(b)) => b.cmp(a).then_with(|| addr_a.cmp(addr_b)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => addr_a.cmp(addr_b),
        });

        peers.into_iter().map(|(addr, _)| addr).collect()
    }

    /// Return the last known hot tip slot for a peer.
    pub fn hot_tip_slot(&self, peer: SocketAddr) -> Option<u64> {
        self.peers.get(&peer).and_then(|entry| entry.hot_tip_slot)
    }

    /// Reconcile root-peer sources from the current root-provider snapshot.
    ///
    /// Root sources are updated to match the snapshot while preserving other
    /// sources such as peer-share information and preserving status for peers
    /// that remain in the registry.
    pub fn sync_root_peers(&mut self, providers: &RootPeerProviders) -> bool {
        let desired = desired_root_sources(providers);
        self.sync_source_map(
            [
                PeerSource::PeerSourceLocalRoot,
                PeerSource::PeerSourcePublicRoot,
                PeerSource::PeerSourceBootstrap,
            ],
            desired,
        )
    }

    /// Reconcile ledger peers from the current dynamic ledger snapshot.
    pub fn sync_ledger_peers(&mut self, peers: impl IntoIterator<Item = SocketAddr>) -> bool {
        self.sync_single_source(PeerSource::PeerSourceLedger, peers)
    }

    /// Reconcile big-ledger peers from the current dynamic ledger snapshot.
    pub fn sync_big_ledger_peers(&mut self, peers: impl IntoIterator<Item = SocketAddr>) -> bool {
        self.sync_single_source(PeerSource::PeerSourceBigLedger, peers)
    }

    /// Reconcile peers learned via peer sharing.
    pub fn sync_peer_share_peers(&mut self, peers: impl IntoIterator<Item = SocketAddr>) -> bool {
        self.sync_single_source(PeerSource::PeerSourcePeerShare, peers)
    }

    /// Count peers by status.
    pub fn status_counts(&self) -> PeerRegistryStatusCounts {
        let mut counts = PeerRegistryStatusCounts::default();
        for entry in self.peers.values() {
            match entry.status {
                PeerStatus::PeerCold => counts.cold += 1,
                PeerStatus::PeerCooling => counts.cooling += 1,
                PeerStatus::PeerWarm => counts.warm += 1,
                PeerStatus::PeerHot => counts.hot += 1,
            }
        }
        counts
    }

    fn sync_single_source(
        &mut self,
        source: PeerSource,
        peers: impl IntoIterator<Item = SocketAddr>,
    ) -> bool {
        let desired = peers
            .into_iter()
            .map(|peer| (peer, source))
            .collect::<BTreeMap<_, _>>();
        self.sync_source_map([source], desired)
    }

    fn sync_source_map(
        &mut self,
        removable_sources: impl IntoIterator<Item = PeerSource>,
        desired: BTreeMap<SocketAddr, PeerSource>,
    ) -> bool {
        let removable_sources = removable_sources.into_iter().collect::<Vec<_>>();
        let current_peers = self.peers.keys().copied().collect::<Vec<_>>();
        let mut changed = false;

        for peer in current_peers {
            let desired_source = desired.get(&peer).copied();
            let mut should_remove = false;

            if let Some(entry) = self.peers.get_mut(&peer) {
                if let Some(source) = desired_source {
                    changed |= entry.sources.insert(source);
                }

                for source in &removable_sources {
                    if Some(*source) != desired_source {
                        changed |= entry.sources.remove(source);
                    }
                }

                should_remove = entry.sources.is_empty();
            }

            if should_remove {
                self.peers.remove(&peer);
            }
        }

        for (peer, source) in desired {
            changed |= self.insert_source(peer, source);
        }

        changed
    }
}

/// Counts of peers by connection status.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerRegistryStatusCounts {
    /// Number of cold peers.
    pub cold: usize,
    /// Number of cooling peers.
    pub cooling: usize,
    /// Number of warm peers.
    pub warm: usize,
    /// Number of hot peers.
    pub hot: usize,
}

fn desired_root_sources(providers: &RootPeerProviders) -> BTreeMap<SocketAddr, PeerSource> {
    let mut desired = BTreeMap::new();

    for group in &providers.local_roots {
        for peer in &group.peers {
            desired.insert(*peer, PeerSource::PeerSourceLocalRoot);
        }
    }

    for peer in &providers.public_roots.bootstrap_peers {
        desired.insert(*peer, PeerSource::PeerSourceBootstrap);
    }

    for peer in &providers.public_roots.public_config_peers {
        desired.insert(*peer, PeerSource::PeerSourcePublicRoot);
    }

    desired
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_selection::PeerDiffusionMode;
    use crate::root_peers::{
        ResolvedLocalRootGroup, ResolvedPublicRootPeers, RootPeerProviders, UseLedgerPeers,
    };

    #[test]
    fn sync_root_peers_inserts_root_sources() {
        let mut registry = PeerRegistry::default();
        let providers = RootPeerProviders {
            local_roots: vec![ResolvedLocalRootGroup {
                peers: vec!["127.0.0.10:3001".parse().expect("addr")],
                advertise: false,
                trustable: true,
                hot_valency: 1,
                warm_valency: 1,
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            }],
            public_roots: ResolvedPublicRootPeers {
                bootstrap_peers: vec!["127.0.0.11:3001".parse().expect("addr")],
                public_config_peers: vec!["127.0.0.12:3001".parse().expect("addr")],
            },
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };

        assert!(registry.sync_root_peers(&providers));
        assert_eq!(registry.len(), 3);
        assert_eq!(
            registry
                .get(&"127.0.0.10:3001".parse().expect("addr"))
                .expect("local")
                .sources,
            BTreeSet::from([PeerSource::PeerSourceLocalRoot])
        );
        assert_eq!(
            registry
                .get(&"127.0.0.11:3001".parse().expect("addr"))
                .expect("bootstrap")
                .sources,
            BTreeSet::from([PeerSource::PeerSourceBootstrap])
        );
        assert_eq!(
            registry
                .get(&"127.0.0.12:3001".parse().expect("addr"))
                .expect("public")
                .sources,
            BTreeSet::from([PeerSource::PeerSourcePublicRoot])
        );
        assert_eq!(
            registry.status_counts(),
            PeerRegistryStatusCounts {
                cold: 3,
                cooling: 0,
                warm: 0,
                hot: 0
            }
        );
    }

    #[test]
    fn sync_root_peers_preserves_non_root_sources_and_status() {
        let peer: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(peer, PeerSource::PeerSourceLocalRoot);
        registry.insert_source(peer, PeerSource::PeerSourcePeerShare);
        registry.set_status(peer, PeerStatus::PeerWarm);

        let providers = RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };

        assert!(registry.sync_root_peers(&providers));
        let entry = registry.get(&peer).expect("peer remains via peer-share");
        assert_eq!(
            entry.sources,
            BTreeSet::from([PeerSource::PeerSourcePeerShare])
        );
        assert_eq!(entry.status, PeerStatus::PeerWarm);
    }

    #[test]
    fn sync_root_peers_preserves_status_for_still_desired_root() {
        let peer: SocketAddr = "127.0.0.11:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        registry.set_status(peer, PeerStatus::PeerHot);

        let providers = RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers {
                bootstrap_peers: vec![peer],
                public_config_peers: vec![],
            },
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };

        assert!(!registry.sync_root_peers(&providers));
        let entry = registry.get(&peer).expect("peer remains rooted");
        assert_eq!(
            entry.sources,
            BTreeSet::from([PeerSource::PeerSourceBootstrap])
        );
        assert_eq!(entry.status, PeerStatus::PeerHot);
    }

    #[test]
    fn sync_root_peers_preserves_status_when_root_source_changes() {
        let peer: SocketAddr = "127.0.0.11:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        registry.set_status(peer, PeerStatus::PeerWarm);

        let providers = RootPeerProviders {
            local_roots: vec![ResolvedLocalRootGroup {
                peers: vec![peer],
                advertise: false,
                trustable: true,
                hot_valency: 1,
                warm_valency: 1,
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            }],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };

        assert!(registry.sync_root_peers(&providers));
        let entry = registry.get(&peer).expect("peer remains rooted");
        assert_eq!(
            entry.sources,
            BTreeSet::from([PeerSource::PeerSourceLocalRoot])
        );
        assert_eq!(entry.status, PeerStatus::PeerWarm);
    }

    #[test]
    fn sync_root_peers_removes_peers_with_no_remaining_sources() {
        let peer: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);

        let providers = RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };

        assert!(registry.sync_root_peers(&providers));
        assert!(registry.get(&peer).is_none());
        assert!(registry.is_empty());
    }

    #[test]
    fn sync_ledger_peers_preserves_other_sources_and_status() {
        let peer: SocketAddr = "127.0.0.20:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        registry.insert_source(peer, PeerSource::PeerSourceLedger);
        registry.set_status(peer, PeerStatus::PeerWarm);

        assert!(registry.sync_ledger_peers([]));

        let entry = registry.get(&peer).expect("bootstrap peer remains");
        assert_eq!(
            entry.sources,
            BTreeSet::from([PeerSource::PeerSourceBootstrap])
        );
        assert_eq!(entry.status, PeerStatus::PeerWarm);
    }

    #[test]
    fn sync_ledger_peers_replaces_only_ledger_source_members() {
        let old_peer: SocketAddr = "127.0.0.21:3001".parse().expect("addr");
        let new_peer: SocketAddr = "127.0.0.22:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(old_peer, PeerSource::PeerSourceLedger);
        registry.insert_source(old_peer, PeerSource::PeerSourcePeerShare);

        assert!(registry.sync_ledger_peers([new_peer]));

        assert_eq!(
            registry
                .get(&old_peer)
                .expect("old peer remains via peer share")
                .sources,
            BTreeSet::from([PeerSource::PeerSourcePeerShare])
        );
        assert_eq!(
            registry.get(&new_peer).expect("new ledger peer").sources,
            BTreeSet::from([PeerSource::PeerSourceLedger])
        );
    }

    #[test]
    fn sync_big_ledger_peers_reconciles_big_ledger_source() {
        let peer: SocketAddr = "127.0.0.23:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        assert!(registry.sync_big_ledger_peers([peer]));
        assert_eq!(
            registry.get(&peer).expect("big ledger peer").sources,
            BTreeSet::from([PeerSource::PeerSourceBigLedger])
        );

        assert!(registry.sync_big_ledger_peers([]));
        assert!(registry.get(&peer).is_none());
    }

    #[test]
    fn sync_peer_share_peers_reconciles_peer_share_source() {
        let peer: SocketAddr = "127.0.0.24:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        assert!(registry.sync_peer_share_peers([peer]));
        assert_eq!(
            registry.get(&peer).expect("peer-share peer").sources,
            BTreeSet::from([PeerSource::PeerSourcePeerShare])
        );

        assert!(registry.sync_peer_share_peers([]));
        assert!(registry.get(&peer).is_none());
    }

    #[test]
    fn status_counts_track_all_status_buckets() {
        let mut registry = PeerRegistry::default();
        let cold: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let cooling: SocketAddr = "127.0.0.11:3001".parse().expect("addr");
        let warm: SocketAddr = "127.0.0.12:3001".parse().expect("addr");
        let hot: SocketAddr = "127.0.0.13:3001".parse().expect("addr");

        registry.insert_source(cold, PeerSource::PeerSourcePublicRoot);
        registry.insert_source(cooling, PeerSource::PeerSourcePublicRoot);
        registry.insert_source(warm, PeerSource::PeerSourcePublicRoot);
        registry.insert_source(hot, PeerSource::PeerSourcePublicRoot);
        registry.set_status(cooling, PeerStatus::PeerCooling);
        registry.set_status(warm, PeerStatus::PeerWarm);
        registry.set_status(hot, PeerStatus::PeerHot);

        assert_eq!(
            registry.status_counts(),
            PeerRegistryStatusCounts {
                cold: 1,
                cooling: 1,
                warm: 1,
                hot: 1
            }
        );
    }

    #[test]
    fn preferred_hot_peer_uses_highest_tip_slot() {
        let hot_a: SocketAddr = "127.0.0.20:3001".parse().expect("addr");
        let hot_b: SocketAddr = "127.0.0.21:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        registry.insert_source(hot_a, PeerSource::PeerSourceBootstrap);
        registry.insert_source(hot_b, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot_a, PeerStatus::PeerHot);
        registry.set_status(hot_b, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot_a, Some(100));
        registry.set_hot_tip_slot(hot_b, Some(250));

        assert_eq!(registry.preferred_hot_peer(), Some(hot_b));
    }

    #[test]
    fn preferred_hot_peer_falls_back_to_first_hot_without_tip() {
        let hot_a: SocketAddr = "127.0.0.22:3001".parse().expect("addr");
        let hot_b: SocketAddr = "127.0.0.23:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        registry.insert_source(hot_b, PeerSource::PeerSourceBootstrap);
        registry.insert_source(hot_a, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot_a, PeerStatus::PeerHot);
        registry.set_status(hot_b, PeerStatus::PeerHot);

        // Registry iteration is stable by address (BTreeMap), so hot_a wins.
        assert_eq!(registry.preferred_hot_peer(), Some(hot_a));
    }

    #[test]
    fn setting_non_hot_status_clears_hot_tip_slot() {
        let peer: SocketAddr = "127.0.0.24:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        registry.set_status(peer, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(peer, Some(123));
        assert_eq!(
            registry.get(&peer).and_then(|entry| entry.hot_tip_slot),
            Some(123)
        );

        registry.set_status(peer, PeerStatus::PeerWarm);
        assert_eq!(
            registry.get(&peer).and_then(|entry| entry.hot_tip_slot),
            None
        );
    }

    #[test]
    fn set_hot_tip_slot_ignored_for_non_hot_peers() {
        let peer: SocketAddr = "127.0.0.25:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        assert!(!registry.set_hot_tip_slot(peer, Some(9)));
        assert_eq!(
            registry.get(&peer).and_then(|entry| entry.hot_tip_slot),
            None
        );

        registry.set_status(peer, PeerStatus::PeerHot);
        assert!(registry.set_hot_tip_slot(peer, Some(9)));
        assert_eq!(
            registry.get(&peer).and_then(|entry| entry.hot_tip_slot),
            Some(9)
        );
    }

    #[test]
    fn hot_peers_by_reconnect_priority_orders_by_tip_then_address() {
        let hot_1: SocketAddr = "127.0.0.30:3001".parse().expect("addr");
        let hot_2: SocketAddr = "127.0.0.31:3001".parse().expect("addr");
        let hot_3: SocketAddr = "127.0.0.32:3001".parse().expect("addr");
        let warm: SocketAddr = "127.0.0.33:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        for peer in [hot_1, hot_2, hot_3, warm] {
            registry.insert_source(peer, PeerSource::PeerSourceBootstrap);
        }
        registry.set_status(hot_1, PeerStatus::PeerHot);
        registry.set_status(hot_2, PeerStatus::PeerHot);
        registry.set_status(hot_3, PeerStatus::PeerHot);
        registry.set_status(warm, PeerStatus::PeerWarm);

        registry.set_hot_tip_slot(hot_1, Some(200));
        registry.set_hot_tip_slot(hot_2, Some(300));
        // hot_3 intentionally has no known tip slot.

        assert_eq!(
            registry.hot_peers_by_reconnect_priority(),
            vec![hot_2, hot_1, hot_3]
        );
    }

    #[test]
    fn hot_tip_slot_returns_slot_for_hot_peer() {
        let mut registry = PeerRegistry::default();
        let hot: SocketAddr = "127.0.0.1:3900".parse().unwrap();

        registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
        registry.set_status(hot, PeerStatus::PeerHot);
        registry.set_hot_tip_slot(hot, Some(777));

        assert_eq!(registry.hot_tip_slot(hot), Some(777));
    }
}
