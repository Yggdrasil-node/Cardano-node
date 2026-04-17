//! Topology root-peer domain types and resolved provider snapshots.
//!
//! This module keeps the topology-root model in the network crate so node code
//! can remain focused on orchestration. It mirrors the upstream Cardano split
//! between local roots, public roots, bootstrap peers, and ledger-peer gating.

use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::peer_selection::{LocalRootConfig, PeerAccessPoint, PublicRootConfig};
use crate::root_peers_provider::RootPeerProviderRefresh;

/// Slot threshold used by `UseLedgerPeers`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AfterSlot {
    /// Use ledger peers immediately.
    Always,
    /// Use ledger peers only after the given slot.
    After(u64),
}

/// Upstream-style ledger-peer toggle from topology configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum UseLedgerPeers {
    /// Do not use ledger peers.
    #[default]
    DontUseLedgerPeers,
    /// Use ledger peers with the given slot policy.
    UseLedgerPeers(AfterSlot),
}

impl UseLedgerPeers {
    /// Returns `true` when ledger peers are enabled.
    pub fn enabled(self) -> bool {
        !matches!(self, Self::DontUseLedgerPeers)
    }

    /// Convert to the legacy `Option<u64>` representation used by current
    /// node configuration code.
    pub fn to_after_slot(self) -> Option<u64> {
        match self {
            Self::DontUseLedgerPeers => None,
            Self::UseLedgerPeers(AfterSlot::Always) => Some(0),
            Self::UseLedgerPeers(AfterSlot::After(slot)) => Some(slot),
        }
    }
}

impl Serialize for UseLedgerPeers {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::DontUseLedgerPeers => serializer.serialize_i64(-1),
            Self::UseLedgerPeers(AfterSlot::Always) => serializer.serialize_u64(0),
            Self::UseLedgerPeers(AfterSlot::After(slot)) => serializer.serialize_u64(*slot),
        }
    }
}

impl<'de> Deserialize<'de> for UseLedgerPeers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UseLedgerPeersVisitor;

        impl Visitor<'_> for UseLedgerPeersVisitor {
            type Value = UseLedgerPeers;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("-1, 0, a positive slot number, or null")
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    i64::MIN..=-1 => Ok(UseLedgerPeers::DontUseLedgerPeers),
                    0 => Ok(UseLedgerPeers::UseLedgerPeers(AfterSlot::Always)),
                    positive => Ok(UseLedgerPeers::UseLedgerPeers(AfterSlot::After(
                        positive as u64,
                    ))),
                }
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(match value {
                    0 => UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
                    slot => UseLedgerPeers::UseLedgerPeers(AfterSlot::After(slot)),
                })
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(UseLedgerPeers::DontUseLedgerPeers)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(UseLedgerPeers::DontUseLedgerPeers)
            }
        }

        deserializer.deserialize_any(UseLedgerPeersVisitor)
    }
}

/// Upstream-style bootstrap-peer toggle from topology configuration.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub enum UseBootstrapPeers {
    /// Do not use configured bootstrap peers.
    #[default]
    DontUseBootstrapPeers,
    /// Use the given configured bootstrap peers.
    UseBootstrapPeers(Vec<PeerAccessPoint>),
}

impl UseBootstrapPeers {
    /// Returns the configured bootstrap peers as a slice.
    pub fn configured_peers(&self) -> &[PeerAccessPoint] {
        match self {
            Self::DontUseBootstrapPeers => &[],
            Self::UseBootstrapPeers(peers) => peers,
        }
    }

    /// Returns `true` when bootstrap peers are enabled.
    ///
    /// Upstream: `isBootstrapPeersEnabled`.
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::UseBootstrapPeers(_))
    }
}

impl Serialize for UseBootstrapPeers {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::DontUseBootstrapPeers => serializer.serialize_none(),
            Self::UseBootstrapPeers(peers) => peers.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for UseBootstrapPeers {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UseBootstrapPeersVisitor;

        impl<'de> Visitor<'de> for UseBootstrapPeersVisitor {
            type Value = UseBootstrapPeers;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("null or a bootstrap peer array")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(UseBootstrapPeers::DontUseBootstrapPeers)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(UseBootstrapPeers::DontUseBootstrapPeers)
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let peers =
                    Deserialize::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))?;
                Ok(UseBootstrapPeers::UseBootstrapPeers(peers))
            }
        }

        deserializer.deserialize_any(UseBootstrapPeersVisitor)
    }
}

