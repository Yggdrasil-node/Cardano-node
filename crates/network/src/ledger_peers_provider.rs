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
use crate::root_peers::{AfterSlot, UseLedgerPeers};

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

/// Consensus-facing judgement about whether the current ledger view is usable
/// for ledger peer selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LedgerStateJudgement {
    /// Ledger state is recent enough to use for ledger peer selection.
    #[default]
    YoungEnough,
    /// Ledger state is available but too old to trust for ledger peers.
    TooOld,
    /// No authoritative ledger-state judgement is currently available.
    Unavailable,
}

/// Freshness state for the optional peer snapshot input referenced by
/// topology configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PeerSnapshotFreshness {
    /// No peer snapshot file is configured, so no freshness gate applies.
    #[default]
    NotConfigured,
    /// The configured peer snapshot is present and fresh enough to use.
    Fresh,
    /// The configured peer snapshot file exists but is stale.
    Stale,
    /// A peer snapshot file is configured, but no freshness judgement is
    /// currently available yet.
    Awaiting,
    /// A peer snapshot file is configured but unavailable or unreadable.
    Unavailable,
}

/// Decision describing whether ledger peers are currently eligible for use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerPeerUseDecision {
    /// Topology disabled ledger peers entirely.
    Disabled,
    /// Ledger peers are enabled, but a latest observed slot is still needed.
    AwaitingLatestSlot { after_slot: u64 },
    /// Latest observed slot has not yet crossed the configured gate.
    BeforeUseLedgerAfterSlot { after_slot: u64, latest_slot: u64 },
    /// Ledger peers are blocked by the current ledger-state judgement.
    BlockedByLedgerState { judgement: LedgerStateJudgement },
    /// Ledger peers are waiting on a configured peer snapshot to become usable.
    AwaitingPeerSnapshot,
    /// Ledger peers are blocked by the configured peer snapshot state.
    BlockedByPeerSnapshot { freshness: PeerSnapshotFreshness },
    /// Ledger peers are eligible and may be reconciled into the registry.
    Eligible,
}

/// Result of applying ledger-peer policy to the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerPeerRegistryUpdate {
    /// Policy decision that was applied.
    pub decision: LedgerPeerUseDecision,
    /// Whether the registry changed as a result.
    pub changed: bool,
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

/// Judge whether ledger peers are currently eligible according to topology,
/// latest-slot gating, and consensus ledger-state judgement.
pub fn judge_ledger_peer_usage(
    use_ledger_peers: UseLedgerPeers,
    latest_slot: Option<u64>,
    ledger_state_judgement: LedgerStateJudgement,
    peer_snapshot_freshness: PeerSnapshotFreshness,
) -> LedgerPeerUseDecision {
    match use_ledger_peers {
        UseLedgerPeers::DontUseLedgerPeers => LedgerPeerUseDecision::Disabled,
        UseLedgerPeers::UseLedgerPeers(AfterSlot::Always) => {
            if ledger_state_judgement != LedgerStateJudgement::YoungEnough {
                LedgerPeerUseDecision::BlockedByLedgerState {
                    judgement: ledger_state_judgement,
                }
            } else {
                judge_peer_snapshot_freshness(peer_snapshot_freshness)
            }
        }
        UseLedgerPeers::UseLedgerPeers(AfterSlot::After(after_slot)) => {
            let Some(latest_slot) = latest_slot else {
                return LedgerPeerUseDecision::AwaitingLatestSlot { after_slot };
            };

            if latest_slot < after_slot {
                return LedgerPeerUseDecision::BeforeUseLedgerAfterSlot {
                    after_slot,
                    latest_slot,
                };
            }

            if ledger_state_judgement != LedgerStateJudgement::YoungEnough {
                LedgerPeerUseDecision::BlockedByLedgerState {
                    judgement: ledger_state_judgement,
                }
            } else {
                judge_peer_snapshot_freshness(peer_snapshot_freshness)
            }
        }
    }
}

