//! Peer candidate resolution helpers for runtime bootstrap and future
//! peer-selection policy work.
//!
//! This module intentionally stays below any governor-style state machine. It
//! provides deterministic candidate ordering and hostname resolution so node
//! runtime code can stay focused on orchestration.

use std::net::{SocketAddr, ToSocketAddrs};

use serde::{Deserialize, Serialize};

use crate::root_peers::{UseLedgerPeers, resolve_root_peer_providers};

/// A hostname or IP address plus port for an outbound peer candidate.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerAccessPoint {
    /// DNS name or IP address.
    pub address: String,
    /// TCP port.
    pub port: u16,
}

/// Diffusion mode for a locally configured root peer group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PeerDiffusionMode {
    /// Dial peers in the group and accept inbound connections from them.
    #[default]
    InitiatorAndResponderDiffusionMode,
    /// Dial peers in the group but do not expect inbound diffusion from them.
    InitiatorOnlyDiffusionMode,
}

/// A locally configured root peer group.
///
/// This mirrors the official `TopologyP2P` split more closely than a flat
/// valency-only group: local roots carry trustability and diffusion-mode
/// semantics in addition to their access points.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRootConfig {
    /// Ordered access points within the group.
    #[serde(default)]
    pub access_points: Vec<PeerAccessPoint>,
    /// Whether peers in this group should be advertised to others.
    #[serde(default)]
    pub advertise: bool,
    /// Whether peers in this group are trusted bootstrap candidates.
    #[serde(default)]
    pub trustable: bool,
    /// Desired number of hot peers for the group.
    #[serde(default, rename = "hotValency", alias = "valency")]
    pub hot_valency: u16,
    /// Desired number of warm peers for the group.
    #[serde(default, rename = "warmValency", skip_serializing_if = "Option::is_none")]
    pub warm_valency: Option<u16>,
    /// Diffusion mode for the group.
    #[serde(default, rename = "diffusionMode")]
    pub diffusion_mode: PeerDiffusionMode,
}

impl LocalRootConfig {
    /// Effective warm valency, defaulting to the hot valency when the topology
    /// omits the explicit warm target.
    pub fn effective_warm_valency(&self) -> u16 {
        self.warm_valency.unwrap_or(self.hot_valency)
    }
}

/// A public root peer group.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicRootConfig {
    /// Ordered access points within the group.
    #[serde(default)]
    pub access_points: Vec<PeerAccessPoint>,
    /// Whether peers in this group should be advertised to others.
    #[serde(default)]
    pub advertise: bool,
}

/// Ordered bootstrap targets consisting of a primary peer and optional
/// fallback peers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerBootstrapTargets {
    primary_peer: SocketAddr,
    fallback_peers: Vec<SocketAddr>,
}

/// Mutable peer attempt state for repeated bootstrap or reconnect loops.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerAttemptState {
    targets: PeerBootstrapTargets,
    preferred_peer: Option<SocketAddr>,
}

impl PeerBootstrapTargets {
    /// Construct bootstrap targets with duplicate fallback peers removed while
    /// preserving order.
    pub fn new(primary_peer: SocketAddr, fallback_peers: &[SocketAddr]) -> Self {
        let mut unique_fallbacks = Vec::with_capacity(fallback_peers.len());
        for peer in fallback_peers {
            if *peer != primary_peer && !unique_fallbacks.contains(peer) {
                unique_fallbacks.push(*peer);
            }
        }

        Self {
            primary_peer,
            fallback_peers: unique_fallbacks,
        }
    }

    /// The primary peer attempted first.
    pub fn primary_peer(&self) -> SocketAddr {
        self.primary_peer
    }

    /// Ordered fallback peers attempted after the primary peer.
    pub fn fallback_peers(&self) -> &[SocketAddr] {
        &self.fallback_peers
    }

    /// Ordered candidate peers including the primary peer followed by unique
    /// fallback peers.
    pub fn candidate_peer_addrs(&self) -> Vec<SocketAddr> {
        let mut candidates = Vec::with_capacity(1 + self.fallback_peers.len());
        candidates.push(self.primary_peer);
        candidates.extend(self.fallback_peers.iter().copied());
        candidates
    }

