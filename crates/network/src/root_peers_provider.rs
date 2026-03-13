//! Refresh-oriented provider interfaces for time-varying root peers.
//!
//! Upstream splits root-peer handling between time-varying providers and the
//! peer-selection governor observing their current values. This module defines
//! the provider-side seam for Yggdrasil without coupling it to any particular
//! DNS or ledger implementation yet.

use std::collections::VecDeque;
use std::net::SocketAddr;

use crate::root_peers::{
    ResolvedLocalRootGroup, ResolvedPublicRootPeers, RootPeerProviderState,
    TopologyConfig,
};

/// The root source a provider is responsible for refreshing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootPeerProviderKind {
    /// Full topology replacement, including policy fields.
    Topology,
    /// Local-root group refreshes.
    LocalRoots,
    /// Bootstrap-peer refreshes.
    BootstrapPeers,
    /// Public configured root-peer refreshes.
    PublicConfigPeers,
    /// Combined public-root refreshes.
    PublicRoots,
}

/// A refresh result emitted by a root provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootPeerProviderRefresh {
    /// Replace the full topology configuration.
    Topology(TopologyConfig),
    /// Replace resolved local-root groups.
    LocalRoots(Vec<ResolvedLocalRootGroup>),
    /// Replace only bootstrap peers.
    BootstrapPeers(Vec<SocketAddr>),
    /// Replace only configured public roots.
    PublicConfigPeers(Vec<SocketAddr>),
    /// Replace all resolved public roots.
    PublicRoots(ResolvedPublicRootPeers),
}

/// Provider-side refresh error.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RootPeerProviderError {
    /// Provider-specific refresh failure.
    #[error("root peer provider refresh failed: {0}")]
    RefreshFailed(String),
}

/// A time-varying root-peer provider.
///
/// Providers return `Ok(None)` when no new snapshot is available, or
/// `Ok(Some(...))` when a refreshed snapshot should be reconciled into the
/// current `RootPeerProviderState`.
pub trait RootPeerProvider {
    /// The source managed by the provider.
    fn kind(&self) -> RootPeerProviderKind;

    /// Poll for the next refresh result.
    fn refresh(&mut self) -> Result<Option<RootPeerProviderRefresh>, RootPeerProviderError>;
}

/// Poll a provider once and reconcile the result into root-provider state.
pub fn refresh_root_peer_state<P>(
    state: &mut RootPeerProviderState,
    provider: &mut P,
) -> Result<bool, RootPeerProviderError>
where
    P: RootPeerProvider,
{
    match provider.refresh()? {
        Some(refresh) => Ok(state.apply_refresh(refresh)),
        None => Ok(false),
    }
}

/// In-memory scripted provider useful for tests and early integration.
#[derive(Clone, Debug)]
pub struct ScriptedRootPeerProvider {
    kind: RootPeerProviderKind,
    scripted_refreshes: VecDeque<Result<Option<RootPeerProviderRefresh>, RootPeerProviderError>>,
}

impl ScriptedRootPeerProvider {
    /// Create a provider from scripted refresh results.
    pub fn new(
        kind: RootPeerProviderKind,
        scripted_refreshes: impl IntoIterator<Item = Result<Option<RootPeerProviderRefresh>, RootPeerProviderError>>,
    ) -> Self {
        Self {
            kind,
            scripted_refreshes: scripted_refreshes.into_iter().collect(),
        }
    }
}

impl RootPeerProvider for ScriptedRootPeerProvider {
    fn kind(&self) -> RootPeerProviderKind {
        self.kind
    }

    fn refresh(&mut self) -> Result<Option<RootPeerProviderRefresh>, RootPeerProviderError> {
        self.scripted_refreshes.pop_front().unwrap_or(Ok(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_selection::{LocalRootConfig, PeerAccessPoint, PeerDiffusionMode};
    use crate::root_peers::{RootPeerProviders, UseBootstrapPeers, UseLedgerPeers};

    #[test]
    fn refresh_root_peer_state_applies_scripted_update() {
        let topology = TopologyConfig {
            bootstrap_peers: UseBootstrapPeers::UseBootstrapPeers(vec![PeerAccessPoint {
                address: "127.0.0.10".to_owned(),
                port: 3001,
            }]),
            local_roots: vec![LocalRootConfig {
                access_points: vec![PeerAccessPoint {
                    address: "127.0.0.11".to_owned(),
                    port: 3001,
                }],
                advertise: false,
                trustable: true,
                hot_valency: 1,
                warm_valency: None,
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            }],
            public_roots: vec![],
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        };
        let mut state = RootPeerProviderState::from_topology(&topology);
        let mut provider = ScriptedRootPeerProvider::new(
            RootPeerProviderKind::BootstrapPeers,
            [Ok(Some(RootPeerProviderRefresh::BootstrapPeers(vec![
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ])))],
        );

        assert!(refresh_root_peer_state(&mut state, &mut provider).expect("refresh"));
        assert_eq!(
            state.providers().public_roots.bootstrap_peers,
            vec!["127.0.0.12:3001".parse().expect("addr")]
        );
    }

    #[test]
    fn refresh_root_peer_state_ignores_empty_provider_poll() {
        let mut state = RootPeerProviderState::from_providers(RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        });
        let before = state.providers().clone();
        let mut provider = ScriptedRootPeerProvider::new(RootPeerProviderKind::PublicRoots, [Ok(None)]);

        assert!(!refresh_root_peer_state(&mut state, &mut provider).expect("refresh"));
        assert_eq!(state.providers(), &before);
    }

    #[test]
    fn refresh_root_peer_state_surfaces_provider_errors() {
        let mut state = RootPeerProviderState::from_providers(RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        });
        let mut provider = ScriptedRootPeerProvider::new(
            RootPeerProviderKind::LocalRoots,
            [Err(RootPeerProviderError::RefreshFailed("dns timeout".to_owned()))],
        );

        assert_eq!(
            refresh_root_peer_state(&mut state, &mut provider).expect_err("error"),
            RootPeerProviderError::RefreshFailed("dns timeout".to_owned())
        );
    }
}