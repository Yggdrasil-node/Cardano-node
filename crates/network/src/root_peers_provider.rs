//! Refresh-oriented provider interfaces for time-varying root peers.
//!
//! Upstream splits root-peer handling between time-varying providers and the
//! peer-selection governor observing their current values. This module defines
//! the provider-side seam for Yggdrasil without coupling it to any particular
//! DNS or ledger implementation yet.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::peer_registry::PeerRegistry;
use crate::peer_selection::{
    resolve_peer_access_points, LocalRootConfig, PeerAccessPoint, PublicRootConfig,
};
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

/// Poll a provider once, reconcile the result into root-provider state, and
/// sync the peer registry from the resulting root snapshot.
///
/// This keeps provider-side refresh handling and consumer-side root-peer
/// registry reconciliation on one crate-owned path so `node` does not need to
/// carry any root-peer bookkeeping state.
pub fn refresh_root_peer_state_and_registry<P>(
    state: &mut RootPeerProviderState,
    registry: &mut PeerRegistry,
    provider: &mut P,
) -> Result<bool, RootPeerProviderError>
where
    P: RootPeerProvider,
{
    match provider.refresh()? {
        Some(refresh) => {
            let state_changed = state.apply_refresh(refresh);
            let registry_changed = registry.sync_root_peers(state.providers());
            Ok(state_changed || registry_changed)
        }
        None => Ok(false),
    }
}

/// In-memory scripted provider useful for tests and early integration.
#[derive(Clone, Debug)]
pub struct ScriptedRootPeerProvider {
    kind: RootPeerProviderKind,
    scripted_refreshes: VecDeque<Result<Option<RootPeerProviderRefresh>, RootPeerProviderError>>,
}

/// Static DNS-backed provider for configured bootstrap and public root peers.
///
/// Re-resolves configured access points on each poll. Lookup failures are
/// treated as absent results for the affected access point so the provider
/// can continue emitting the currently resolvable subset.
///
/// When a [`DnsRefreshPolicy`] is attached via [`with_policy`](Self::with_policy),
/// the provider gates re-resolution behind a time-based schedule with
/// exponential backoff on unchanged (stale) results, matching the upstream
/// `clipTTLBelow`/`clipTTLAbove` constants.
#[derive(Clone, Debug)]
pub struct DnsRootPeerProvider {
    config: DnsRootPeerProviderConfig,
    last_refresh: Option<RootPeerProviderRefresh>,
    schedule: Option<DnsRefreshSchedule>,
}

/// Time-gated refresh policy for DNS-backed root-peer providers.
///
/// Since `std::net::ToSocketAddrs` does not expose DNS TTL, we use
/// configurable base and maximum intervals with exponential backoff on stale
/// (unchanged) resolution results.  The defaults match the upstream
/// `clipTTLBelow` (60 s) and `clipTTLAbove` (900 s / 15 min) constants from
/// `ouroboros-network`.
#[derive(Clone, Debug)]
pub struct DnsRefreshPolicy {
    /// Base delay between DNS re-resolution attempts (default 60 s).
    pub base_interval: Duration,
    /// Maximum delay after exponential backoff on stale results (default
    /// 900 s / 15 min).
    pub max_interval: Duration,
}

impl Default for DnsRefreshPolicy {
    fn default() -> Self {
        Self {
            base_interval: Duration::from_secs(60),
            max_interval: Duration::from_secs(900),
        }
    }
}

/// Internal schedule state tracking when the next DNS resolution should happen.
#[derive(Clone, Debug)]
struct DnsRefreshSchedule {
    policy: DnsRefreshPolicy,
    /// Number of consecutive unchanged-result resolutions.
    stale_count: u32,
    /// When the last DNS resolution was performed.
    last_resolved_at: Option<Instant>,
}

impl DnsRefreshSchedule {
    fn new(policy: DnsRefreshPolicy) -> Self {
        Self {
            policy,
            stale_count: 0,
            last_resolved_at: None,
        }
    }

    /// Whether enough time has elapsed to perform a new resolution.
    fn should_resolve(&self, now: Instant) -> bool {
        match self.last_resolved_at {
            None => true,
            Some(last) => now.duration_since(last) >= self.current_interval(),
        }
    }