/// Network-owned topology root configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopologyConfig {
    /// Configured bootstrap peers, if enabled.
    #[serde(default)]
    pub bootstrap_peers: UseBootstrapPeers,
    /// Configured local root peer groups.
    #[serde(default)]
    pub local_roots: Vec<LocalRootConfig>,
    /// Configured public root peer groups.
    #[serde(default)]
    pub public_roots: Vec<PublicRootConfig>,
    /// Ledger-peer gating policy.
    #[serde(default, rename = "useLedgerAfterSlot")]
    pub use_ledger_peers: UseLedgerPeers,
    /// Optional peer snapshot file.
    #[serde(default)]
    pub peer_snapshot_file: Option<String>,
}

impl TopologyConfig {
    /// Resolve configured roots into a provider snapshot with stable ordering
    /// and upstream precedence between local, bootstrap, and public roots.
    pub fn resolved_root_providers(&self) -> RootPeerProviders {
        resolve_root_peer_providers(
            self.bootstrap_peers.configured_peers(),
            &self.local_roots,
            &self.public_roots,
            self.use_ledger_peers,
            self.peer_snapshot_file.clone(),
        )
    }
}

/// Resolved local-root group with concrete peer addresses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedLocalRootGroup {
    /// Ordered concrete peer addresses for the group.
    pub peers: Vec<SocketAddr>,
    /// Whether peers in this group may be advertised.
    pub advertise: bool,
    /// Whether peers in this group are trustable.
    pub trustable: bool,
    /// Requested hot peer count.
    pub hot_valency: u16,
    /// Requested warm peer count after defaulting.
    pub warm_valency: u16,
    /// Group diffusion mode.
    pub diffusion_mode: crate::peer_selection::PeerDiffusionMode,
}

/// Resolved public-root sets.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedPublicRootPeers {
    /// Configured bootstrap peers, after removing overlaps with local roots.
    pub bootstrap_peers: Vec<SocketAddr>,
    /// Configured public root peers, after removing overlaps with local and
    /// bootstrap peers.
    pub public_config_peers: Vec<SocketAddr>,
}

impl ResolvedPublicRootPeers {
    /// All resolved public-root peers in bootstrap-before-public order.
    pub fn all_peers(&self) -> Vec<SocketAddr> {
        let mut peers = self.bootstrap_peers.clone();
        peers.extend(self.public_config_peers.iter().copied());
        peers
    }
}

/// Resolved provider snapshot for root peers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootPeerProviders {
    /// Resolved local roots grouped by topology group.
    pub local_roots: Vec<ResolvedLocalRootGroup>,
    /// Resolved public roots split by source.
    pub public_roots: ResolvedPublicRootPeers,
    /// Ledger-peer gating policy.
    pub use_ledger_peers: UseLedgerPeers,
    /// Optional peer snapshot file.
    pub peer_snapshot_file: Option<String>,
}

impl RootPeerProviders {
    /// Ordered candidate peers with upstream-style precedence.
    pub fn ordered_candidates(&self) -> Vec<SocketAddr> {
        let mut ordered = self.public_roots.bootstrap_peers.clone();

        for group in self.local_roots.iter().filter(|group| group.trustable) {
            extend_unique(&mut ordered, &group.peers);
        }

        for group in self.local_roots.iter().filter(|group| !group.trustable) {
            extend_unique(&mut ordered, &group.peers);
        }

        extend_unique(&mut ordered, &self.public_roots.public_config_peers);
        ordered
    }

    /// Ordered fallback peers after a chosen primary peer.
    pub fn ordered_fallback_peers(&self, primary_peer: SocketAddr) -> Vec<SocketAddr> {
        self.ordered_candidates()
            .into_iter()
            .filter(|peer| *peer != primary_peer)
            .collect()
    }
}

