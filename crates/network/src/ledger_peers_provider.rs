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

/// Inputs required to derive a [`LedgerStateJudgement`] from the current
/// ledger tip's age relative to wall-clock time.
///
/// All fields are seconds since the Unix epoch except `tip_slot`, which is
/// the absolute slot number of the most recently applied block. When any
/// of `system_start_unix_secs`, `slot_length_secs`, or `tip_slot` is
/// missing, the judgement falls back to [`LedgerStateJudgement::Unavailable`]
/// — the same conservative behavior the runtime exhibits when ledger
/// recovery itself fails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedgerStateAgeInputs {
    /// Absolute slot of the latest applied block, or `None` when the
    /// node has no recovered tip yet (genesis startup).
    pub tip_slot: Option<u64>,
    /// Seconds since the Unix epoch of the network genesis moment, parsed
    /// from `ShelleyGenesis.system_start`. `None` disables wall-clock
    /// derivation.
    pub system_start_unix_secs: Option<f64>,
    /// Slot duration in seconds from `ShelleyGenesis.slot_length`. `None`
    /// disables wall-clock derivation.
    pub slot_length_secs: Option<f64>,
    /// Maximum acceptable age in seconds before the judgement flips to
    /// [`LedgerStateJudgement::TooOld`]. Upstream uses `stabilityWindow *
    /// slotLength` (≈ `3k/f * slotLength` for Praos), so on mainnet this
    /// is ≈ `3 * 2160 / 0.05 * 1.0 ≈ 129 600 s` (36 h) by default.
    pub max_age_secs: f64,
    /// Wall-clock "now" as seconds since the Unix epoch. Passed in so the
    /// helper stays pure / unit-testable.
    pub now_unix_secs: f64,
}

