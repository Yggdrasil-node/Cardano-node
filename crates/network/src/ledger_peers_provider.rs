//! Refresh-oriented provider interfaces for time-varying ledger peers.
//!
//! Upstream keeps ledger peers behind a dedicated provider/thread boundary that
//! feeds the peer-selection governor with either all-ledger peers or big-ledger
//! peers. This module defines a crate-owned seam for Yggdrasil so ledger-driven
//! peer discovery can reconcile into the networking peer registry without
//! pushing source bookkeeping into `node`.

use std::collections::VecDeque;
use std::net::SocketAddr;

use crate::peer_registry::PeerRegistry;

/// The ledger peer source a provider is responsible for refreshing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerPeerProviderKind {
    /// All ledger peers, excluding peers promoted to the big-ledger set.
    LedgerPeers,
    /// Big-ledger peers used for genesis-style sync or eclipse-resistance work.
    BigLedgerPeers,
    /// A combined snapshot containing both ledger and big-ledger peers.
    Combined,
}

/// A normalized snapshot of ledger-derived peer sets.
///
/// Big-ledger peers are kept disjoint from ledger peers to match the upstream
/// `PublicRootPeers.fromDisjointSets` invariant, where ledger peers take
/// precedence over big-ledger peers when overlaps appear.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LedgerPeerSnapshot {
    /// Ledger peers selected from the current ledger view.
    pub ledger_peers: Vec<SocketAddr>,
    /// Big-ledger peers selected from the current ledger or snapshot view.
    pub big_ledger_peers: Vec<SocketAddr>,
}

impl LedgerPeerSnapshot {
    /// Construct a normalized, deduplicated ledger-peer snapshot.
    pub fn new(
        ledger_peers: impl IntoIterator<Item = SocketAddr>,
        big_ledger_peers: impl IntoIterator<Item = SocketAddr>,
    ) -> Self {
        let ledger_peers = unique_peers(ledger_peers);
        let big_ledger_peers = unique_peers(big_ledger_peers)
            .into_iter()
            .filter(|peer| !ledger_peers.contains(peer))
            .collect();

        Self {
            ledger_peers,
            big_ledger_peers,
        }
    }
}

/// A refresh result emitted by a ledger peer provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LedgerPeerProviderRefresh {
    /// Replace the full ledger-peer snapshot.
    Combined(LedgerPeerSnapshot),
    /// Replace only the ledger-peer set.
    LedgerPeers(Vec<SocketAddr>),
    /// Replace only the big-ledger-peer set.
    BigLedgerPeers(Vec<SocketAddr>),
}

/// Provider-side refresh error.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LedgerPeerProviderError {
    /// Provider-specific refresh failure.
    #[error("ledger peer provider refresh failed: {0}")]
    RefreshFailed(String),
}

/// A time-varying ledger-peer provider.
pub trait LedgerPeerProvider {
    /// The source managed by the provider.
    fn kind(&self) -> LedgerPeerProviderKind;

    /// Poll for the next refresh result.
    fn refresh(&mut self) -> Result<Option<LedgerPeerProviderRefresh>, LedgerPeerProviderError>;
}

/// Apply a ledger-provider refresh to the peer registry.
pub fn apply_ledger_peer_refresh(
    registry: &mut PeerRegistry,
    refresh: LedgerPeerProviderRefresh,
) -> bool {
    match refresh {
        LedgerPeerProviderRefresh::Combined(snapshot) => {
            let snapshot = LedgerPeerSnapshot::new(snapshot.ledger_peers, snapshot.big_ledger_peers);
            let ledger_changed = registry.sync_ledger_peers(snapshot.ledger_peers);
            let big_ledger_changed = registry.sync_big_ledger_peers(snapshot.big_ledger_peers);
            ledger_changed || big_ledger_changed
        }
        LedgerPeerProviderRefresh::LedgerPeers(peers) => {
            registry.sync_ledger_peers(unique_peers(peers))
        }
        LedgerPeerProviderRefresh::BigLedgerPeers(peers) => {
            registry.sync_big_ledger_peers(unique_peers(peers))
        }
    }
}

/// Poll a provider once and reconcile the result into the peer registry.
pub fn refresh_ledger_peer_registry<P>(
    registry: &mut PeerRegistry,
    provider: &mut P,
) -> Result<bool, LedgerPeerProviderError>
where
    P: LedgerPeerProvider,
{
    match provider.refresh()? {
        Some(refresh) => Ok(apply_ledger_peer_refresh(registry, refresh)),
        None => Ok(false),
    }
}

/// In-memory scripted provider useful for tests and early integration.
#[derive(Clone, Debug)]
pub struct ScriptedLedgerPeerProvider {
    kind: LedgerPeerProviderKind,
    scripted_refreshes: VecDeque<Result<Option<LedgerPeerProviderRefresh>, LedgerPeerProviderError>>,
}