    /// Ordered candidate peers, optionally preferring a previously successful
    /// peer first while preserving the remaining stable order.
    pub fn attempt_order(&self, preferred_peer: Option<SocketAddr>) -> Vec<SocketAddr> {
        let candidates = self.candidate_peer_addrs();

        match preferred_peer {
            Some(preferred) if candidates.contains(&preferred) => {
                let mut ordered = Vec::with_capacity(candidates.len());
                ordered.push(preferred);
                ordered.extend(candidates.into_iter().filter(|peer| *peer != preferred));
                ordered
            }
            _ => candidates,
        }
    }
}

impl PeerAttemptState {
    /// Create attempt state from stable bootstrap targets.
    pub fn new(targets: PeerBootstrapTargets) -> Self {
        Self {
            targets,
            preferred_peer: None,
        }
    }

    /// Stable bootstrap targets tracked by this attempt state.
    pub fn targets(&self) -> &PeerBootstrapTargets {
        &self.targets
    }

    /// Currently preferred peer, typically the most recent successful peer.
    pub fn preferred_peer(&self) -> Option<SocketAddr> {
        self.preferred_peer
    }

    /// Ordered candidate peers for the next bootstrap attempt.
    pub fn attempt_order(&self) -> Vec<SocketAddr> {
        self.targets.attempt_order(self.preferred_peer)
    }

    /// Record a successful peer so the next reconnect attempt can prefer it.
    pub fn record_success(&mut self, peer_addr: SocketAddr) {
        self.preferred_peer = Some(peer_addr);
    }
}

/// Resolve a single access point to the first usable socket address.
pub fn resolve_peer_access_point(access_point: &PeerAccessPoint) -> Option<SocketAddr> {
    resolve_peer_access_points(access_point).into_iter().next()
}

/// Resolve an access point to all usable socket addresses in stable order.
pub fn resolve_peer_access_points(access_point: &PeerAccessPoint) -> Vec<SocketAddr> {
    let mut resolved = Vec::new();

    if let Ok(addrs) = format!("{}:{}", access_point.address, access_point.port).to_socket_addrs() {
        for addr in addrs {
            if !resolved.contains(&addr) {
                resolved.push(addr);
            }
        }
    }

    resolved
}

/// Resolve ordered candidate peers from topology-style groups.
///
/// Ordering mirrors the upstream topology split used by the node runtime:
/// bootstrap peers first, then trustable local roots, then non-trustable local
/// roots, and finally public roots.
pub fn ordered_peer_candidates(
    bootstrap_peers: &[PeerAccessPoint],
    local_roots: &[LocalRootConfig],
    public_roots: &[PublicRootConfig],
) -> Vec<SocketAddr> {
    resolve_root_peer_providers(
        bootstrap_peers,
        local_roots,
        public_roots,
        UseLedgerPeers::DontUseLedgerPeers,
        None,
    )
    .ordered_candidates()
}

/// Build ordered fallback peers after a chosen primary peer.
pub fn ordered_fallback_peers(
    primary_peer: SocketAddr,
    bootstrap_peers: &[SocketAddr],
    local_roots: &[LocalRootConfig],
    public_roots: &[PublicRootConfig],
) -> Vec<SocketAddr> {
    let bootstrap_access_points = bootstrap_peers
        .iter()
        .map(|addr| PeerAccessPoint {
            address: addr.ip().to_string(),
            port: addr.port(),
        })
        .collect::<Vec<_>>();

    resolve_root_peer_providers(
        &bootstrap_access_points,
        local_roots,
        public_roots,
        UseLedgerPeers::DontUseLedgerPeers,
        None,
    )
    .ordered_fallback_peers(primary_peer)
}

/// Build ordered bootstrap targets from a primary peer and ordered fallbacks.
pub fn bootstrap_targets(
    primary_peer: SocketAddr,
    fallback_peers: &[SocketAddr],
) -> PeerBootstrapTargets {
    PeerBootstrapTargets::new(primary_peer, fallback_peers)
}