/// Derives a [`LedgerStateJudgement`] by comparing the ledger tip's
/// wall-clock arrival time to the current time.
///
/// Mirrors upstream `Cardano.Node.Diffusion.Configuration` `mkLedgerStateJudgement`,
/// where the judgement is `TooOld` when `now - tipSlotTime > maxLedgerStateAge`
/// and `YoungEnough` otherwise. Returns [`LedgerStateJudgement::Unavailable`]
/// when any of the wall-clock inputs is missing or the inputs imply a
/// negative or non-finite age.
///
/// References:
/// * [`Cardano.Node.Diffusion.Configuration`](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/src/Cardano/Node/Diffusion)
/// * [`Ouroboros.Consensus.HardFork.Combinator.Ledger`](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus)
pub fn judge_ledger_state_age(inputs: LedgerStateAgeInputs) -> LedgerStateJudgement {
    let (Some(tip_slot), Some(start), Some(slot_len)) = (
        inputs.tip_slot,
        inputs.system_start_unix_secs,
        inputs.slot_length_secs,
    ) else {
        return LedgerStateJudgement::Unavailable;
    };
    if !slot_len.is_finite() || slot_len <= 0.0 || !inputs.max_age_secs.is_finite() {
        return LedgerStateJudgement::Unavailable;
    }
    let tip_unix_secs = start + (tip_slot as f64) * slot_len;
    let age_secs = inputs.now_unix_secs - tip_unix_secs;
    if !age_secs.is_finite() {
        return LedgerStateJudgement::Unavailable;
    }
    if age_secs > inputs.max_age_secs {
        LedgerStateJudgement::TooOld
    } else {
        LedgerStateJudgement::YoungEnough
    }
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

/// Observation emitted by one live ledger-peer refresh tick.
///
/// Carries the policy update together with the consensus-fed inputs used for
/// that decision so callers can reuse a single authoritative observation per
/// tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveLedgerPeerRefreshObservation {
    /// Registry update produced by policy reconciliation.
    pub update: LedgerPeerRegistryUpdate,
    /// Latest slot observed from the consensus source.
    pub latest_slot: Option<u64>,
    /// Consensus judgement for whether the ledger view is usable.
    pub judgement: LedgerStateJudgement,
    /// Freshness judgement for the optional snapshot-file overlay.
    pub peer_snapshot_freshness: PeerSnapshotFreshness,
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

/// Merge a live ledger snapshot with an optional peer-snapshot-file overlay.
///
/// Overlay peers are appended in-order while preserving uniqueness, and the
/// final snapshot is normalized so big-ledger peers remain disjoint from the
/// ledger-peer set.
pub fn merge_ledger_peer_snapshots(
    ledger_snapshot: &LedgerPeerSnapshot,
    snapshot_overlay: Option<LedgerPeerSnapshot>,
) -> LedgerPeerSnapshot {
    let mut merged_ledger_peers = ledger_snapshot.ledger_peers.clone();
    let mut merged_big_ledger_peers = ledger_snapshot.big_ledger_peers.clone();

    if let Some(snapshot_overlay) = snapshot_overlay {
        for peer in snapshot_overlay.ledger_peers {
            if !merged_ledger_peers.contains(&peer) {
                merged_ledger_peers.push(peer);
            }
        }

        for peer in snapshot_overlay.big_ledger_peers {
            if !merged_big_ledger_peers.contains(&peer) {
                merged_big_ledger_peers.push(peer);
            }
        }
    }

    LedgerPeerSnapshot::new(merged_ledger_peers, merged_big_ledger_peers)
}

/// Derive peer-snapshot freshness from policy, snapshot presence, and the
/// latest observed slot.
///
/// This logic belongs with ledger-peer policy because freshness is part of the
/// decision of whether snapshot-backed ledger peers may participate in peer
/// selection, not a configuration-parsing concern.
pub fn derive_peer_snapshot_freshness(
    use_ledger_peers: UseLedgerPeers,
    snapshot_configured: bool,
    snapshot_slot: Option<u64>,
    latest_slot: Option<u64>,
    snapshot_available: bool,
) -> PeerSnapshotFreshness {
    if !snapshot_configured {
        return PeerSnapshotFreshness::NotConfigured;
    }

    if !snapshot_available {
        return PeerSnapshotFreshness::Unavailable;
    }

    match use_ledger_peers {
        UseLedgerPeers::DontUseLedgerPeers | UseLedgerPeers::UseLedgerPeers(AfterSlot::Always) => {
            PeerSnapshotFreshness::Fresh
        }
        UseLedgerPeers::UseLedgerPeers(AfterSlot::After(after_slot)) => {
            let Some(latest_slot) = latest_slot else {
                return PeerSnapshotFreshness::Awaiting;
            };

            if latest_slot < after_slot {
                return PeerSnapshotFreshness::Awaiting;
            }

            match snapshot_slot {
                Some(snapshot_slot) if snapshot_slot >= after_slot => PeerSnapshotFreshness::Fresh,
                Some(_) => PeerSnapshotFreshness::Stale,
                None => PeerSnapshotFreshness::Unavailable,
            }
        }
    }
}

/// Return currently eligible ledger-derived peer candidates while excluding
/// peers already covered by other bootstrap or reconnect sources.
///
/// The returned order preserves the normalized snapshot order: ledger peers
/// first, then big-ledger peers.
pub fn eligible_ledger_peer_candidates(
    snapshot: &LedgerPeerSnapshot,
    blocked_peers: &[SocketAddr],
    use_ledger_peers: UseLedgerPeers,
    latest_slot: Option<u64>,
    ledger_state_judgement: LedgerStateJudgement,
    peer_snapshot_freshness: PeerSnapshotFreshness,
) -> (LedgerPeerUseDecision, Vec<SocketAddr>) {
    let decision = judge_ledger_peer_usage(
        use_ledger_peers,
        latest_slot,
        ledger_state_judgement,
        peer_snapshot_freshness,
    );

    if decision != LedgerPeerUseDecision::Eligible {
        return (decision, Vec::new());
    }

    let mut eligible = Vec::new();
    for peer in snapshot
        .ledger_peers
        .iter()
        .chain(snapshot.big_ledger_peers.iter())
        .copied()
    {
        if !blocked_peers.contains(&peer) && !eligible.contains(&peer) {
            eligible.push(peer);
        }
    }

    (decision, eligible)
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
            let snapshot =
                LedgerPeerSnapshot::new(snapshot.ledger_peers, snapshot.big_ledger_peers);
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
        LedgerPeerUseDecision::Eligible => {
            apply_ledger_peer_refresh(registry, LedgerPeerProviderRefresh::Combined(snapshot))
        }
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

/// Authoritative inputs observed from the consensus/storage layer for one
/// ledger-peer refresh tick.
///
/// Mirrors the upstream `Ouroboros.Network.PeerSelection.LedgerPeers.Type`
/// `LedgerPeersConsensusInterface` shape: a bundle of `(ledgerStateJudgement,
/// latestSlotNo, ledgerPeerSnapshot)` derived from the live ledger view, fed
/// into the network-owned ledger-peer provider.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConsensusLedgerPeerInputs {
    /// The latest slot observed on the consensus chain tip, when known.
    pub latest_slot: Option<u64>,
    /// Whether the current ledger view is recent enough for ledger peers.
    pub judgement: LedgerStateJudgement,
    /// The ledger-derived peer snapshot extracted from the current view.
    pub ledger_snapshot: LedgerPeerSnapshot,
}

/// Result of polling a configured peer-snapshot file source.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeerSnapshotFileObservation {
    /// Whether a snapshot file is configured at all.
    pub configured: bool,
    /// Whether the snapshot file was successfully observed this tick.
    pub available: bool,
    /// Slot recorded inside the snapshot file, when present.
    pub snapshot_slot: Option<u64>,
    /// The snapshot read from the file, when one was loaded.
    pub overlay: Option<LedgerPeerSnapshot>,
}

