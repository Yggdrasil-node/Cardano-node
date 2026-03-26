//! Peer governor — promotion, demotion, and valency enforcement.
//!
//! The governor evaluates the current [`PeerRegistry`] state against
//! configured targets and produces [`GovernorAction`] decisions.  The
//! runtime executes those actions by connecting/disconnecting peers and
//! updating the registry.
//!
//! This follows the upstream Ouroboros design where the governor is a
//! pure decision function separated from effectful connection management.
//!
//! Reference: `Ouroboros.Network.PeerSelection.Governor`.

use crate::peer_registry::{PeerRegistry, PeerSource, PeerStatus};
use crate::peer_selection::LocalRootConfig;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Governor targets
// ---------------------------------------------------------------------------

/// Target peer counts that the governor tries to maintain.
///
/// Matches the upstream `PeerSelectionTargets` record in
/// `Ouroboros.Network.PeerSelection.Governor.Types`, which defines seven
/// fields split into *regular* and *big-ledger* categories.
///
/// **Upstream field mapping:**
///
/// | Upstream Haskell field                          | Rust field                             |
/// |-------------------------------------------------|----------------------------------------|
/// | `targetNumberOfRootPeers`                       | `target_root`                          |
/// | `targetNumberOfKnownPeers`                      | `target_known`                         |
/// | `targetNumberOfEstablishedPeers`                | `target_established`                   |
/// | `targetNumberOfActivePeers`                     | `target_active`                        |
/// | `targetNumberOfKnownBigLedgerPeers`             | `target_known_big_ledger`              |
/// | `targetNumberOfEstablishedBigLedgerPeers`       | `target_established_big_ledger`        |
/// | `targetNumberOfActiveBigLedgerPeers`            | `target_active_big_ledger`             |
///
/// The `target_root` field is a one-sided target (from below only): the
/// governor stops looking for more roots once reached but never shrinks
/// the set.  Regular targets (`target_known`, `target_established`,
/// `target_active`) are two-sided (the governor grows *and* shrinks).
/// Big-ledger targets operate independently and their counts do not
/// overlap with regular targets.
///
/// Reference: `Ouroboros.Network.PeerSelection.Governor.Types`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernorTargets {
    // -- Regular peer targets (excludes big-ledger) ---------------------------

    /// Target number of root peers (one-sided, from below only).
    ///
    /// Upstream: `targetNumberOfRootPeers`.
    pub target_root: usize,
    /// Target number of known (cold + warm + hot) peers.
    ///
    /// Upstream: `targetNumberOfKnownPeers`.
    pub target_known: usize,
    /// Target number of established (warm + hot) peers.
    ///
    /// Upstream: `targetNumberOfEstablishedPeers`.
    pub target_established: usize,
    /// Target number of active (hot) peers.
    ///
    /// Upstream: `targetNumberOfActivePeers`.
    pub target_active: usize,

    // -- Big-ledger peer targets (independent of regular) ---------------------

    /// Target number of known big-ledger peers.
    ///
    /// Upstream: `targetNumberOfKnownBigLedgerPeers`.
    pub target_known_big_ledger: usize,
    /// Target number of established big-ledger peers.
    ///
    /// Upstream: `targetNumberOfEstablishedBigLedgerPeers`.
    pub target_established_big_ledger: usize,
    /// Target number of active big-ledger peers.
    ///
    /// Upstream: `targetNumberOfActiveBigLedgerPeers`.
    pub target_active_big_ledger: usize,
}

impl GovernorTargets {
    /// Checks whether the targets satisfy the upstream `sanePeerSelectionTargets`
    /// invariants.
    ///
    /// The upstream Haskell implementation enforces:
    ///
    /// ```text
    /// 0 ≤ active ≤ established ≤ known
    /// 0 ≤ root ≤ known
    /// 0 ≤ active_big ≤ established_big ≤ known_big
    /// active ≤ 100, established ≤ 1000, known ≤ 10000
    /// active_big ≤ 100, established_big ≤ 1000, known_big ≤ 10000
    /// ```
    ///
    /// Reference: `sanePeerSelectionTargets` in
    /// `Ouroboros.Network.PeerSelection.Governor.Types`.
    pub fn is_sane(&self) -> bool {
        // Regular chain: 0 ≤ active ≤ established ≤ known, root ≤ known
        self.target_active <= self.target_established
            && self.target_established <= self.target_known
            && self.target_root <= self.target_known
            // Big-ledger chain: 0 ≤ active_big ≤ established_big ≤ known_big
            && self.target_active_big_ledger <= self.target_established_big_ledger
            && self.target_established_big_ledger <= self.target_known_big_ledger
            // Upper bounds (matching upstream constants)
            && self.target_active <= 100
            && self.target_established <= 1000
            && self.target_known <= 10000
            && self.target_active_big_ledger <= 100
            && self.target_established_big_ledger <= 1000
            && self.target_known_big_ledger <= 10000
    }
}

