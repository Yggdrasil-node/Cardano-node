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
}

impl PeerRegistryEntry {
    fn new(source: PeerSource) -> Self {
        Self {
            sources: BTreeSet::from([source]),
            status: PeerStatus::PeerCold,
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

    /// Set the status of an existing peer.
    pub fn set_status(&mut self, peer: SocketAddr, status: PeerStatus) -> bool {
        match self.peers.get_mut(&peer) {
            Some(entry) if entry.status != status => {
                entry.status = status;
                true
            }
            _ => false,
        }
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
    pub fn sync_big_ledger_peers(
        &mut self,
        peers: impl IntoIterator<Item = SocketAddr>,
    ) -> bool {
        self.sync_single_source(PeerSource::PeerSourceBigLedger, peers)
    }

    /// Reconcile peers learned via peer sharing.
    pub fn sync_peer_share_peers(
        &mut self,
        peers: impl IntoIterator<Item = SocketAddr>,
    ) -> bool {
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
            for source in &removable_sources {
                changed |= self.remove_source(peer, *source);
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
    use crate::root_peers::{ResolvedLocalRootGroup, ResolvedPublicRootPeers, RootPeerProviders, UseLedgerPeers};

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
            registry.get(&"127.0.0.10:3001".parse().expect("addr")).expect("local").sources,
            BTreeSet::from([PeerSource::PeerSourceLocalRoot])
        );
        assert_eq!(
            registry.get(&"127.0.0.11:3001".parse().expect("addr")).expect("bootstrap").sources,
            BTreeSet::from([PeerSource::PeerSourceBootstrap])
        );
        assert_eq!(
            registry.get(&"127.0.0.12:3001".parse().expect("addr")).expect("public").sources,
            BTreeSet::from([PeerSource::PeerSourcePublicRoot])
        );
        assert_eq!(registry.status_counts(), PeerRegistryStatusCounts { cold: 3, cooling: 0, warm: 0, hot: 0 });
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
        assert_eq!(entry.sources, BTreeSet::from([PeerSource::PeerSourcePeerShare]));
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
        assert_eq!(entry.sources, BTreeSet::from([PeerSource::PeerSourceBootstrap]));
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
            registry.get(&old_peer).expect("old peer remains via peer share").sources,
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

        assert_eq!(registry.status_counts(), PeerRegistryStatusCounts { cold: 1, cooling: 1, warm: 1, hot: 1 });
    }
}