impl ScriptedLedgerPeerProvider {
    /// Create a provider from scripted refresh results.
    pub fn new(
        kind: LedgerPeerProviderKind,
        scripted_refreshes: impl IntoIterator<Item = Result<Option<LedgerPeerProviderRefresh>, LedgerPeerProviderError>>,
    ) -> Self {
        Self {
            kind,
            scripted_refreshes: scripted_refreshes.into_iter().collect(),
        }
    }
}

impl LedgerPeerProvider for ScriptedLedgerPeerProvider {
    fn kind(&self) -> LedgerPeerProviderKind {
        self.kind
    }

    fn refresh(&mut self) -> Result<Option<LedgerPeerProviderRefresh>, LedgerPeerProviderError> {
        self.scripted_refreshes.pop_front().unwrap_or(Ok(None))
    }
}

fn unique_peers(peers: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut unique = Vec::new();
    for peer in peers {
        if !unique.contains(&peer) {
            unique.push(peer);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_registry::{PeerSource, PeerStatus};

    #[test]
    fn ledger_peer_snapshot_normalizes_duplicates_and_overlap() {
        let shared: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let ledger_only: SocketAddr = "127.0.0.11:3001".parse().expect("addr");
        let big_only: SocketAddr = "127.0.0.12:3001".parse().expect("addr");

        let snapshot = LedgerPeerSnapshot::new(
            [shared, ledger_only, shared],
            [shared, big_only, big_only],
        );

        assert_eq!(snapshot.ledger_peers, vec![shared, ledger_only]);
        assert_eq!(snapshot.big_ledger_peers, vec![big_only]);
    }

    #[test]
    fn apply_combined_ledger_refresh_updates_registry() {
        let ledger_peer: SocketAddr = "127.0.0.20:3001".parse().expect("addr");
        let big_ledger_peer: SocketAddr = "127.0.0.21:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        assert!(apply_ledger_peer_refresh(
            &mut registry,
            LedgerPeerProviderRefresh::Combined(LedgerPeerSnapshot::new(
                [ledger_peer],
                [big_ledger_peer],
            )),
        ));

        assert_eq!(
            registry.get(&ledger_peer).expect("ledger").sources,
            [PeerSource::PeerSourceLedger].into_iter().collect()
        );
        assert_eq!(
            registry.get(&big_ledger_peer).expect("big ledger").sources,
            [PeerSource::PeerSourceBigLedger].into_iter().collect()
        );
    }

    #[test]
    fn apply_combined_ledger_refresh_preserves_status_and_other_sources() {
        let peer: SocketAddr = "127.0.0.22:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(peer, PeerSource::PeerSourceLedger);
        registry.insert_source(peer, PeerSource::PeerSourcePeerShare);
        registry.set_status(peer, PeerStatus::PeerHot);

        assert!(apply_ledger_peer_refresh(
            &mut registry,
            LedgerPeerProviderRefresh::Combined(LedgerPeerSnapshot::default()),
        ));

        let entry = registry.get(&peer).expect("peer remains");
        assert_eq!(entry.sources, [PeerSource::PeerSourcePeerShare].into_iter().collect());
        assert_eq!(entry.status, PeerStatus::PeerHot);
    }

    #[test]
    fn refresh_ledger_peer_registry_applies_scripted_provider() {
        let peer: SocketAddr = "127.0.0.23:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        let mut provider = ScriptedLedgerPeerProvider::new(
            LedgerPeerProviderKind::LedgerPeers,
            [Ok(Some(LedgerPeerProviderRefresh::LedgerPeers(vec![peer])))],
        );

        assert!(refresh_ledger_peer_registry(&mut registry, &mut provider).expect("refresh"));
        assert_eq!(
            registry.get(&peer).expect("ledger").sources,
            [PeerSource::PeerSourceLedger].into_iter().collect()
        );
    }

    #[test]
    fn refresh_ledger_peer_registry_ignores_empty_provider_poll() {
        let mut registry = PeerRegistry::default();
        let mut provider = ScriptedLedgerPeerProvider::new(
            LedgerPeerProviderKind::Combined,
            [Ok(None)],
        );

        assert!(!refresh_ledger_peer_registry(&mut registry, &mut provider).expect("refresh"));
        assert!(registry.is_empty());
    }

    #[test]
    fn refresh_ledger_peer_registry_surfaces_provider_errors() {
        let mut registry = PeerRegistry::default();
        let mut provider = ScriptedLedgerPeerProvider::new(
            LedgerPeerProviderKind::Combined,
            [Err(LedgerPeerProviderError::RefreshFailed("ledger snapshot unavailable".to_owned()))],
        );

        assert_eq!(
            refresh_ledger_peer_registry(&mut registry, &mut provider).expect_err("error"),
            LedgerPeerProviderError::RefreshFailed("ledger snapshot unavailable".to_owned())
        );
    }
}