impl Default for GovernorTargets {
    fn default() -> Self {
        Self {
            target_root: 3,
            target_known: 20,
            target_established: 10,
            target_active: 5,
            target_known_big_ledger: 0,
            target_established_big_ledger: 0,
            target_active_big_ledger: 0,
        }
    }
}

/// Per-group governor targets derived from local root config.
#[derive(Clone, Debug)]
pub struct LocalRootTargets {
    /// Peers belonging to this local root group.
    pub peers: Vec<SocketAddr>,
    /// Desired hot (active) peer count for this group.
    pub hot_valency: u16,
    /// Desired warm (established) peer count for this group.
    pub warm_valency: u16,
}

impl LocalRootTargets {
    /// Build targets from a local root config and resolved peer addresses.
    pub fn from_config(config: &LocalRootConfig, resolved_peers: Vec<SocketAddr>) -> Self {
        Self {
            peers: resolved_peers,
            hot_valency: config.hot_valency,
            warm_valency: config.effective_warm_valency(),
        }
    }
}

// ---------------------------------------------------------------------------
// Governor state
// ---------------------------------------------------------------------------

/// Configurable churn parameters.
///
/// Upstream reference: `Ouroboros.Network.PeerSelection.Governor.ActivePeers`
/// — `policyChurnInterval` defaults 300 s for bulk sync and 200 s for
/// deadline mode.
#[derive(Clone, Debug)]
pub struct ChurnConfig {
    /// How often the governor selects a warm peer to demote.
    pub churn_interval: Duration,
}

impl Default for ChurnConfig {
    fn default() -> Self {
        Self {
            churn_interval: Duration::from_secs(300),
        }
    }
}

/// Mutable governor state carried across ticks.
///
/// Tracks connection failures and churn timing so that the governor can
/// back off from failing peers and periodically rotate the warm set.
#[derive(Clone, Debug)]
pub struct GovernorState {
    /// Consecutive connection failure count per peer.
    pub failures: BTreeMap<SocketAddr, u32>,
    /// Maximum failures before a peer is temporarily skipped.
    pub max_failures: u32,
    /// Base back-off per failure count.
    pub failure_backoff: Duration,
    /// Churn configuration.
    pub churn: ChurnConfig,
    /// When the last churn demotion happened.
    pub last_churn: Option<Instant>,
}

impl Default for GovernorState {
    fn default() -> Self {
        Self {
            failures: BTreeMap::new(),
            max_failures: 5,
            failure_backoff: Duration::from_secs(30),
            churn: ChurnConfig::default(),
            last_churn: None,
        }
    }
}

impl GovernorState {
    /// Record a successful connection to `peer`, resetting its failure count.
    pub fn record_success(&mut self, peer: SocketAddr) {
        self.failures.remove(&peer);
    }

    /// Record a connection failure for `peer`.
    pub fn record_failure(&mut self, peer: SocketAddr) {
        *self.failures.entry(peer).or_insert(0) += 1;
    }

    /// Return true if `peer` should be skipped due to recent failures.
    pub fn is_backing_off(&self, peer: &SocketAddr) -> bool {
        self.failures.get(peer).copied().unwrap_or(0) >= self.max_failures
    }

    /// Filter a list of governor actions, removing promotions for peers
    /// that are currently in the back-off window.
    pub fn filter_backed_off(&self, actions: Vec<GovernorAction>) -> Vec<GovernorAction> {
        actions
            .into_iter()
            .filter(|a| match a {
                GovernorAction::PromoteToWarm(addr)
                | GovernorAction::PromoteToHot(addr) => !self.is_backing_off(addr),
                _ => true,
            })
            .collect()
    }