impl PeerSnapshotFileObservation {
    /// Convenience constructor for "no snapshot file is configured".
    pub fn not_configured() -> Self {
        Self {
            configured: false,
            available: true,
            snapshot_slot: None,
            overlay: None,
        }
    }

    /// Convenience constructor for "snapshot configured but unavailable".
    pub fn unavailable() -> Self {
        Self {
            configured: true,
            available: false,
            snapshot_slot: None,
            overlay: None,
        }
    }

    /// Convenience constructor for a successfully loaded snapshot.
    pub fn loaded(slot: Option<u64>, overlay: LedgerPeerSnapshot) -> Self {
        Self {
            configured: true,
            available: true,
            snapshot_slot: slot,
            overlay: Some(overlay),
        }
    }
}

/// A live consensus-fed source of ledger-peer inputs.
///
/// Implementations bridge the network crate's ledger-peer provider layer into
/// the consensus/storage layer without making the network crate depend on any
/// concrete storage type. The node provides a `ChainDb`-backed implementation
/// at runtime; tests use scripted implementations.
pub trait ConsensusLedgerPeerSource {
    /// Observe the current ledger-peer inputs from the consensus layer.
    fn observe(&mut self) -> ConsensusLedgerPeerInputs;
}

/// A live source for the optional `peerSnapshotFile` overlay.
pub trait PeerSnapshotFileSource {
    /// Poll the peer-snapshot file for a fresh observation.
    fn observe(&mut self) -> PeerSnapshotFileObservation;
}

/// Run one live ledger-peer refresh tick using the supplied consensus-fed
/// sources, then reconcile the resulting snapshot into the peer registry under
/// the configured topology policy.
///
/// This is the network-owned orchestration entry point that replaces inline
/// node-side bookkeeping; the node only supplies trait implementations bridged
/// to its concrete storage and configuration layers.
pub fn live_refresh_ledger_peer_registry<C, S>(
    registry: &mut PeerRegistry,
    use_ledger_peers: UseLedgerPeers,
    consensus_source: &mut C,
    snapshot_source: &mut S,
) -> LedgerPeerRegistryUpdate
where
    C: ConsensusLedgerPeerSource + ?Sized,
    S: PeerSnapshotFileSource + ?Sized,
{
    live_refresh_ledger_peer_registry_observed(
        registry,
        use_ledger_peers,
        consensus_source,
        snapshot_source,
    )
    .update
}