fn judge_peer_snapshot_freshness(
    peer_snapshot_freshness: PeerSnapshotFreshness,
) -> LedgerPeerUseDecision {
    match peer_snapshot_freshness {
        PeerSnapshotFreshness::NotConfigured | PeerSnapshotFreshness::Fresh => {
            LedgerPeerUseDecision::Eligible
        }
        PeerSnapshotFreshness::Awaiting => LedgerPeerUseDecision::AwaitingPeerSnapshot,
        PeerSnapshotFreshness::Stale | PeerSnapshotFreshness::Unavailable => {
            LedgerPeerUseDecision::BlockedByPeerSnapshot {
                freshness: peer_snapshot_freshness,
            }
        }
    }
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

/// Apply ledger peers to the registry only when the current policy judgement
/// allows them. Blocked decisions clear crate-owned ledger and big-ledger
/// sources while preserving unrelated peer sources and status.
pub fn reconcile_ledger_peer_registry_with_policy(
    registry: &mut PeerRegistry,
    snapshot: LedgerPeerSnapshot,
    use_ledger_peers: UseLedgerPeers,
    latest_slot: Option<u64>,
    ledger_state_judgement: LedgerStateJudgement,
    peer_snapshot_freshness: PeerSnapshotFreshness,
) -> LedgerPeerRegistryUpdate {
    let decision = judge_ledger_peer_usage(
        use_ledger_peers,
        latest_slot,
        ledger_state_judgement,
        peer_snapshot_freshness,
    );

    let changed = match decision {
        LedgerPeerUseDecision::Eligible => apply_ledger_peer_refresh(
            registry,
            LedgerPeerProviderRefresh::Combined(snapshot),
        ),
        LedgerPeerUseDecision::Disabled
        | LedgerPeerUseDecision::AwaitingLatestSlot { .. }
        | LedgerPeerUseDecision::BeforeUseLedgerAfterSlot { .. }
        | LedgerPeerUseDecision::BlockedByLedgerState { .. }
        | LedgerPeerUseDecision::AwaitingPeerSnapshot
        | LedgerPeerUseDecision::BlockedByPeerSnapshot { .. } => {
            let ledger_changed = registry.sync_ledger_peers(Vec::new());
            let big_ledger_changed = registry.sync_big_ledger_peers(Vec::new());
            ledger_changed || big_ledger_changed
        }
    };

    LedgerPeerRegistryUpdate { decision, changed }
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
    use crate::root_peers::AfterSlot;

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

    #[test]
    fn judge_ledger_peer_usage_rejects_disabled_policy() {
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::DontUseLedgerPeers,
                Some(100),
                LedgerStateJudgement::YoungEnough,
                PeerSnapshotFreshness::NotConfigured,
            ),
            LedgerPeerUseDecision::Disabled
        );
    }

    #[test]
    fn judge_ledger_peer_usage_waits_for_latest_slot_when_thresholded() {
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::After(500)),
                None,
                LedgerStateJudgement::YoungEnough,
                PeerSnapshotFreshness::NotConfigured,
            ),
            LedgerPeerUseDecision::AwaitingLatestSlot { after_slot: 500 }
        );
    }

    #[test]
    fn judge_ledger_peer_usage_blocks_before_after_slot() {
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::After(500)),
                Some(499),
                LedgerStateJudgement::YoungEnough,
                PeerSnapshotFreshness::NotConfigured,
            ),
            LedgerPeerUseDecision::BeforeUseLedgerAfterSlot {
                after_slot: 500,
                latest_slot: 499,
            }
        );
    }

    #[test]
    fn judge_ledger_peer_usage_requires_young_enough_ledger_state() {
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
                None,
                LedgerStateJudgement::TooOld,
                PeerSnapshotFreshness::NotConfigured,
            ),
            LedgerPeerUseDecision::BlockedByLedgerState {
                judgement: LedgerStateJudgement::TooOld,
            }
        );
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
                None,
                LedgerStateJudgement::Unavailable,
                PeerSnapshotFreshness::NotConfigured,
            ),
            LedgerPeerUseDecision::BlockedByLedgerState {
                judgement: LedgerStateJudgement::Unavailable,
            }
        );
    }

    #[test]
    fn judge_ledger_peer_usage_blocks_on_peer_snapshot_state() {
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
                None,
                LedgerStateJudgement::YoungEnough,
                PeerSnapshotFreshness::Awaiting,
            ),
            LedgerPeerUseDecision::AwaitingPeerSnapshot
        );
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
                None,
                LedgerStateJudgement::YoungEnough,
                PeerSnapshotFreshness::Stale,
            ),
            LedgerPeerUseDecision::BlockedByPeerSnapshot {
                freshness: PeerSnapshotFreshness::Stale,
            }
        );
        assert_eq!(
            judge_ledger_peer_usage(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
                None,
                LedgerStateJudgement::YoungEnough,
                PeerSnapshotFreshness::Unavailable,
            ),
            LedgerPeerUseDecision::BlockedByPeerSnapshot {
                freshness: PeerSnapshotFreshness::Unavailable,
            }
        );
    }

    #[test]
    fn reconcile_ledger_peer_registry_with_policy_applies_snapshot_when_eligible() {
        let ledger_peer: SocketAddr = "127.0.0.30:3001".parse().expect("addr");
        let big_ledger_peer: SocketAddr = "127.0.0.31:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        let update = reconcile_ledger_peer_registry_with_policy(
            &mut registry,
            LedgerPeerSnapshot::new([ledger_peer], [big_ledger_peer]),
            UseLedgerPeers::UseLedgerPeers(AfterSlot::After(10)),
            Some(10),
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::NotConfigured,
        );

        assert_eq!(update.decision, LedgerPeerUseDecision::Eligible);
        assert!(update.changed);
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
    fn reconcile_ledger_peer_registry_with_policy_clears_blocked_sources_only() {
        let ledger_peer: SocketAddr = "127.0.0.32:3001".parse().expect("addr");
        let big_ledger_peer: SocketAddr = "127.0.0.33:3001".parse().expect("addr");
        let shared_peer: SocketAddr = "127.0.0.34:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(ledger_peer, PeerSource::PeerSourceLedger);
        registry.insert_source(big_ledger_peer, PeerSource::PeerSourceBigLedger);
        registry.insert_source(shared_peer, PeerSource::PeerSourceLedger);
        registry.insert_source(shared_peer, PeerSource::PeerSourcePeerShare);
        registry.set_status(shared_peer, PeerStatus::PeerWarm);

        let update = reconcile_ledger_peer_registry_with_policy(
            &mut registry,
            LedgerPeerSnapshot::new([ledger_peer], [big_ledger_peer]),
            UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
            Some(99),
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::NotConfigured,
        );

        assert_eq!(
            update.decision,
            LedgerPeerUseDecision::BeforeUseLedgerAfterSlot {
                after_slot: 100,
                latest_slot: 99,
            }
        );
        assert!(update.changed);
        assert!(registry.get(&ledger_peer).is_none());
        assert!(registry.get(&big_ledger_peer).is_none());

        let shared_entry = registry.get(&shared_peer).expect("shared peer remains");
        assert_eq!(
            shared_entry.sources,
            [PeerSource::PeerSourcePeerShare].into_iter().collect()
        );
        assert_eq!(shared_entry.status, PeerStatus::PeerWarm);
    }

    #[test]
    fn reconcile_ledger_peer_registry_with_policy_clears_ledger_sources_when_snapshot_is_stale() {
        let ledger_peer: SocketAddr = "127.0.0.35:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(ledger_peer, PeerSource::PeerSourceLedger);
        registry.insert_source(ledger_peer, PeerSource::PeerSourcePeerShare);
        registry.set_status(ledger_peer, PeerStatus::PeerHot);

        let update = reconcile_ledger_peer_registry_with_policy(
            &mut registry,
            LedgerPeerSnapshot::new([ledger_peer], Vec::<SocketAddr>::new()),
            UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
            None,
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::Stale,
        );

        assert_eq!(
            update.decision,
            LedgerPeerUseDecision::BlockedByPeerSnapshot {
                freshness: PeerSnapshotFreshness::Stale,
            }
        );
        assert!(update.changed);

        let entry = registry.get(&ledger_peer).expect("peer share remains");
        assert_eq!(
            entry.sources,
            [PeerSource::PeerSourcePeerShare].into_iter().collect()
        );
        assert_eq!(entry.status, PeerStatus::PeerHot);
    }
}