/// Mutable root-provider state for time-varying root sources.
///
/// This is the next layer above static topology parsing: it holds the current
/// resolved root snapshot and lets future DNS- or ledger-backed provider code
/// replace local or public root results while preserving the same invariants
/// as startup resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootPeerProviderState {
    providers: RootPeerProviders,
}

impl RootPeerProviderState {
    /// Create provider state from an already reconciled provider snapshot.
    pub fn from_providers(providers: RootPeerProviders) -> Self {
        Self { providers }
    }

    /// Create provider state from a topology configuration.
    pub fn from_topology(topology: &TopologyConfig) -> Self {
        Self::from_providers(topology.resolved_root_providers())
    }

    /// Current reconciled provider snapshot.
    pub fn providers(&self) -> &RootPeerProviders {
        &self.providers
    }

    /// Replace the state from a new topology configuration.
    ///
    /// Returns `true` when the reconciled provider snapshot changed.
    pub fn replace_topology(&mut self, topology: &TopologyConfig) -> bool {
        self.replace_providers(topology.resolved_root_providers())
    }

    /// Replace the resolved local-root groups.
    ///
    /// Returns `true` when the reconciled provider snapshot changed.
    pub fn replace_local_roots(&mut self, local_roots: Vec<ResolvedLocalRootGroup>) -> bool {
        let next = reconcile_root_peer_providers(
            local_roots,
            self.providers.public_roots.clone(),
            self.providers.use_ledger_peers,
            self.providers.peer_snapshot_file.clone(),
        );
        self.replace_providers(next)
    }

    /// Replace the resolved public-root peers.
    ///
    /// Returns `true` when the reconciled provider snapshot changed.
    pub fn replace_public_roots(&mut self, public_roots: ResolvedPublicRootPeers) -> bool {
        let next = reconcile_root_peer_providers(
            self.providers.local_roots.clone(),
            public_roots,
            self.providers.use_ledger_peers,
            self.providers.peer_snapshot_file.clone(),
        );
        self.replace_providers(next)
    }

    /// Replace only the bootstrap peer portion of the resolved public roots.
    ///
    /// Returns `true` when the reconciled provider snapshot changed.
    pub fn replace_bootstrap_peers(&mut self, bootstrap_peers: Vec<SocketAddr>) -> bool {
        self.replace_public_roots(ResolvedPublicRootPeers {
            bootstrap_peers,
            public_config_peers: self.providers.public_roots.public_config_peers.clone(),
        })
    }

    /// Replace only the configured public-root portion of the resolved public
    /// roots.
    ///
    /// Returns `true` when the reconciled provider snapshot changed.
    pub fn replace_public_config_peers(&mut self, public_config_peers: Vec<SocketAddr>) -> bool {
        self.replace_public_roots(ResolvedPublicRootPeers {
            bootstrap_peers: self.providers.public_roots.bootstrap_peers.clone(),
            public_config_peers,
        })
    }

    /// Apply a provider refresh to the current state.
    pub fn apply_refresh(&mut self, refresh: RootPeerProviderRefresh) -> bool {
        match refresh {
            RootPeerProviderRefresh::Topology(topology) => self.replace_topology(&topology),
            RootPeerProviderRefresh::LocalRoots(local_roots) => {
                self.replace_local_roots(local_roots)
            }
            RootPeerProviderRefresh::BootstrapPeers(bootstrap_peers) => {
                self.replace_bootstrap_peers(bootstrap_peers)
            }
            RootPeerProviderRefresh::PublicConfigPeers(public_config_peers) => {
                self.replace_public_config_peers(public_config_peers)
            }
            RootPeerProviderRefresh::PublicRoots(public_roots) => {
                self.replace_public_roots(public_roots)
            }
        }
    }

    fn replace_providers(&mut self, next: RootPeerProviders) -> bool {
        if self.providers == next {
            false
        } else {
            self.providers = next;
            true
        }
    }
}