/// Run one live ledger-peer refresh tick and return both the registry update
/// and the consensus-fed observations used to compute it.
pub fn live_refresh_ledger_peer_registry_observed<C, S>(
    registry: &mut PeerRegistry,
    use_ledger_peers: UseLedgerPeers,
    consensus_source: &mut C,
    snapshot_source: &mut S,
) -> LiveLedgerPeerRefreshObservation
where
    C: ConsensusLedgerPeerSource + ?Sized,
    S: PeerSnapshotFileSource + ?Sized,
{
    if !use_ledger_peers.enabled() {
        return LiveLedgerPeerRefreshObservation {
            update: LedgerPeerRegistryUpdate {
                decision: LedgerPeerUseDecision::Disabled,
                changed: false,
            },
            latest_slot: None,
            judgement: LedgerStateJudgement::Unavailable,
            peer_snapshot_freshness: PeerSnapshotFreshness::NotConfigured,
        };
    }

    let consensus_inputs = consensus_source.observe();
    let snapshot_observation = snapshot_source.observe();

    let merged_snapshot =
        merge_ledger_peer_snapshots(&consensus_inputs.ledger_snapshot, snapshot_observation.overlay);

    let peer_snapshot_freshness = derive_peer_snapshot_freshness(
        use_ledger_peers,
        snapshot_observation.configured,
        snapshot_observation.snapshot_slot,
        consensus_inputs.latest_slot,
        snapshot_observation.available,
    );

    let update = reconcile_ledger_peer_registry_with_policy(
        registry,
        merged_snapshot,
        use_ledger_peers,
        consensus_inputs.latest_slot,
        consensus_inputs.judgement,
        peer_snapshot_freshness,
    );

    LiveLedgerPeerRefreshObservation {
        update,
        latest_slot: consensus_inputs.latest_slot,
        judgement: consensus_inputs.judgement,
        peer_snapshot_freshness,
    }
}

/// In-memory scripted provider useful for tests and early integration.
#[derive(Clone, Debug)]
pub struct ScriptedLedgerPeerProvider {
    kind: LedgerPeerProviderKind,
    scripted_refreshes:
        VecDeque<Result<Option<LedgerPeerProviderRefresh>, LedgerPeerProviderError>>,
}

impl ScriptedLedgerPeerProvider {
    /// Create a provider from scripted refresh results.
    pub fn new(
        kind: LedgerPeerProviderKind,
        scripted_refreshes: impl IntoIterator<
            Item = Result<Option<LedgerPeerProviderRefresh>, LedgerPeerProviderError>,
        >,
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

        let snapshot =
            LedgerPeerSnapshot::new([shared, ledger_only, shared], [shared, big_only, big_only]);

        assert_eq!(snapshot.ledger_peers, vec![shared, ledger_only]);
        assert_eq!(snapshot.big_ledger_peers, vec![big_only]);
    }

    #[test]
    fn merge_ledger_peer_snapshots_appends_overlay_uniquely() {
        let base_ledger: SocketAddr = "127.0.0.40:3001".parse().expect("addr");
        let overlay_ledger: SocketAddr = "127.0.0.41:3001".parse().expect("addr");
        let overlay_big: SocketAddr = "127.0.0.42:3001".parse().expect("addr");

        let merged = merge_ledger_peer_snapshots(
            &LedgerPeerSnapshot::new([base_ledger], Vec::<SocketAddr>::new()),
            Some(LedgerPeerSnapshot::new(
                [base_ledger, overlay_ledger],
                [overlay_ledger, overlay_big],
            )),
        );

        assert_eq!(merged.ledger_peers, vec![base_ledger, overlay_ledger]);
        assert_eq!(merged.big_ledger_peers, vec![overlay_big]);
    }

    #[test]
    fn derive_peer_snapshot_freshness_waits_for_latest_slot_before_gate() {
        assert_eq!(
            derive_peer_snapshot_freshness(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
                true,
                Some(100),
                None,
                true,
            ),
            PeerSnapshotFreshness::Awaiting
        );
        assert_eq!(
            derive_peer_snapshot_freshness(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
                true,
                Some(100),
                Some(99),
                true,
            ),
            PeerSnapshotFreshness::Awaiting
        );
    }