    /// If the churn interval has elapsed, select one non-local-root warm
    /// peer for demotion to cold, cycling the warm set.
    ///
    /// Returns `Some(action)` if a churn demotion is needed.
    pub fn evaluate_churn(
        &mut self,
        registry: &PeerRegistry,
        now: Instant,
    ) -> Option<GovernorAction> {
        let due = match self.last_churn {
            Some(last) => now.duration_since(last) >= self.churn.churn_interval,
            None => true,
        };
        if !due {
            return None;
        }

        // Pick the first non-local-root warm peer for demotion.
        for (addr, entry) in registry.iter() {
            if entry.status == PeerStatus::PeerWarm
                && !entry.sources.contains(&PeerSource::PeerSourceLocalRoot)
            {
                self.last_churn = Some(now);
                return Some(GovernorAction::DemoteToCold(*addr));
            }
        }
        None
    }

    /// Run a full governance pass with churn and failure filtering.
    pub fn tick(
        &mut self,
        registry: &PeerRegistry,
        targets: &GovernorTargets,
        local_root_groups: &[LocalRootTargets],
        now: Instant,
    ) -> Vec<GovernorAction> {
        let mut actions = governor_tick(registry, targets, local_root_groups);
        actions = self.filter_backed_off(actions);

        if let Some(churn_action) = self.evaluate_churn(registry, now) {
            actions.push(churn_action);
        }

        actions
    }
}

// ---------------------------------------------------------------------------
// Governor actions
// ---------------------------------------------------------------------------

/// An action produced by the governor for the runtime to execute.
///
/// The governor never touches connections directly — it only emits
/// decisions.  The runtime loop processes these and updates the
/// [`PeerRegistry`] accordingly.
///
/// Reference: `Ouroboros.Network.PeerSelection.Governor.Types` —
/// `Decision` / `PeerSelectionActions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernorAction {
    /// Promote a cold peer to warm (establish a connection).
    PromoteToWarm(SocketAddr),
    /// Promote a warm peer to hot (activate data protocols).
    PromoteToHot(SocketAddr),
    /// Demote a hot peer to warm (deactivate data protocols).
    DemoteToWarm(SocketAddr),
    /// Demote a warm peer to cold (close the connection).
    DemoteToCold(SocketAddr),
}

// ---------------------------------------------------------------------------
// Evaluation helpers
// ---------------------------------------------------------------------------

/// Evaluate which cold peers should be promoted to warm to meet the
/// established peer target.
///
/// Returns promotion actions, choosing local-root peers first for
/// stability, then other cold peers.
pub fn evaluate_cold_to_warm_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = registry.status_counts();
    let established = counts.warm + counts.hot;
    if established >= targets.target_established {
        return Vec::new();
    }
    let needed = targets.target_established - established;

    // Collect cold peers, preferring local roots.
    let mut local_root_cold = Vec::new();
    let mut other_cold = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerCold {
            if entry.sources.contains(&PeerSource::PeerSourceLocalRoot) {
                local_root_cold.push(*addr);
            } else {
                other_cold.push(*addr);
            }
        }
    }

    let mut actions = Vec::new();
    for addr in local_root_cold.into_iter().chain(other_cold) {
        if actions.len() >= needed {
            break;
        }
        actions.push(GovernorAction::PromoteToWarm(addr));
    }
    actions
}

/// Evaluate which warm peers should be promoted to hot to meet the
/// active peer target.
///
/// Returns promotion actions, choosing local-root peers first.
pub fn evaluate_warm_to_hot_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = registry.status_counts();
    if counts.hot >= targets.target_active {
        return Vec::new();
    }
    let needed = targets.target_active - counts.hot;

    let mut local_root_warm = Vec::new();
    let mut other_warm = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerWarm {
            if entry.sources.contains(&PeerSource::PeerSourceLocalRoot) {
                local_root_warm.push(*addr);
            } else {
                other_warm.push(*addr);
            }
        }
    }

    let mut actions = Vec::new();
    for addr in local_root_warm.into_iter().chain(other_warm) {
        if actions.len() >= needed {
            break;
        }
        actions.push(GovernorAction::PromoteToHot(addr));
    }
    actions
}

/// Evaluate which hot peers should be demoted to warm because we have
/// more active peers than the target.
///
/// Prefers demoting non-local-root peers first.
pub fn evaluate_hot_to_warm_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = registry.status_counts();
    if counts.hot <= targets.target_active {
        return Vec::new();
    }
    let excess = counts.hot - targets.target_active;

    // Collect hot peers, preferring to demote non-local-root first.
    let mut non_local_hot = Vec::new();
    let mut local_hot = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerHot {
            if entry.sources.contains(&PeerSource::PeerSourceLocalRoot) {
                local_hot.push(*addr);
            } else {
                non_local_hot.push(*addr);
            }
        }
    }

    let mut actions = Vec::new();
    for addr in non_local_hot.into_iter().chain(local_hot) {
        if actions.len() >= excess {
            break;
        }
        actions.push(GovernorAction::DemoteToWarm(addr));
    }
    actions
}