/// Build reusable attempt state from a primary peer and ordered fallbacks.
pub fn peer_attempt_state(
    primary_peer: SocketAddr,
    fallback_peers: &[SocketAddr],
) -> PeerAttemptState {
    PeerAttemptState::new(bootstrap_targets(primary_peer, fallback_peers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_candidates_prefer_bootstrap_then_trustable_then_public() {
        let bootstrap = vec![
            PeerAccessPoint {
                address: "127.0.0.10".to_owned(),
                port: 3001,
            },
            PeerAccessPoint {
                address: "127.0.0.11".to_owned(),
                port: 3001,
            },
        ];
        let local = vec![
            LocalRootConfig {
                access_points: vec![PeerAccessPoint {
                    address: "127.0.0.12".to_owned(),
                    port: 3001,
                }],
                advertise: false,
                trustable: false,
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
                trustable: true,
                hot_valency: 1,
                warm_valency: None,
                diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
            },
        ];
        let public = vec![PublicRootConfig {
            access_points: vec![PeerAccessPoint {
                address: "127.0.0.14".to_owned(),
                port: 3001,
            }],
            advertise: false,
        }];

        assert_eq!(
            ordered_peer_candidates(&bootstrap, &local, &public),
            vec![
                "127.0.0.10:3001".parse().expect("addr"),
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.13:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
                "127.0.0.14:3001".parse().expect("addr"),
            ]
        );
    }

    #[test]
    fn ordered_fallbacks_drop_primary_and_duplicates() {
        let primary: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let bootstrap = vec![primary, "127.0.0.11:3001".parse().expect("addr")];
        let local = vec![LocalRootConfig {
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
        }];

        assert_eq!(
            ordered_fallback_peers(primary, &bootstrap, &local, &[]),
            vec![
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ]
        );
    }

    #[test]
    fn bootstrap_targets_dedup_fallbacks_and_keep_primary_first() {
        let primary: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let targets = PeerBootstrapTargets::new(
            primary,
            &[
                primary,
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ],
        );

        assert_eq!(targets.primary_peer(), primary);
        assert_eq!(
            targets.fallback_peers(),
            &[
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ]
        );
        assert_eq!(
            targets.candidate_peer_addrs(),
            vec![
                primary,
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ]
        );
    }

    #[test]
    fn bootstrap_targets_attempt_order_prefers_successful_peer_when_present() {
        let primary: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let targets = PeerBootstrapTargets::new(
            primary,
            &[
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ],
        );

        assert_eq!(
            targets.attempt_order(Some("127.0.0.12:3001".parse().expect("addr"))),
            vec![
                "127.0.0.12:3001".parse().expect("addr"),
                primary,
                "127.0.0.11:3001".parse().expect("addr"),
            ]
        );
        assert_eq!(
            targets.attempt_order(Some("127.0.0.99:3001".parse().expect("addr"))),
            targets.candidate_peer_addrs()
        );
    }

    #[test]
    fn peer_attempt_state_tracks_preferred_peer() {
        let primary: SocketAddr = "127.0.0.10:3001".parse().expect("addr");
        let mut state = peer_attempt_state(
            primary,
            &[
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ],
        );

        assert_eq!(state.preferred_peer(), None);
        assert_eq!(
            state.attempt_order(),
            vec![
                primary,
                "127.0.0.11:3001".parse().expect("addr"),
                "127.0.0.12:3001".parse().expect("addr"),
            ]
        );

        state.record_success("127.0.0.12:3001".parse().expect("addr"));
        assert_eq!(
            state.attempt_order(),
            vec![
                "127.0.0.12:3001".parse().expect("addr"),
                primary,
                "127.0.0.11:3001".parse().expect("addr"),
            ]
        );
    }

    #[test]
    fn local_root_config_parses_legacy_valency_as_hot_valency() {
        let group: LocalRootConfig = serde_json::from_str(
            r#"{
                "accessPoints": [{ "address": "127.0.0.1", "port": 3001 }],
                "advertise": false,
                "trustable": true,
                "valency": 2
            }"#,
        )
        .expect("parse local root");

        assert_eq!(group.hot_valency, 2);
        assert_eq!(group.effective_warm_valency(), 2);
        assert_eq!(
            group.diffusion_mode,
            PeerDiffusionMode::InitiatorAndResponderDiffusionMode
        );
    }

    #[test]
    fn local_root_config_parses_explicit_upstream_fields() {
        let group: LocalRootConfig = serde_json::from_str(
            r#"{
                "accessPoints": [{ "address": "127.0.0.1", "port": 3001 }],
                "advertise": true,
                "trustable": true,
                "hotValency": 2,
                "warmValency": 4,
                "diffusionMode": "InitiatorOnlyDiffusionMode"
            }"#,
        )
        .expect("parse local root");

        assert_eq!(group.hot_valency, 2);
        assert_eq!(group.effective_warm_valency(), 4);
        assert_eq!(group.diffusion_mode, PeerDiffusionMode::InitiatorOnlyDiffusionMode);
    }
}