    /// Current interval based on exponential backoff state.
    ///
    /// `base_interval * 2^stale_count`, capped at `max_interval`.
    fn current_interval(&self) -> Duration {
        let shift = self.stale_count.min(8);
        let multiplier = 1u32.checked_shl(shift).unwrap_or(u32::MAX);
        let backed_off = self.policy.base_interval.saturating_mul(multiplier);
        backed_off.min(self.policy.max_interval)
    }

    /// Record that resolution produced changed results.
    fn record_change(&mut self, now: Instant) {
        self.stale_count = 0;
        self.last_resolved_at = Some(now);
    }

    /// Record that resolution produced unchanged results.
    fn record_stale(&mut self, now: Instant) {
        self.stale_count = self.stale_count.saturating_add(1);
        self.last_resolved_at = Some(now);
    }
}

/// Input configuration for the DNS-backed root-peer provider.
#[derive(Clone, Debug)]
pub enum DnsRootPeerProviderConfig {
    /// Resolve configured local-root groups.
    LocalRoots(Vec<LocalRootConfig>),
    /// Resolve configured bootstrap peers.
    BootstrapPeers(Vec<PeerAccessPoint>),
    /// Resolve configured public-root groups.
    PublicConfigPeers(Vec<PublicRootConfig>),
    /// Resolve both bootstrap peers and configured public-root groups.
    PublicRoots {
        bootstrap_peers: Vec<PeerAccessPoint>,
        public_roots: Vec<PublicRootConfig>,
    },
}

impl DnsRootPeerProvider {
    /// Create a DNS-backed provider for configured local-root groups.
    pub fn local_roots(local_roots: Vec<LocalRootConfig>) -> Self {
        Self {
            config: DnsRootPeerProviderConfig::LocalRoots(local_roots),
            last_refresh: None,
            schedule: None,
        }
    }

    /// Create a DNS-backed provider for configured bootstrap peers.
    pub fn bootstrap_peers(access_points: Vec<PeerAccessPoint>) -> Self {
        Self {
            config: DnsRootPeerProviderConfig::BootstrapPeers(access_points),
            last_refresh: None,
            schedule: None,
        }
    }

    /// Create a DNS-backed provider for configured public-root groups.
    pub fn public_config_peers(public_roots: Vec<PublicRootConfig>) -> Self {
        Self {
            config: DnsRootPeerProviderConfig::PublicConfigPeers(public_roots),
            last_refresh: None,
            schedule: None,
        }
    }

    /// Create a DNS-backed provider for both bootstrap and configured public roots.
    pub fn public_roots(
        bootstrap_peers: Vec<PeerAccessPoint>,
        public_roots: Vec<PublicRootConfig>,
    ) -> Self {
        Self {
            config: DnsRootPeerProviderConfig::PublicRoots {
                bootstrap_peers,
                public_roots,
            },
            last_refresh: None,
            schedule: None,
        }
    }

    /// Attach a time-gated refresh policy.
    ///
    /// When a policy is attached, the provider gates DNS re-resolution behind
    /// a time-based schedule. After each resolution, the provider waits at
    /// least `base_interval` before re-resolving. If consecutive resolutions
    /// produce unchanged results, the wait doubles each time (exponential
    /// backoff) up to `max_interval`.
    ///
    /// Without a policy, the provider resolves on every `refresh()` call and
    /// suppresses unchanged results via value comparison alone.
    pub fn with_policy(mut self, policy: DnsRefreshPolicy) -> Self {
        self.schedule = Some(DnsRefreshSchedule::new(policy));
        self
    }
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

impl RootPeerProvider for DnsRootPeerProvider {
    fn kind(&self) -> RootPeerProviderKind {
        match self.config {
            DnsRootPeerProviderConfig::LocalRoots(_) => RootPeerProviderKind::LocalRoots,
            DnsRootPeerProviderConfig::BootstrapPeers(_) => RootPeerProviderKind::BootstrapPeers,
            DnsRootPeerProviderConfig::PublicConfigPeers(_) => {
                RootPeerProviderKind::PublicConfigPeers
            }
            DnsRootPeerProviderConfig::PublicRoots { .. } => RootPeerProviderKind::PublicRoots,
        }
    }