/// Evaluate which warm peers should be demoted to cold because we have
/// more established peers than the target.
///
/// Prefers demoting non-local-root peers first.
pub fn evaluate_warm_to_cold_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = registry.status_counts();
    let established = counts.warm + counts.hot;
    if established <= targets.target_established {
        return Vec::new();
    }
    let excess = established - targets.target_established;

    let mut non_local_warm = Vec::new();
    let mut local_warm = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerWarm {
            if entry.sources.contains(&PeerSource::PeerSourceLocalRoot) {
                local_warm.push(*addr);
            } else {
                non_local_warm.push(*addr);
            }
        }
    }

    let mut actions = Vec::new();
    for addr in non_local_warm.into_iter().chain(local_warm) {
        if actions.len() >= excess {
            break;
        }
        actions.push(GovernorAction::DemoteToCold(addr));
    }
    actions
}

// ---------------------------------------------------------------------------
// Local root valency enforcement
// ---------------------------------------------------------------------------

/// Check local root group valency targets and produce actions to meet them.
///
/// For each local root group, ensures at least `hot_valency` peers are hot
/// and at least `warm_valency` peers are warm (including hot).  Promotes
/// cold→warm and warm→hot as needed within each group.
pub fn enforce_local_root_valency(
    registry: &PeerRegistry,
    groups: &[LocalRootTargets],
) -> Vec<GovernorAction> {
    let mut actions = Vec::new();

    for group in groups {
        let mut warm_count = 0u16;
        let mut hot_count = 0u16;
        let mut cold_peers = Vec::new();
        let mut warm_peers = Vec::new();

        for addr in &group.peers {
            if let Some(entry) = registry.get(addr) {
                match entry.status {
                    PeerStatus::PeerHot => {
                        hot_count += 1;
                        warm_count += 1; // hot counts as established
                    }
                    PeerStatus::PeerWarm => {
                        warm_count += 1;
                        warm_peers.push(*addr);
                    }
                    PeerStatus::PeerCold => {
                        cold_peers.push(*addr);
                    }
                    PeerStatus::PeerCooling => {}
                }
            }
        }

        // Promote cold→warm until we meet warm_valency.
        if warm_count < group.warm_valency {
            let needed = (group.warm_valency - warm_count) as usize;
            for addr in cold_peers.iter().take(needed) {
                actions.push(GovernorAction::PromoteToWarm(*addr));
            }
        }

        // Promote warm→hot until we meet hot_valency.
        if hot_count < group.hot_valency {
            let needed = (group.hot_valency - hot_count) as usize;
            for addr in warm_peers.iter().take(needed) {
                actions.push(GovernorAction::PromoteToHot(*addr));
            }
        }
    }

    actions
}

// ---------------------------------------------------------------------------
// Governor tick — combined evaluation
// ---------------------------------------------------------------------------