/// Resolve configured roots into a provider snapshot.
pub fn resolve_root_peer_providers(
    bootstrap_peers: &[PeerAccessPoint],
    local_roots: &[LocalRootConfig],
    public_roots: &[PublicRootConfig],
    use_ledger_peers: UseLedgerPeers,
    peer_snapshot_file: Option<String>,
) -> RootPeerProviders {
    let resolved_local_roots = local_roots
        .iter()
        .map(|group| {
            let mut peers = Vec::new();
            for access_point in &group.access_points {
                if let Some(addr) = resolve_access_point(access_point) {
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
        .collect::<Vec<_>>();

    let mut resolved_bootstrap = Vec::new();
    for access_point in bootstrap_peers {
        if let Some(addr) = resolve_access_point(access_point) {
            push_unique(&mut resolved_bootstrap, addr);
        }
    }

    let mut resolved_public = Vec::new();
    for group in public_roots {
        for access_point in &group.access_points {
            if let Some(addr) = resolve_access_point(access_point) {
                push_unique(&mut resolved_public, addr);
            }
        }
    }

    reconcile_root_peer_providers(
        resolved_local_roots,
        ResolvedPublicRootPeers {
            bootstrap_peers: resolved_bootstrap,
            public_config_peers: resolved_public,
        },
        use_ledger_peers,
        peer_snapshot_file,
    )
}

/// Reconcile already-resolved local and public roots into a canonical provider
/// snapshot.
pub fn reconcile_root_peer_providers(
    local_roots: Vec<ResolvedLocalRootGroup>,
    public_roots: ResolvedPublicRootPeers,
    use_ledger_peers: UseLedgerPeers,
    peer_snapshot_file: Option<String>,
) -> RootPeerProviders {
    let reconciled_local_roots = reconcile_local_root_groups(local_roots);
    let local_root_set = collect_local_root_peers(&reconciled_local_roots);

    let mut bootstrap_peers = Vec::new();
    for peer in public_roots.bootstrap_peers {
        if !local_root_set.contains(&peer) {
            push_unique(&mut bootstrap_peers, peer);
        }
    }

    let mut public_config_peers = Vec::new();
    for peer in public_roots.public_config_peers {
        if !local_root_set.contains(&peer) && !bootstrap_peers.contains(&peer) {
            push_unique(&mut public_config_peers, peer);
        }
    }

    RootPeerProviders {
        local_roots: reconciled_local_roots,
        public_roots: ResolvedPublicRootPeers {
            bootstrap_peers,
            public_config_peers,
        },
        use_ledger_peers,
        peer_snapshot_file,
    }
}

fn reconcile_local_root_groups(
    local_roots: Vec<ResolvedLocalRootGroup>,
) -> Vec<ResolvedLocalRootGroup> {
    let mut seen = Vec::new();

    local_roots
        .into_iter()
        .map(|group| {
            let mut peers = Vec::new();
            for peer in group.peers {
                if !seen.contains(&peer) {
                    peers.push(peer);
                    seen.push(peer);
                }
            }

            ResolvedLocalRootGroup { peers, ..group }
        })
        .collect()
}

fn collect_local_root_peers(local_roots: &[ResolvedLocalRootGroup]) -> Vec<SocketAddr> {
    let mut peers = Vec::new();
    for group in local_roots {
        extend_unique(&mut peers, &group.peers);
    }
    peers
}

fn resolve_access_point(access_point: &PeerAccessPoint) -> Option<SocketAddr> {
    format!("{}:{}", access_point.address, access_point.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
}

fn extend_unique(out: &mut Vec<SocketAddr>, peers: &[SocketAddr]) {
    for peer in peers {
        push_unique(out, *peer);
    }
}

fn push_unique(out: &mut Vec<SocketAddr>, peer: SocketAddr) {
    if !out.contains(&peer) {
        out.push(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_selection::PeerDiffusionMode;

    #[test]
    fn use_ledger_peers_parses_upstream_numeric_encoding() {
        assert_eq!(
            serde_json::from_str::<UseLedgerPeers>("-1").expect("parse"),
            UseLedgerPeers::DontUseLedgerPeers
        );
        assert_eq!(
            serde_json::from_str::<UseLedgerPeers>("0").expect("parse"),
            UseLedgerPeers::UseLedgerPeers(AfterSlot::Always)
        );
        assert_eq!(
            serde_json::from_str::<UseLedgerPeers>("42").expect("parse"),
            UseLedgerPeers::UseLedgerPeers(AfterSlot::After(42))
        );
    }

    #[test]
    fn use_bootstrap_peers_parses_null_or_access_points() {
        assert_eq!(
            serde_json::from_str::<UseBootstrapPeers>("null").expect("parse"),
            UseBootstrapPeers::DontUseBootstrapPeers
        );
        assert_eq!(
            serde_json::from_str::<UseBootstrapPeers>(r#"[{"address":"127.0.0.10","port":3001}]"#)
                .expect("parse"),
            UseBootstrapPeers::UseBootstrapPeers(vec![PeerAccessPoint {
                address: "127.0.0.10".to_owned(),
                port: 3001,
            }])
        );
    }

    #[test]
    fn topology_config_parses_upstream_fields() {
        let parsed: TopologyConfig = serde_json::from_str(
            r#"{
                "bootstrapPeers": [{"address":"127.0.0.10","port":3001}],
                "localRoots": [],
                "publicRoots": [],
                "useLedgerAfterSlot": 0,
                "peerSnapshotFile": "peer-snapshot.json"
            }"#,
        )
        .expect("parse topology");

        assert_eq!(
            parsed.bootstrap_peers,
            UseBootstrapPeers::UseBootstrapPeers(vec![PeerAccessPoint {
                address: "127.0.0.10".to_owned(),
                port: 3001,
            }])
        );
        assert_eq!(
            parsed.use_ledger_peers,
            UseLedgerPeers::UseLedgerPeers(AfterSlot::Always)
        );
        assert_eq!(
            parsed.peer_snapshot_file.as_deref(),
            Some("peer-snapshot.json")
        );
    }

    #[test]
    fn resolved_root_providers_enforce_local_and_bootstrap_public_precedence() {
        let providers = resolve_root_peer_providers(
            &[
                PeerAccessPoint {
                    address: "127.0.0.10".to_owned(),
                    port: 3001,
                },
                PeerAccessPoint {
                    address: "127.0.0.11".to_owned(),
                    port: 3001,
                },
            ],
            &[
                LocalRootConfig {
                    access_points: vec![
                        PeerAccessPoint {
                            address: "127.0.0.11".to_owned(),
                            port: 3001,
                        },
                        PeerAccessPoint {
                            address: "127.0.0.12".to_owned(),
                            port: 3001,
                        },
                    ],
                    advertise: false,
                    trustable: true,
                    hot_valency: 1,
                    warm_valency: None,
                    diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
                },
                LocalRootConfig {
                    access_points: vec![PeerAccessPoint {
                        address: "127.0.0.13".to_owned(),
                        port: 3001,
                    }],
                    advertise: false,
                    trustable: false,
                    hot_valency: 1,
                    warm_valency: Some(1),
                    diffusion_mode: PeerDiffusionMode::InitiatorOnlyDiffusionMode,
                },
            ],
            &[PublicRootConfig {
                access_points: vec![
                    PeerAccessPoint {
                        address: "127.0.0.10".to_owned(),
                        port: 3001,
                    },
                    PeerAccessPoint {
                        address: "127.0.0.12".to_owned(),
                        port: 3001,
                    },
                    PeerAccessPoint {
                        address: "127.0.0.14".to_owned(),
                        port: 3001,
                    },
                ],
                advertise: false,
            }],
            UseLedgerPeers::UseLedgerPeers(AfterSlot::After(99)),
            Some("peer-snapshot.json".to_owned()),
        );

        assert_eq!(
            providers.public_roots.bootstrap_peers,
            vec!["127.0.0.10:3001".parse().expect("addr")]
        );
        assert_eq!(
            providers.public_roots.public_config_peers,
            vec!["127.0.0.14:3001".parse().expect("addr")]
        );
        assert_eq!(
            providers.ordered_candidates(),
            vec![
                "127.0.0.10:3001".parse().expect("addr"),
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
                "127.0.0.13:3001".parse().expect("addr"),
                "127.0.0.14:3001".parse().expect("addr"),
            ]
        );
        assert_eq!(providers.use_ledger_peers.to_after_slot(), Some(99));
        assert_eq!(
            providers.peer_snapshot_file.as_deref(),
            Some("peer-snapshot.json")
        );
    }

    #[test]
    fn provider_state_reconciles_dynamic_public_root_updates() {
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
        assert!(state.replace_public_roots(ResolvedPublicRootPeers {
            bootstrap_peers: vec![
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ],
            public_config_peers: vec![
                "127.0.0.10:3001".parse().expect("addr"),
                "127.0.0.13:3001".parse().expect("addr"),
            ],
        }));

        assert_eq!(
            state.providers().public_roots.bootstrap_peers,
            vec!["127.0.0.12:3001".parse().expect("addr")]
        );
        assert_eq!(
            state.providers().public_roots.public_config_peers,
            vec![
                "127.0.0.10:3001".parse().expect("addr"),
                "127.0.0.13:3001".parse().expect("addr")
            ]
        );
    }

    #[test]
    fn provider_state_reconciles_dynamic_local_root_updates() {
        let mut state = RootPeerProviderState {
            providers: RootPeerProviders {
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
                    public_config_peers: vec![
                        "127.0.0.12:3001".parse().expect("addr"),
                        "127.0.0.13:3001".parse().expect("addr"),
                    ],
                },
                use_ledger_peers: UseLedgerPeers::DontUseLedgerPeers,
                peer_snapshot_file: None,
            },
        };

        assert!(state.replace_local_roots(vec![
            ResolvedLocalRootGroup {
                peers: vec![
                    "127.0.0.12:3001".parse().expect("addr"),
                    "127.0.0.14:3001".parse().expect("addr"),
                ],
                advertise: false,
                trustable: true,
                hot_valency: 1,
                warm_valency: 1,
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            },
            ResolvedLocalRootGroup {
                peers: vec![
                    "127.0.0.14:3001".parse().expect("addr"),
                    "127.0.0.15:3001".parse().expect("addr"),
                ],
                advertise: false,
                trustable: false,
                hot_valency: 1,
                warm_valency: 1,
                diffusion_mode: PeerDiffusionMode::InitiatorOnlyDiffusionMode,
            },
        ]));

        assert_eq!(
            state.providers().local_roots[0].peers,
            vec![
                "127.0.0.12:3001".parse().expect("addr"),
                "127.0.0.14:3001".parse().expect("addr"),
            ]
        );
        assert_eq!(
            state.providers().local_roots[1].peers,
            vec!["127.0.0.15:3001".parse().expect("addr")]
        );
        assert_eq!(
            state.providers().public_roots.public_config_peers,
            vec!["127.0.0.13:3001".parse().expect("addr")]
        );
    }

    #[test]
    fn provider_state_replace_topology_updates_policy_fields() {
        let mut state = RootPeerProviderState::from_topology(&TopologyConfig::default());
        assert!(state.replace_topology(&TopologyConfig {
            bootstrap_peers: UseBootstrapPeers::DontUseBootstrapPeers,
            local_roots: vec![],
            public_roots: vec![],
            use_ledger_peers: UseLedgerPeers::UseLedgerPeers(AfterSlot::After(77)),
            peer_snapshot_file: Some("peer-snapshot.json".to_owned()),
        }));

        assert_eq!(state.providers().use_ledger_peers.to_after_slot(), Some(77));
        assert_eq!(
            state.providers().peer_snapshot_file.as_deref(),
            Some("peer-snapshot.json")
        );
        assert!(!state.replace_topology(&TopologyConfig {
            bootstrap_peers: UseBootstrapPeers::DontUseBootstrapPeers,
            local_roots: vec![],
            public_roots: vec![],
            use_ledger_peers: UseLedgerPeers::UseLedgerPeers(AfterSlot::After(77)),
            peer_snapshot_file: Some("peer-snapshot.json".to_owned()),
        }));
    }
}