    #[test]
    fn derive_peer_snapshot_freshness_marks_old_snapshot_stale_after_gate() {
        assert_eq!(
            derive_peer_snapshot_freshness(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
                true,
                Some(99),
                Some(100),
                true,
            ),
            PeerSnapshotFreshness::Stale
        );
        assert_eq!(
            derive_peer_snapshot_freshness(
                UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
                true,
                Some(100),
                Some(100),
                true,
            ),
            PeerSnapshotFreshness::Fresh
        );
    }

    #[test]
    fn eligible_ledger_peer_candidates_filters_blocked_peers() {
        let primary: SocketAddr = "127.0.0.50:3001".parse().expect("addr");
        let blocked_fallback: SocketAddr = "127.0.0.51:3001".parse().expect("addr");
        let ledger_peer: SocketAddr = "127.0.0.52:3001".parse().expect("addr");
        let big_ledger_peer: SocketAddr = "127.0.0.53:3001".parse().expect("addr");

        let (decision, peers) = eligible_ledger_peer_candidates(
            &LedgerPeerSnapshot::new(
                [primary, blocked_fallback, ledger_peer],
                [blocked_fallback, big_ledger_peer],
            ),
            &[primary, blocked_fallback],
            UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
            Some(1),
            LedgerStateJudgement::YoungEnough,
            PeerSnapshotFreshness::Fresh,
        );

        assert_eq!(decision, LedgerPeerUseDecision::Eligible);
        assert_eq!(peers, vec![ledger_peer, big_ledger_peer]);
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
        assert_eq!(
            entry.sources,
            [PeerSource::PeerSourcePeerShare].into_iter().collect()
        );
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
        let mut provider =
            ScriptedLedgerPeerProvider::new(LedgerPeerProviderKind::Combined, [Ok(None)]);

        assert!(!refresh_ledger_peer_registry(&mut registry, &mut provider).expect("refresh"));
        assert!(registry.is_empty());
    }