/// Run one governance evaluation pass, returning all actions needed to
/// converge toward the configured targets.
///
/// Actions are ordered: local-root valency enforcement first, then global
/// promotions, then global demotions.
pub fn governor_tick(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    local_root_groups: &[LocalRootTargets],
) -> Vec<GovernorAction> {
    let mut actions = Vec::new();

    // 1. Local root valency takes priority.
    actions.extend(enforce_local_root_valency(registry, local_root_groups));

    // 2. Global promotion targets (if not already covered by local roots).
    actions.extend(evaluate_cold_to_warm_promotions(registry, targets));
    actions.extend(evaluate_warm_to_hot_promotions(registry, targets));

    // 3. Global demotion targets.
    actions.extend(evaluate_hot_to_warm_demotions(registry, targets));
    actions.extend(evaluate_warm_to_cold_demotions(registry, targets));

    actions
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
    }

    fn make_registry(peers: &[(u16, PeerSource, PeerStatus)]) -> PeerRegistry {
        let mut reg = PeerRegistry::default();
        for &(port, source, status) in peers {
            reg.insert_source(addr(port), source);
            reg.set_status(addr(port), status);
        }
        reg
    }

    #[test]
    fn promote_cold_to_warm_when_below_target() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };

        let actions = evaluate_cold_to_warm_promotions(&reg, &targets);
        assert_eq!(actions.len(), 2);
        // Local root should be promoted first.
        assert_eq!(actions[0], GovernorAction::PromoteToWarm(addr(1)));
    }

    #[test]
    fn no_promotions_when_targets_met() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };

        let actions = evaluate_cold_to_warm_promotions(&reg, &targets);
        assert!(actions.is_empty());

        let actions = evaluate_warm_to_hot_promotions(&reg, &targets);
        assert!(actions.is_empty());
    }

    #[test]
    fn demote_hot_when_excess() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 3,
            target_active: 1,
            ..Default::default()
        };

        let actions = evaluate_hot_to_warm_demotions(&reg, &targets);
        assert_eq!(actions.len(), 2);
        // Non-local-root peers should be demoted first.
        for action in &actions {
            if let GovernorAction::DemoteToWarm(peer) = action {
                assert_ne!(*peer, addr(1), "local root should not be demoted first");
            }
        }
    }

    #[test]
    fn local_root_valency_enforcement() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        ]);
        let group = LocalRootTargets {
            peers: vec![addr(1), addr(2), addr(3)],
            hot_valency: 1,
            warm_valency: 2,
        };

        let actions = enforce_local_root_valency(&reg, &[group]);
        // Need 1 more warm (have 1, target 2) → promote 1 cold to warm.
        // Need 1 hot (have 0, target 1) → promote 1 warm to hot.
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
        assert!(actions.contains(&GovernorAction::PromoteToHot(addr(3))));
    }

    #[test]
    fn governor_tick_combined() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        let groups = vec![LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 1,
            warm_valency: 1,
        }];

        let actions = governor_tick(&reg, &targets, &groups);
        // Should have at least the local root promotion.
        assert!(!actions.is_empty());
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    }

    #[test]
    fn empty_registry_produces_no_actions() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let actions = governor_tick(&reg, &targets, &[]);
        assert!(actions.is_empty());
    }

    #[test]
    fn failure_tracking_and_backoff() {
        let mut state = GovernorState::default();
        let peer = addr(1);

        assert!(!state.is_backing_off(&peer));

        // Reach max_failures (default 5).
        for _ in 0..5 {
            state.record_failure(peer);
        }
        assert!(state.is_backing_off(&peer));

        // Success resets.
        state.record_success(peer);
        assert!(!state.is_backing_off(&peer));
    }

    #[test]
    fn filter_removes_backed_off_promotions() {
        let mut state = GovernorState::default();
        for _ in 0..5 {
            state.record_failure(addr(2));
        }

        let actions = vec![
            GovernorAction::PromoteToWarm(addr(1)),
            GovernorAction::PromoteToWarm(addr(2)),
            GovernorAction::DemoteToWarm(addr(3)),
        ];
        let filtered = state.filter_backed_off(actions);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(1))));
        assert!(filtered.contains(&GovernorAction::DemoteToWarm(addr(3))));
    }

    #[test]
    fn churn_demotes_non_local_root_warm() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        ]);
        let mut state = GovernorState::default();
        let now = Instant::now();

        // First tick — churn fires immediately (no previous churn).
        let action = state.evaluate_churn(&reg, now);
        assert_eq!(action, Some(GovernorAction::DemoteToCold(addr(2))));

        // Immediately again — interval not elapsed.
        let action = state.evaluate_churn(&reg, now);
        assert_eq!(action, None);

        // After churn interval — fires again.
        let later = now + Duration::from_secs(301);
        let action = state.evaluate_churn(&reg, later);
        assert_eq!(action, Some(GovernorAction::DemoteToCold(addr(2))));
    }

    #[test]
    fn churn_skips_local_root_only_warm() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        ]);
        let mut state = GovernorState::default();

        // No non-local-root warm peer → no churn demotion.
        let action = state.evaluate_churn(&reg, Instant::now());
        assert_eq!(action, None);
    }

    #[test]
    fn stateful_tick_integrates_churn_and_backoff() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        let groups = vec![LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 0,
            warm_valency: 1,
        }];
        let mut state = GovernorState::default();

        // Back off peer 1 so the local-root promotion is suppressed.
        for _ in 0..5 {
            state.record_failure(addr(1));
        }

        let actions = state.tick(&reg, &targets, &groups, Instant::now());
        // PromoteToWarm(addr(1)) should be filtered out.
        assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
        // Churn should demote addr(2) (non-local-root warm).
        assert!(actions.contains(&GovernorAction::DemoteToCold(addr(2))));
    }
}
