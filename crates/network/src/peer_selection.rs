//! Peer candidate resolution helpers for runtime bootstrap and future
//! peer-selection policy work.
//!
//! This module intentionally stays below any governor-style state machine. It
//! provides deterministic candidate ordering and hostname resolution so node
//! runtime code can stay focused on orchestration.

use std::net::{SocketAddr, ToSocketAddrs};

/// A hostname or IP address plus port for an outbound peer candidate.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeerAccessPoint {
    /// DNS name or IP address.
    pub address: String,
    /// TCP port.
    pub port: u16,
}

/// A topology group of access points sharing trust and advertisement flags.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeerRootGroup {
    /// Ordered access points within the group.
    pub access_points: Vec<PeerAccessPoint>,
    /// Whether peers in this group should be advertised to others.
    pub advertise: bool,
    /// Whether peers in this group are trusted bootstrap candidates.
    pub trustable: bool,
    /// Requested outbound valency for the group.
    pub valency: Option<u16>,
}

/// Ordered bootstrap targets consisting of a primary peer and optional
/// fallback peers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerBootstrapTargets {
    primary_peer: SocketAddr,
    fallback_peers: Vec<SocketAddr>,
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

/// Resolve a single access point to the first usable socket address.
pub fn resolve_peer_access_point(access_point: &PeerAccessPoint) -> Option<SocketAddr> {
    format!("{}:{}", access_point.address, access_point.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
}

/// Resolve ordered candidate peers from topology-style groups.
///
/// Ordering mirrors the upstream topology split used by the node runtime:
/// bootstrap peers first, then trustable local roots, then non-trustable local
/// roots, and finally public roots.
pub fn ordered_peer_candidates(
    bootstrap_peers: &[PeerAccessPoint],
    local_roots: &[PeerRootGroup],
    public_roots: &[PeerRootGroup],
) -> Vec<SocketAddr> {
    let mut ordered = Vec::new();

    for peer in bootstrap_peers {
        if let Some(addr) = resolve_peer_access_point(peer) {
            push_unique_addr(&mut ordered, addr);
        }
    }

    for group in local_roots.iter().filter(|group| group.trustable) {
        extend_group(&mut ordered, group);
    }

    for group in local_roots.iter().filter(|group| !group.trustable) {
        extend_group(&mut ordered, group);
    }

    for group in public_roots {
        extend_group(&mut ordered, group);
    }

    ordered
}

/// Build ordered fallback peers after a chosen primary peer.
pub fn ordered_fallback_peers(
    primary_peer: SocketAddr,
    bootstrap_peers: &[SocketAddr],
    local_roots: &[PeerRootGroup],
    public_roots: &[PeerRootGroup],
) -> Vec<SocketAddr> {
    let mut ordered = bootstrap_peers.to_vec();

    for group in local_roots.iter().filter(|group| group.trustable) {
        extend_group(&mut ordered, group);
    }

    for group in local_roots.iter().filter(|group| !group.trustable) {
        extend_group(&mut ordered, group);
    }

    for group in public_roots {
        extend_group(&mut ordered, group);
    }

    ordered.retain(|addr| *addr != primary_peer);
    ordered.dedup();
    ordered
}

/// Build ordered bootstrap targets from a primary peer and ordered fallbacks.
pub fn bootstrap_targets(
    primary_peer: SocketAddr,
    fallback_peers: &[SocketAddr],
) -> PeerBootstrapTargets {
    PeerBootstrapTargets::new(primary_peer, fallback_peers)
}

fn extend_group(addrs: &mut Vec<SocketAddr>, group: &PeerRootGroup) {
    for access_point in &group.access_points {
        if let Some(addr) = resolve_peer_access_point(access_point) {
            push_unique_addr(addrs, addr);
        }
    }
}

fn push_unique_addr(addrs: &mut Vec<SocketAddr>, addr: SocketAddr) {
    if !addrs.contains(&addr) {
        addrs.push(addr);
    }
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
            PeerRootGroup {
                access_points: vec![PeerAccessPoint {
                    address: "127.0.0.12".to_owned(),
                    port: 3001,
                }],
                advertise: false,
                trustable: false,
                valency: Some(1),
            },
            PeerRootGroup {
                access_points: vec![PeerAccessPoint {
                    address: "127.0.0.13".to_owned(),
                    port: 3001,
                }],
                advertise: false,
                trustable: true,
                valency: Some(1),
            },
        ];
        let public = vec![PeerRootGroup {
            access_points: vec![PeerAccessPoint {
                address: "127.0.0.14".to_owned(),
                port: 3001,
            }],
            advertise: false,
            trustable: false,
            valency: None,
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
        let local = vec![PeerRootGroup {
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
            valency: Some(1),
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
}