    fn refresh(&mut self) -> Result<Option<RootPeerProviderRefresh>, RootPeerProviderError> {
        // When a schedule is active, skip resolution if not enough time has
        // elapsed since the last attempt.
        let now = Instant::now();
        if let Some(ref schedule) = self.schedule {
            if !schedule.should_resolve(now) {
                return Ok(None);
            }
        }

        let next = match &self.config {
            DnsRootPeerProviderConfig::LocalRoots(local_roots) => {
                RootPeerProviderRefresh::LocalRoots(resolve_local_root_groups(local_roots))
            }
            DnsRootPeerProviderConfig::BootstrapPeers(access_points) => {
                RootPeerProviderRefresh::BootstrapPeers(resolve_access_points(access_points))
            }
            DnsRootPeerProviderConfig::PublicConfigPeers(public_roots) => {
                RootPeerProviderRefresh::PublicConfigPeers(resolve_public_root_groups(public_roots))
            }
            DnsRootPeerProviderConfig::PublicRoots {
                bootstrap_peers,
                public_roots,
            } => RootPeerProviderRefresh::PublicRoots(ResolvedPublicRootPeers {
                bootstrap_peers: resolve_access_points(bootstrap_peers),
                public_config_peers: resolve_public_root_groups(public_roots),
            }),
        };

        if self.last_refresh.as_ref() == Some(&next) {
            if let Some(ref mut schedule) = self.schedule {
                schedule.record_stale(now);
            }
            Ok(None)
        } else {
            self.last_refresh = Some(next.clone());
            if let Some(ref mut schedule) = self.schedule {
                schedule.record_change(now);
            }
            Ok(Some(next))
        }
    }
}

fn resolve_access_points(access_points: &[PeerAccessPoint]) -> Vec<SocketAddr> {
    let mut resolved = Vec::new();

    for access_point in access_points {
        for addr in resolve_peer_access_points(access_point) {
            if !resolved.contains(&addr) {
                resolved.push(addr);
            }
        }
    }

    resolved
}

fn resolve_public_root_groups(public_roots: &[PublicRootConfig]) -> Vec<SocketAddr> {
    let mut resolved = Vec::new();

    for group in public_roots {
        for addr in resolve_access_points(&group.access_points) {
            if !resolved.contains(&addr) {
                resolved.push(addr);
            }
        }
    }

    resolved
}

fn resolve_local_root_groups(local_roots: &[LocalRootConfig]) -> Vec<ResolvedLocalRootGroup> {
    local_roots
        .iter()
        .map(|group| {
            let mut peers = Vec::new();
            for access_point in &group.access_points {
                for addr in resolve_peer_access_points(access_point) {
                    if !peers.contains(&addr) {
                        peers.push(addr);
                    }
                }
            }

            ResolvedLocalRootGroup {
                peers,
                advertise: group.advertise,
                trustable: group.trustable,
                hot_valency: group.hot_valency,
                warm_valency: group.effective_warm_valency(),
                diffusion_mode: group.diffusion_mode,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_registry::{PeerSource, PeerStatus};
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

    #[test]
    fn refresh_root_peer_state_and_registry_syncs_root_entries() {
        let local_peer: SocketAddr = "127.0.0.11:3001".parse().expect("addr");
        let old_bootstrap: SocketAddr = "127.0.0.12:3001".parse().expect("addr");
        let new_bootstrap: SocketAddr = "127.0.0.13:3001".parse().expect("addr");
        let topology = TopologyConfig {
            bootstrap_peers: UseBootstrapPeers::UseBootstrapPeers(vec![PeerAccessPoint {
                address: "127.0.0.12".to_owned(),
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
        let mut registry = PeerRegistry::default();
        assert!(registry.sync_root_peers(state.providers()));
        assert!(registry.set_status(local_peer, PeerStatus::PeerWarm));
        registry.insert_source(local_peer, PeerSource::PeerSourcePeerShare);

        let mut provider = ScriptedRootPeerProvider::new(
            RootPeerProviderKind::BootstrapPeers,
            [Ok(Some(RootPeerProviderRefresh::BootstrapPeers(vec![new_bootstrap])))],
        );

        assert!(refresh_root_peer_state_and_registry(&mut state, &mut registry, &mut provider)
            .expect("refresh"));
        assert_eq!(state.providers().public_roots.bootstrap_peers, vec![new_bootstrap]);

        let local_entry = registry.get(&local_peer).expect("local peer");
        assert_eq!(local_entry.status, PeerStatus::PeerWarm);
        assert_eq!(
            local_entry.sources,
            [
                PeerSource::PeerSourceLocalRoot,
                PeerSource::PeerSourcePeerShare,
            ]
            .into_iter()
            .collect()
        );
        assert!(registry.get(&old_bootstrap).is_none());
        assert_eq!(
            registry.get(&new_bootstrap).expect("new bootstrap").sources,
            [PeerSource::PeerSourceBootstrap].into_iter().collect()
        );
    }

    #[test]
    fn refresh_root_peer_state_and_registry_ignores_empty_provider_poll() {
        let mut state = RootPeerProviderState::from_providers(RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        });
        let mut registry = PeerRegistry::default();
        let mut provider =
            ScriptedRootPeerProvider::new(RootPeerProviderKind::PublicRoots, [Ok(None)]);

        assert!(registry.is_empty());
        assert!(state.providers().public_roots.all_peers().is_empty());
        assert!(
            !refresh_root_peer_state_and_registry(&mut state, &mut registry, &mut provider)
                .expect("refresh")
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn dns_root_peer_provider_resolves_bootstrap_peers_and_suppresses_noop_refreshes() {
        let mut provider = DnsRootPeerProvider::bootstrap_peers(vec![PeerAccessPoint {
            address: "127.0.0.10".to_owned(),
            port: 3001,
        }]);

        assert_eq!(provider.kind(), RootPeerProviderKind::BootstrapPeers);
        assert_eq!(
            provider.refresh().expect("refresh"),
            Some(RootPeerProviderRefresh::BootstrapPeers(vec![
                "127.0.0.10:3001".parse().expect("addr"),
            ]))
        );
        assert_eq!(provider.refresh().expect("refresh"), None);
    }

    #[test]
    fn dns_root_peer_provider_resolves_public_root_groups() {
        let mut provider = DnsRootPeerProvider::public_config_peers(vec![PublicRootConfig {
            access_points: vec![
                PeerAccessPoint {
                    address: "127.0.0.20".to_owned(),
                    port: 3001,
                },
                PeerAccessPoint {
                    address: "127.0.0.21".to_owned(),
                    port: 3001,
                },
            ],
            advertise: false,
        }]);

        assert_eq!(provider.kind(), RootPeerProviderKind::PublicConfigPeers);
        assert_eq!(
            provider.refresh().expect("refresh"),
            Some(RootPeerProviderRefresh::PublicConfigPeers(vec![
                "127.0.0.20:3001".parse().expect("addr"),
                "127.0.0.21:3001".parse().expect("addr"),
            ]))
        );
    }

    #[test]
    fn dns_root_peer_provider_combines_bootstrap_and_public_roots() {
        let mut provider = DnsRootPeerProvider::public_roots(
            vec![PeerAccessPoint {
                address: "127.0.0.30".to_owned(),
                port: 3001,
            }],
            vec![PublicRootConfig {
                access_points: vec![PeerAccessPoint {
                    address: "127.0.0.31".to_owned(),
                    port: 3001,
                }],
                advertise: false,
            }],
        );

        assert_eq!(provider.kind(), RootPeerProviderKind::PublicRoots);
        assert_eq!(
            provider.refresh().expect("refresh"),
            Some(RootPeerProviderRefresh::PublicRoots(ResolvedPublicRootPeers {
                bootstrap_peers: vec!["127.0.0.30:3001".parse().expect("addr")],
                public_config_peers: vec!["127.0.0.31:3001".parse().expect("addr")],
            }))
        );
    }

    #[test]
    fn dns_root_peer_provider_resolves_local_roots_and_suppresses_noop_refreshes() {
        let mut provider = DnsRootPeerProvider::local_roots(vec![
            LocalRootConfig {
                access_points: vec![
                    PeerAccessPoint {
                        address: "127.0.0.40".to_owned(),
                        port: 3001,
                    },
                    PeerAccessPoint {
                        address: "127.0.0.41".to_owned(),
                        port: 3001,
                    },
                ],
                advertise: true,
                trustable: true,
                hot_valency: 2,
                warm_valency: Some(3),
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            },
            LocalRootConfig {
                access_points: vec![PeerAccessPoint {
                    address: "127.0.0.42".to_owned(),
                    port: 3002,
                }],
                advertise: false,
                trustable: false,
                hot_valency: 1,
                warm_valency: None,
                diffusion_mode: PeerDiffusionMode::InitiatorOnlyDiffusionMode,
            },
        ]);

        assert_eq!(provider.kind(), RootPeerProviderKind::LocalRoots);

        let refresh = provider.refresh().expect("refresh");
        let expected = vec![
            ResolvedLocalRootGroup {
                peers: vec![
                    "127.0.0.40:3001".parse().expect("addr"),
                    "127.0.0.41:3001".parse().expect("addr"),
                ],
                advertise: true,
                trustable: true,
                hot_valency: 2,
                warm_valency: 3,
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            },
            ResolvedLocalRootGroup {
                peers: vec!["127.0.0.42:3002".parse().expect("addr")],
                advertise: false,
                trustable: false,
                hot_valency: 1,
                warm_valency: 1,
                diffusion_mode: PeerDiffusionMode::InitiatorOnlyDiffusionMode,
            },
        ];
        assert_eq!(refresh, Some(RootPeerProviderRefresh::LocalRoots(expected)));

        // Second poll with same config should be suppressed.
        assert_eq!(provider.refresh().expect("refresh"), None);
    }

    #[test]
    fn dns_local_root_provider_deduplicates_within_group() {
        let mut provider = DnsRootPeerProvider::local_roots(vec![LocalRootConfig {
            access_points: vec![
                PeerAccessPoint {
                    address: "127.0.0.50".to_owned(),
                    port: 3001,
                },
                PeerAccessPoint {
                    address: "127.0.0.50".to_owned(),
                    port: 3001,
                },
            ],
            advertise: false,
            trustable: false,
            hot_valency: 1,
            warm_valency: None,
            diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
        }]);

        let refresh = provider.refresh().expect("refresh");
        let expected = vec![ResolvedLocalRootGroup {
            peers: vec!["127.0.0.50:3001".parse().expect("addr")],
            advertise: false,
            trustable: false,
            hot_valency: 1,
            warm_valency: 1,
            diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
        }];
        assert_eq!(refresh, Some(RootPeerProviderRefresh::LocalRoots(expected)));
    }

    #[test]
    fn dns_local_root_provider_integrates_with_registry() {
        let mut state = RootPeerProviderState::from_providers(RootPeerProviders {
            local_roots: vec![],
            public_roots: ResolvedPublicRootPeers::default(),
            use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
            peer_snapshot_file: None,
        });
        let mut registry = PeerRegistry::default();

        let mut provider = DnsRootPeerProvider::local_roots(vec![LocalRootConfig {
            access_points: vec![PeerAccessPoint {
                address: "127.0.0.60".to_owned(),
                port: 3001,
            }],
            advertise: false,
            trustable: true,
            hot_valency: 1,
            warm_valency: None,
            diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
        }]);

        assert!(refresh_root_peer_state_and_registry(&mut state, &mut registry, &mut provider)
            .expect("refresh"));

        let peer_addr: SocketAddr = "127.0.0.60:3001".parse().expect("addr");
        let entry = registry.get(&peer_addr).expect("local root peer");
        assert!(entry.sources.contains(&PeerSource::PeerSourceLocalRoot));
        assert_eq!(entry.status, PeerStatus::PeerCold);

        assert_eq!(state.providers().local_roots.len(), 1);
        assert_eq!(state.providers().local_roots[0].peers, vec![peer_addr]);
    }

    // -- Refresh-policy tests -------------------------------------------------

    #[test]
    fn refresh_schedule_current_interval_uses_exponential_backoff() {
        let policy = DnsRefreshPolicy {
            base_interval: Duration::from_secs(60),
            max_interval: Duration::from_secs(900),
        };
        let mut schedule = DnsRefreshSchedule::new(policy);
        let now = Instant::now();

        // stale_count 0: base interval
        assert_eq!(schedule.current_interval(), Duration::from_secs(60));

        // stale 1: 120 s
        schedule.record_stale(now);
        assert_eq!(schedule.current_interval(), Duration::from_secs(120));

        // stale 2: 240 s
        schedule.record_stale(now);
        assert_eq!(schedule.current_interval(), Duration::from_secs(240));

        // stale 3: 480 s
        schedule.record_stale(now);
        assert_eq!(schedule.current_interval(), Duration::from_secs(480));

        // stale 4: 960 s → capped at max 900 s
        schedule.record_stale(now);
        assert_eq!(schedule.current_interval(), Duration::from_secs(900));

        // After a change: reset to base
        schedule.record_change(now);
        assert_eq!(schedule.current_interval(), Duration::from_secs(60));
    }

    #[test]
    fn refresh_schedule_should_resolve_gates_on_elapsed_time() {
        let policy = DnsRefreshPolicy {
            base_interval: Duration::from_secs(60),
            max_interval: Duration::from_secs(900),
        };
        let mut schedule = DnsRefreshSchedule::new(policy);
        let now = Instant::now();

        // First call: always resolve (no prior resolution)
        assert!(schedule.should_resolve(now));

        schedule.record_change(now);

        // Immediately after: should not resolve
        assert!(!schedule.should_resolve(now));

        // Just before base interval: still no
        assert!(!schedule.should_resolve(now + Duration::from_secs(59)));

        // At base interval: should resolve
        assert!(schedule.should_resolve(now + Duration::from_secs(60)));
    }

    #[test]
    fn refresh_schedule_backoff_increases_wait_between_resolves() {
        let policy = DnsRefreshPolicy {
            base_interval: Duration::from_secs(60),
            max_interval: Duration::from_secs(900),
        };
        let mut schedule = DnsRefreshSchedule::new(policy);
        let t0 = Instant::now();

        // First resolve at t0
        schedule.record_stale(t0);

        // After 60 s (base): would resolve if stale_count were 0, but
        // stale_count is 1 so need 120 s.
        assert!(!schedule.should_resolve(t0 + Duration::from_secs(60)));
        assert!(!schedule.should_resolve(t0 + Duration::from_secs(119)));
        assert!(schedule.should_resolve(t0 + Duration::from_secs(120)));
    }

    #[test]
    fn dns_provider_with_policy_first_call_resolves() {
        let mut provider = DnsRootPeerProvider::bootstrap_peers(vec![PeerAccessPoint {
            address: "127.0.0.10".to_owned(),
            port: 3001,
        }])
        .with_policy(DnsRefreshPolicy {
            base_interval: Duration::from_secs(3600),
            max_interval: Duration::from_secs(7200),
        });

        // First call always resolves regardless of policy.
        let first = provider.refresh().expect("refresh");
        assert!(first.is_some());

        // Second call within the 1-hour base interval: suppressed by schedule.
        let second = provider.refresh().expect("refresh");
        assert!(second.is_none());
    }

    #[test]
    fn dns_provider_without_policy_resolves_every_call() {
        let mut provider = DnsRootPeerProvider::bootstrap_peers(vec![PeerAccessPoint {
            address: "127.0.0.10".to_owned(),
            port: 3001,
        }]);

        // Without policy, every call resolves (but unchanged results are still
        // suppressed via value comparison).
        assert!(provider.refresh().expect("first").is_some());
        // Second call: same result, suppressed by value comparison.
        assert!(provider.refresh().expect("second").is_none());
    }

    #[test]
    fn dns_provider_with_zero_interval_policy_resolves_every_call() {
        let mut provider = DnsRootPeerProvider::bootstrap_peers(vec![PeerAccessPoint {
            address: "127.0.0.10".to_owned(),
            port: 3001,
        }])
        .with_policy(DnsRefreshPolicy {
            base_interval: Duration::from_secs(0),
            max_interval: Duration::from_secs(0),
        });

        // Zero interval: always resolves, but result unchanged → suppressed.
        assert!(provider.refresh().expect("first").is_some());
        assert!(provider.refresh().expect("second").is_none());
    }
}