    #[test]
    fn refresh_ledger_peer_registry_surfaces_provider_errors() {
        let mut registry = PeerRegistry::default();
        let mut provider = ScriptedLedgerPeerProvider::new(
            LedgerPeerProviderKind::Combined,
            [Err(LedgerPeerProviderError::RefreshFailed(
                "ledger snapshot unavailable".to_owned(),
            ))],
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

    #[derive(Default)]
    struct StaticConsensusSource {
        observations: VecDeque<ConsensusLedgerPeerInputs>,
        last: ConsensusLedgerPeerInputs,
    }

    impl ConsensusLedgerPeerSource for StaticConsensusSource {
        fn observe(&mut self) -> ConsensusLedgerPeerInputs {
            if let Some(next) = self.observations.pop_front() {
                self.last = next;
            }
            self.last.clone()
        }
    }

    #[derive(Default)]
    struct StaticSnapshotSource {
        observations: VecDeque<PeerSnapshotFileObservation>,
        last: PeerSnapshotFileObservation,
    }

    impl PeerSnapshotFileSource for StaticSnapshotSource {
        fn observe(&mut self) -> PeerSnapshotFileObservation {
            if let Some(next) = self.observations.pop_front() {
                self.last = next;
            }
            self.last.clone()
        }
    }

    #[test]
    fn live_refresh_short_circuits_when_ledger_peers_disabled() {
        let mut registry = PeerRegistry::default();
        let mut consensus_source = StaticConsensusSource::default();
        let mut snapshot_source = StaticSnapshotSource::default();

        let update = live_refresh_ledger_peer_registry(
            &mut registry,
            UseLedgerPeers::DontUseLedgerPeers,
            &mut consensus_source,
            &mut snapshot_source,
        );

        assert_eq!(update.decision, LedgerPeerUseDecision::Disabled);
        assert!(!update.changed);
    }

    #[test]
    fn live_refresh_reconciles_consensus_inputs_through_policy() {
        let ledger_peer: SocketAddr = "127.0.0.50:3001".parse().expect("addr");
        let big_ledger_peer: SocketAddr = "127.0.0.51:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();

        let mut consensus_source = StaticConsensusSource {
            observations: VecDeque::from([ConsensusLedgerPeerInputs {
                latest_slot: Some(200),
                judgement: LedgerStateJudgement::YoungEnough,
                ledger_snapshot: LedgerPeerSnapshot::new(
                    [ledger_peer],
                    [big_ledger_peer],
                ),
            }]),
            last: ConsensusLedgerPeerInputs::default(),
        };
        let mut snapshot_source = StaticSnapshotSource {
            observations: VecDeque::from([PeerSnapshotFileObservation::not_configured()]),
            last: PeerSnapshotFileObservation::default(),
        };

        let update = live_refresh_ledger_peer_registry(
            &mut registry,
            UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
            &mut consensus_source,
            &mut snapshot_source,
        );

        assert_eq!(update.decision, LedgerPeerUseDecision::Eligible);
        assert!(update.changed);
        assert!(registry.get(&ledger_peer).is_some());
        assert!(registry.get(&big_ledger_peer).is_some());
    }

    #[test]
    fn live_refresh_blocks_when_ledger_state_too_old() {
        let ledger_peer: SocketAddr = "127.0.0.52:3001".parse().expect("addr");
        let mut registry = PeerRegistry::default();
        registry.insert_source(ledger_peer, PeerSource::PeerSourceLedger);

        let mut consensus_source = StaticConsensusSource {
            observations: VecDeque::from([ConsensusLedgerPeerInputs {
                latest_slot: Some(100),
                judgement: LedgerStateJudgement::TooOld,
                ledger_snapshot: LedgerPeerSnapshot::new([ledger_peer], Vec::<SocketAddr>::new()),
            }]),
            last: ConsensusLedgerPeerInputs::default(),
        };
        let mut snapshot_source = StaticSnapshotSource {
            observations: VecDeque::from([PeerSnapshotFileObservation::not_configured()]),
            last: PeerSnapshotFileObservation::default(),
        };

        let update = live_refresh_ledger_peer_registry(
            &mut registry,
            UseLedgerPeers::UseLedgerPeers(AfterSlot::Always),
            &mut consensus_source,
            &mut snapshot_source,
        );

        assert_eq!(
            update.decision,
            LedgerPeerUseDecision::BlockedByLedgerState {
                judgement: LedgerStateJudgement::TooOld,
            }
        );
        assert!(update.changed);
        assert!(registry.get(&ledger_peer).is_none());
    }

    #[test]
    fn live_refresh_observed_returns_shared_consensus_judgement() {
        let mut registry = PeerRegistry::default();
        let mut consensus_source = StaticConsensusSource {
            observations: VecDeque::from([ConsensusLedgerPeerInputs {
                latest_slot: Some(777),
                judgement: LedgerStateJudgement::YoungEnough,
                ledger_snapshot: LedgerPeerSnapshot::default(),
            }]),
            last: ConsensusLedgerPeerInputs::default(),
        };
        let mut snapshot_source = StaticSnapshotSource {
            observations: VecDeque::from([PeerSnapshotFileObservation::not_configured()]),
            last: PeerSnapshotFileObservation::default(),
        };

        let observation = live_refresh_ledger_peer_registry_observed(
            &mut registry,
            UseLedgerPeers::UseLedgerPeers(AfterSlot::After(100)),
            &mut consensus_source,
            &mut snapshot_source,
        );

        assert_eq!(observation.latest_slot, Some(777));
        assert_eq!(observation.judgement, LedgerStateJudgement::YoungEnough);
        assert_eq!(
            observation.peer_snapshot_freshness,
            PeerSnapshotFreshness::NotConfigured
        );
        assert_eq!(observation.update.decision, LedgerPeerUseDecision::Eligible);
    }

    // ── judge_ledger_state_age ───────────────────────────────────────────

    /// Pins the upstream `mkLedgerStateJudgement` boundary: when the tip's
    /// wall-clock age is below `max_age_secs` the judgement is
    /// `YoungEnough`, when it exceeds the threshold it flips to `TooOld`.
    /// Reference: `Cardano.Node.Diffusion.Configuration.mkLedgerStateJudgement`.
    #[test]
    fn judge_ledger_state_age_flips_at_threshold() {
        // tip at slot 100, slot length 1.0 s, system start at unix 0.0 →
        // tipUnixSecs = 100. max_age = 60 s, now = 150 s → age = 50 s →
        // YoungEnough.
        let young = judge_ledger_state_age(LedgerStateAgeInputs {
            tip_slot: Some(100),
            system_start_unix_secs: Some(0.0),
            slot_length_secs: Some(1.0),
            max_age_secs: 60.0,
            now_unix_secs: 150.0,
        });
        assert_eq!(young, LedgerStateJudgement::YoungEnough);

        // Same tip, now = 200 → age = 100 s > 60 s → TooOld.
        let old = judge_ledger_state_age(LedgerStateAgeInputs {
            tip_slot: Some(100),
            system_start_unix_secs: Some(0.0),
            slot_length_secs: Some(1.0),
            max_age_secs: 60.0,
            now_unix_secs: 200.0,
        });
        assert_eq!(old, LedgerStateJudgement::TooOld);
    }

    /// Pins the inclusive `>=` boundary at exactly `now == tip + max_age`:
    /// upstream uses strict `>` so equality stays `YoungEnough`. Without
    /// this, a small refactor that flipped the comparator (`>` ↔ `>=`)
    /// would silently break BlockFetch concurrency around the tip.
    #[test]
    fn judge_ledger_state_age_boundary_is_strict_greater_than() {
        // age == max_age → still YoungEnough.
        let exact = judge_ledger_state_age(LedgerStateAgeInputs {
            tip_slot: Some(100),
            system_start_unix_secs: Some(0.0),
            slot_length_secs: Some(1.0),
            max_age_secs: 60.0,
            now_unix_secs: 160.0,
        });
        assert_eq!(exact, LedgerStateJudgement::YoungEnough);

        // age == max_age + 1 → TooOld.
        let just_over = judge_ledger_state_age(LedgerStateAgeInputs {
            tip_slot: Some(100),
            system_start_unix_secs: Some(0.0),
            slot_length_secs: Some(1.0),
            max_age_secs: 60.0,
            now_unix_secs: 161.0,
        });
        assert_eq!(just_over, LedgerStateJudgement::TooOld);
    }

    /// Missing wall-clock inputs (no genesis configured, no tip recovered
    /// yet) must return `Unavailable` so the governor falls back to
    /// `BulkSync` rather than incorrectly claiming the node is caught up.
    #[test]
    fn judge_ledger_state_age_returns_unavailable_for_missing_inputs() {
        let cases = [
            LedgerStateAgeInputs {
                tip_slot: None,
                system_start_unix_secs: Some(0.0),
                slot_length_secs: Some(1.0),
                max_age_secs: 60.0,
                now_unix_secs: 150.0,
            },
            LedgerStateAgeInputs {
                tip_slot: Some(100),
                system_start_unix_secs: None,
                slot_length_secs: Some(1.0),
                max_age_secs: 60.0,
                now_unix_secs: 150.0,
            },
            LedgerStateAgeInputs {
                tip_slot: Some(100),
                system_start_unix_secs: Some(0.0),
                slot_length_secs: None,
                max_age_secs: 60.0,
                now_unix_secs: 150.0,
            },
        ];
        for inputs in cases {
            assert_eq!(
                judge_ledger_state_age(inputs),
                LedgerStateJudgement::Unavailable
            );
        }
    }

    /// Pathological numeric inputs (NaN, ≤ 0 slot length, NaN max_age)
    /// must return `Unavailable` rather than producing a garbage
    /// `YoungEnough`/`TooOld` from arithmetic that does not make sense.
    #[test]
    fn judge_ledger_state_age_rejects_pathological_numerics() {
        let nan_max = judge_ledger_state_age(LedgerStateAgeInputs {
            tip_slot: Some(100),
            system_start_unix_secs: Some(0.0),
            slot_length_secs: Some(1.0),
            max_age_secs: f64::NAN,
            now_unix_secs: 150.0,
        });
        assert_eq!(nan_max, LedgerStateJudgement::Unavailable);

        let zero_slot_length = judge_ledger_state_age(LedgerStateAgeInputs {
            tip_slot: Some(100),
            system_start_unix_secs: Some(0.0),
            slot_length_secs: Some(0.0),
            max_age_secs: 60.0,
            now_unix_secs: 150.0,
        });
        assert_eq!(zero_slot_length, LedgerStateJudgement::Unavailable);
    }
}
