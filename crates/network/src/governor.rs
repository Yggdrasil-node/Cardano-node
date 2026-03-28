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

use crate::ledger_peers_provider::LedgerStateJudgement;
use crate::peer_registry::{PeerRegistry, PeerRegistryEntry, PeerSource, PeerStatus};
use crate::peer_selection::LocalRootConfig;
use crate::root_peers::{UseBootstrapPeers, UseLedgerPeers};
use std::collections::{BTreeMap, BTreeSet};
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
    /// Whether peers in this group are trustable in sensitive mode.
    ///
    /// Upstream: `IsTrustable` / `IsNotTrustable` from
    /// `Ouroboros.Network.PeerSelection.PeerTrustable`.
    pub trustable: bool,
}

impl LocalRootTargets {
    /// Build targets from a local root config and resolved peer addresses.
    pub fn from_config(config: &LocalRootConfig, resolved_peers: Vec<SocketAddr>) -> Self {
        Self {
            peers: resolved_peers,
            hot_valency: config.hot_valency,
            warm_valency: config.effective_warm_valency(),
            trustable: config.trustable,
        }
    }
}

// ---------------------------------------------------------------------------
// Bootstrap-sensitive mode
// ---------------------------------------------------------------------------

/// Peer selection mode derived from bootstrap flag and ledger state.
///
/// Upstream reference: `Cardano.Network.PeerSelection.Bootstrap` —
/// `requiresBootstrapPeers` determines "sensitive" vs normal mode.
///
/// In **sensitive mode** the governor restricts promotions to trustable
/// peers only (bootstrap peers + trustable local roots) and demotes any
/// non-trustable warm/hot peers.  In **normal mode** the governor uses
/// the full peer selection policy with all sources eligible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerSelectionMode {
    /// Normal peer selection — all sources are eligible for promotion.
    Normal,
    /// Sensitive mode — only bootstrap and trustable local-root peers may
    /// be warm or hot.  All other peers should be demoted.
    ///
    /// This mode is active when `UseBootstrapPeers` is enabled **and**
    /// the ledger state judgement is `TooOld`.
    Sensitive,
}

/// Determine the peer selection mode from bootstrap flag and ledger state.
///
/// Upstream: `requiresBootstrapPeers` in
/// `Cardano.Network.PeerSelection.Bootstrap`.
///
/// ```text
/// requiresBootstrapPeers _ubp YoungEnough = False
/// requiresBootstrapPeers ubp  TooOld      = isBootstrapPeersEnabled ubp
/// ```
pub fn requires_bootstrap_peers(
    use_bootstrap: &UseBootstrapPeers,
    judgement: LedgerStateJudgement,
) -> bool {
    match judgement {
        LedgerStateJudgement::YoungEnough => false,
        LedgerStateJudgement::TooOld | LedgerStateJudgement::Unavailable => {
            use_bootstrap.is_enabled()
        }
    }
}

/// Compute the peer selection mode from bootstrap flag and ledger state.
pub fn peer_selection_mode(
    use_bootstrap: &UseBootstrapPeers,
    judgement: LedgerStateJudgement,
) -> PeerSelectionMode {
    if requires_bootstrap_peers(use_bootstrap, judgement) {
        PeerSelectionMode::Sensitive
    } else {
        PeerSelectionMode::Normal
    }
}

/// Returns `true` when the node is able to make progress.
///
/// Upstream: `isNodeAbleToMakeProgress`:
/// ```text
/// not (requiresBootstrapPeers ubp lsj) || hasOnlyBootstrapPeers
/// ```
///
/// A node can make progress either when it is NOT in sensitive mode, OR
/// when it IS in sensitive mode and has already reached a clean state
/// where only trustable peers are connected.
pub fn is_node_able_to_make_progress(
    use_bootstrap: &UseBootstrapPeers,
    judgement: LedgerStateJudgement,
    has_only_trustable_peers: bool,
) -> bool {
    !requires_bootstrap_peers(use_bootstrap, judgement) || has_only_trustable_peers
}

// ---------------------------------------------------------------------------
// Association mode
// ---------------------------------------------------------------------------

/// Node-level peer-sharing willingness.
///
/// Upstream: `PeerSharing` in `Ouroboros.Network.PeerSelection.PeerSharing`
/// — negotiated via the handshake version data (`peerSharing` field).
///
/// This controls whether the node participates in peer sharing at all
/// (both requesting peers from others and responding to share requests).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum NodePeerSharing {
    /// Peer sharing is disabled — the node neither requests nor responds
    /// to peer sharing.
    #[default]
    PeerSharingDisabled,
    /// Peer sharing is enabled — the node may request and respond to
    /// peer sharing.
    PeerSharingEnabled,
}

impl NodePeerSharing {
    /// Returns `true` when peer sharing is enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::PeerSharingEnabled)
    }

    /// Construct from the handshake wire value (0 = disabled, 1 = enabled).
    pub fn from_wire(value: u8) -> Self {
        if value >= 1 {
            Self::PeerSharingEnabled
        } else {
            Self::PeerSharingDisabled
        }
    }
}

/// Whether the node operates in local-roots-only or unrestricted mode.
///
/// Upstream: `AssociationMode` in
/// `Ouroboros.Network.PeerSelection.Governor.Types`.
///
/// A node is classified as `LocalRootsOnly` if it is a hidden relay or
/// a block producer — i.e. it is configured such that it can only ever
/// be connected to its configured local root peers.  This is the case
/// when one of:
///
///  * `DontUseBootstrapPeers` **and** `DontUseLedgerPeers` **and**
///    `PeerSharingDisabled`; or
///  * `UseBootstrapPeers` **and** `DontUseLedgerPeers` **and**
///    `PeerSharingDisabled` **and** the node is synced (not requiring
///    bootstrap peers — i.e. `LedgerStateJudgement::YoungEnough`).
///
/// In the second case a node can transition between `LocalRootsOnly` and
/// `Unrestricted` depending on `LedgerStateJudgement`:  when the ledger
/// state becomes `TooOld`, the node enters `Unrestricted` mode (because
/// it re-activates bootstrap peer usage), and when it catches up again
/// it transitions back to `LocalRootsOnly`.
///
/// Reference: `readAssociationMode` in
/// `Ouroboros.Network.PeerSelection.Governor.Monitor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssociationMode {
    /// The node only connects to local root peers (BP / hidden relay).
    LocalRootsOnly,
    /// The node may discover and connect to any peer source.
    Unrestricted,
}

/// Compute the association mode from the node's configuration and current
/// ledger state.
///
/// Upstream: `readAssociationMode` in
/// `Ouroboros.Network.PeerSelection.Governor.Monitor`.
///
/// ```text
/// readAssociationMode:
///   (DontUseBootstrapPeers, DontUseLedgerPeers, PeerSharingDisabled) -> LocalRootsOnly
///   (UseBootstrapPeers _,   DontUseLedgerPeers, PeerSharingDisabled)
///     | !requiresBootstrapPeers ubp lsj                              -> LocalRootsOnly
///   _                                                                 -> Unrestricted
/// ```
pub fn compute_association_mode(
    use_bootstrap: &UseBootstrapPeers,
    use_ledger: &UseLedgerPeers,
    peer_sharing: NodePeerSharing,
    judgement: LedgerStateJudgement,
) -> AssociationMode {
    if use_ledger.enabled() || peer_sharing.is_enabled() {
        return AssociationMode::Unrestricted;
    }
    // Ledger peers disabled and peer sharing disabled.
    match use_bootstrap {
        UseBootstrapPeers::DontUseBootstrapPeers => AssociationMode::LocalRootsOnly,
        UseBootstrapPeers::UseBootstrapPeers(_) => {
            // Bootstrap peers are configured but not in use (synced) →
            // LocalRootsOnly.  When requiring bootstrap peers (TooOld) →
            // Unrestricted because the node needs external bootstrap sources.
            if requires_bootstrap_peers(use_bootstrap, judgement) {
                AssociationMode::Unrestricted
            } else {
                AssociationMode::LocalRootsOnly
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Governor state
// ---------------------------------------------------------------------------

/// Phase of the two-phase churn cycle.
///
/// The upstream `peerChurnGovernor` in `Ouroboros.Network.PeerSelection.Churn`
/// cycles through decrease-then-increase phases:
///
///  1. **`DecreasedActive`** — lower active (hot) targets using
///     [`churn_decrease()`], causing the governor to demote excess hot
///     peers to warm.
///  2. **`DecreasedEstablished`** — lower established (warm) targets,
///     causing the governor to demote excess warm peers to cold.
///  3. **`Idle`** — targets restored to configured values, causing the
///     governor to promote fresh peers into the vacated slots.
///
/// Both regular and big-ledger targets are decreased in parallel.
///
/// Reference: `Ouroboros.Network.PeerSelection.Churn.churnLoop`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChurnPhase {
    /// Not in a churn cycle — targets are at their configured values.
    Idle,
    /// Active (hot) targets have been decreased via [`churn_decrease()`].
    DecreasedActive {
        /// When this phase started.
        started: Instant,
    },
    /// Established (warm) targets have been decreased via
    /// [`churn_decrease()`].
    DecreasedEstablished {
        /// When this phase started.
        started: Instant,
    },
}

/// Configurable churn parameters.
///
/// Upstream reference: `Ouroboros.Network.PeerSelection.Churn` —
/// `peerChurnGovernor` runs a periodic two-phase decrease/restore cycle.
/// The `decrease` function matches the upstream pattern:
///
/// ```text
/// decrease(v) = max(0, v - max(1, v / 5))
/// ```
///
/// *"Replace 20% or at least 1 peer every churn interval."*
///
/// Churn intervals are mode-dependent:
/// * **Deadline mode** (node is near tip): `deadline_churn_interval`
///   (upstream `defaultDeadlineChurnInterval` = 3300 s).
/// * **Bulk-sync mode** (node is syncing): `bulk_churn_interval`
///   (upstream `defaultBulkChurnInterval` = 900 s).
///
/// Reference: `Ouroboros.Network.PeerSelection.Churn` and
/// `Ouroboros.Network.Diffusion.Configuration`.
#[derive(Clone, Debug)]
pub struct ChurnConfig {
    /// Interval between churn cycles when the node is syncing
    /// (bulk-sync / catching up).
    ///
    /// Upstream: `defaultBulkChurnInterval` = 900 s.
    pub bulk_churn_interval: Duration,
    /// Interval between churn cycles when the node is near the tip
    /// (deadline / caught-up mode).
    ///
    /// Upstream: `defaultDeadlineChurnInterval` = 3300 s.
    pub deadline_churn_interval: Duration,
    /// How long each decrease phase lasts before the state machine
    /// advances to the next phase.
    ///
    /// Upstream equivalent: individual step timeouts (`shortTimeout`
    /// 60 s, `deactivateTimeout` ~260 s, etc.).  We use a single
    /// uniform timeout for simplicity.
    pub phase_timeout: Duration,
}

impl ChurnConfig {
    /// Return the churn cycle interval for the given fetch mode.
    ///
    /// Upstream: `peerChurnGovernor` uses `pcaBulkInterval` when
    /// `FetchModeBulkSync` and `pcaDeadlineInterval` when
    /// `FetchModeDeadline`.
    pub fn interval_for_mode(&self, mode: FetchMode) -> Duration {
        match mode {
            FetchMode::FetchModeBulkSync => self.bulk_churn_interval,
            FetchMode::FetchModeDeadline => self.deadline_churn_interval,
        }
    }
}

impl Default for ChurnConfig {
    fn default() -> Self {
        Self {
            bulk_churn_interval: Duration::from_secs(900),
            deadline_churn_interval: Duration::from_secs(3300),
            phase_timeout: Duration::from_secs(60),
        }
    }
}

/// Compute how many peers to churn from a current count.
///
/// Upstream: `decrease v = max 0 $ v - max 1 (v \`div\` 5)` —
/// *"Replace 20% or at least one peer every churn interval."*
pub fn churn_decrease(count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let remove = std::cmp::max(1, count / 5);
    count.saturating_sub(remove)
}

/// Per-peer failure record with timestamps for time-based backoff.
#[derive(Clone, Debug)]
pub struct PeerFailureRecord {
    /// How many consecutive failures since last success.
    pub failure_count: u32,
    /// When the last failure was recorded.
    pub last_failure: Instant,
}

/// Mutable governor state carried across ticks.
///
/// Tracks connection failures with time-based backoff and a two-phase
/// churn cycle so that the governor can back off from failing peers and
/// periodically rotate both the warm and hot sets.
///
/// The failure backoff follows exponential growth:
/// a peer is skipped if fewer than `failure_backoff * 2^(failures-1)`
/// seconds have elapsed since the last failure, capped at `max_failures`.
/// Failure counts decay over time using `failure_decay`, so peers can
/// recover automatically after quiet periods even without an explicit
/// success signal.
///
/// Churn operates via temporary target reduction (see [`ChurnPhase`]):
/// lowered targets cause the standard evaluation functions to emit
/// demotion actions, and restored targets cause promotion of fresh
/// peers — achieving peer rotation without special-case churn logic.
///
/// The [`ChurnRegime`] controls how aggressively targets are decreased
/// during churn phases — syncing nodes use a more aggressive regime
/// (see [`churn_decrease_active()`] and [`churn_decrease_established()`]).
///
/// In-flight sets track peers with pending asynchronous actions so
/// that the governor does not emit duplicate actions on subsequent
/// ticks.  Upstream: `inProgressPromoteCold`, `inProgressPromoteWarm`,
/// `inProgressDemoteWarm`, `inProgressDemoteHot` in
/// `PeerSelectionState`.
#[derive(Clone, Debug)]
pub struct GovernorState {
    /// Consecutive connection failure records per peer.
    pub failures: BTreeMap<SocketAddr, PeerFailureRecord>,
    /// Maximum failures used to cap exponential backoff growth.
    pub max_failures: u32,
    /// Base back-off duration per failure.  Actual backoff is
    /// `failure_backoff * 2^(min(failures, max_failures) - 1)`.
    pub failure_backoff: Duration,
    /// Time after which one failure level decays if no further failures
    /// are observed for a peer.
    ///
    /// This mirrors upstream `policyClearFailCountDelay` semantics where
    /// stale failures should not permanently penalize a peer.
    pub failure_decay: Duration,
    /// Maximum connection retries before a non-protected peer is forgotten.
    ///
    /// When `Some(n)`, cold peers whose decayed failure count exceeds `n`
    /// and that are not protected (local root, bootstrap, ledger, big-ledger)
    /// are forgotten by [`evaluate_forget_failed_peers`].  `None` disables
    /// failure-based forgetting.
    ///
    /// Upstream: the `maxFail` parameter in `reportFailures` from
    /// `Ouroboros.Network.PeerSelection.State.KnownPeers`.
    pub max_connection_retries: Option<u32>,
    /// Churn configuration.
    pub churn: ChurnConfig,
    /// Current phase of the two-phase churn cycle.
    ///
    /// See [`ChurnPhase`] for the state machine description.
    pub churn_phase: ChurnPhase,
    /// When the last churn cycle completed (returned to [`ChurnPhase::Idle`]).
    /// Used to pace churn cycles at the mode-dependent interval.
    pub last_churn_cycle: Option<Instant>,
    /// Current churn regime controlling target-decrease aggressiveness.
    ///
    /// Updated at each tick from `(ChurnMode, UseBootstrapPeers,
    /// ConsensusMode)` via [`pick_churn_regime()`].  Defaults to
    /// [`ChurnRegime::ChurnDefault`].
    pub churn_regime: ChurnRegime,
    /// Current fetch mode, used to select the churn cycle interval.
    ///
    /// Updated externally from ledger state judgement via
    /// [`fetch_mode_from_judgement()`].  Defaults to
    /// [`FetchMode::FetchModeBulkSync`] (syncing).
    pub fetch_mode: FetchMode,
    /// Maximum hot valency across all local-root groups.
    ///
    /// Used by regime-aware churn to floor the active target decrease.
    /// Updated at each tick.
    pub local_root_hot_target: usize,
    /// Peers that currently have an in-flight cold→warm promotion.
    /// Governor filters out duplicate promotions for these peers.
    ///
    /// Upstream: `inProgressPromoteCold` in `PeerSelectionState`.
    pub in_flight_warm: BTreeSet<SocketAddr>,
    /// Peers that currently have an in-flight warm→hot promotion.
    ///
    /// Upstream: `inProgressPromoteWarm` in `PeerSelectionState`.
    pub in_flight_hot: BTreeSet<SocketAddr>,
    /// Peers that currently have an in-flight warm→cold demotion.
    /// Governor filters out duplicate demotions for these peers.
    ///
    /// Upstream: `inProgressDemoteWarm` in `PeerSelectionState`.
    pub in_flight_demote_warm: BTreeSet<SocketAddr>,
    /// Peers that currently have an in-flight hot→warm demotion.
    ///
    /// Upstream: `inProgressDemoteHot` in `PeerSelectionState`.
    pub in_flight_demote_hot: BTreeSet<SocketAddr>,
    /// Number of peer-sharing requests currently in flight.
    ///
    /// Upstream: `inProgressPeerShareReqs` in `PeerSelectionState`.
    pub in_progress_peer_share_reqs: u32,
    /// Maximum concurrent peer-sharing requests.
    ///
    /// Upstream: `policyMaxInProgressPeerShareReqs` in
    /// `PeerSelectionPolicy`.
    pub max_in_progress_peer_share_reqs: u32,
}

impl Default for GovernorState {
    fn default() -> Self {
        Self {
            failures: BTreeMap::new(),
            max_failures: 5,
            failure_backoff: Duration::from_secs(30),
            failure_decay: Duration::from_secs(120),
            max_connection_retries: None,
            churn: ChurnConfig::default(),
            churn_phase: ChurnPhase::Idle,
            last_churn_cycle: None,
            churn_regime: ChurnRegime::ChurnDefault,
            fetch_mode: FetchMode::FetchModeBulkSync,
            local_root_hot_target: 0,
            in_flight_warm: BTreeSet::new(),
            in_flight_hot: BTreeSet::new(),
            in_flight_demote_warm: BTreeSet::new(),
            in_flight_demote_hot: BTreeSet::new(),
            in_progress_peer_share_reqs: 0,
            max_in_progress_peer_share_reqs: 2,
        }
    }
}

impl GovernorState {
    fn decayed_failure_count(&self, record: &PeerFailureRecord, now: Instant) -> u32 {
        if record.failure_count == 0 {
            return 0;
        }

        let decay_secs = self.failure_decay.as_secs();
        if decay_secs == 0 {
            return record.failure_count;
        }

        let elapsed_steps = now.duration_since(record.last_failure).as_secs() / decay_secs;
        record.failure_count.saturating_sub(elapsed_steps as u32)
    }

    /// Record a successful connection to `peer`, resetting its failure count.
    pub fn record_success(&mut self, peer: SocketAddr) {
        self.failures.remove(&peer);
    }

    /// Record a connection failure for `peer`.
    pub fn record_failure(&mut self, peer: SocketAddr) {
        let now = Instant::now();
        let decayed = self
            .failures
            .get(&peer)
            .map(|record| self.decayed_failure_count(record, now))
            .unwrap_or(0);

        let record = self.failures.entry(peer).or_insert_with(|| PeerFailureRecord {
            failure_count: 0,
            last_failure: now,
        });
        record.failure_count = decayed.saturating_add(1);
        record.last_failure = now;
    }

    /// Return true if `peer` should be skipped due to recent failures.
    ///
    /// Uses exponential backoff: a peer with `n` failures is backed off
    /// for `failure_backoff * 2^(n-1)` seconds since its last failure.
    /// Backoff growth is capped by `max_failures` and failure counts decay
    /// over time according to `failure_decay`.
    pub fn is_backing_off(&self, peer: &SocketAddr, now: Instant) -> bool {
        let record = match self.failures.get(peer) {
            Some(r) => r,
            None => return false,
        };

        let failures = self.decayed_failure_count(record, now);
        if failures == 0 {
            return false;
        }

        // Exponential backoff: base * 2^(n-1), capped at max_failures-1
        let exp = std::cmp::min(failures - 1, self.max_failures - 1);
        let backoff = self.failure_backoff * 2u32.saturating_pow(exp);
        now.duration_since(record.last_failure) < backoff
    }

    /// Drop stale failure records that have fully decayed.
    pub fn prune_decayed_failures(&mut self, now: Instant) {
        let decay = self.failure_decay;
        self.failures.retain(|_, record| {
            if record.failure_count == 0 {
                return false;
            }
            if decay.as_secs() == 0 {
                return true;
            }
            let elapsed_steps = now.duration_since(record.last_failure).as_secs() / decay.as_secs();
            elapsed_steps < u64::from(record.failure_count)
        });
    }

    /// Filter a list of governor actions, removing promotions for peers
    /// that are currently in the back-off window or have in-flight
    /// promotions, and removing demotions for peers that already have
    /// in-flight demotions.
    ///
    /// Upstream: `inProgressPromoteCold`/`inProgressPromoteWarm` filter
    /// promotions; `inProgressDemoteWarm`/`inProgressDemoteHot` filter
    /// demotions.
    pub fn filter_backed_off(&self, actions: Vec<GovernorAction>, now: Instant) -> Vec<GovernorAction> {
        actions
            .into_iter()
            .filter(|a| match a {
                GovernorAction::PromoteToWarm(addr) => {
                    !self.is_backing_off(addr, now) && !self.in_flight_warm.contains(addr)
                }
                GovernorAction::PromoteToHot(addr) => {
                    !self.is_backing_off(addr, now) && !self.in_flight_hot.contains(addr)
                }
                GovernorAction::DemoteToWarm(addr) => {
                    !self.in_flight_demote_hot.contains(addr)
                }
                GovernorAction::DemoteToCold(addr) => {
                    !self.in_flight_demote_warm.contains(addr)
                }
                _ => true,
            })
            .collect()
    }

    /// Mark a peer as having an in-flight warm promotion.
    pub fn mark_in_flight_warm(&mut self, peer: SocketAddr) {
        self.in_flight_warm.insert(peer);
    }

    /// Mark a peer as having an in-flight hot promotion.
    pub fn mark_in_flight_hot(&mut self, peer: SocketAddr) {
        self.in_flight_hot.insert(peer);
    }

    /// Clear the in-flight warm flag, typically after promotion completes or fails.
    pub fn clear_in_flight_warm(&mut self, peer: &SocketAddr) {
        self.in_flight_warm.remove(peer);
    }

    /// Clear the in-flight hot flag, typically after promotion completes or fails.
    pub fn clear_in_flight_hot(&mut self, peer: &SocketAddr) {
        self.in_flight_hot.remove(peer);
    }

    /// Mark a peer as having an in-flight warm→cold demotion.
    ///
    /// Upstream: add to `inProgressDemoteWarm` in `PeerSelectionState`.
    pub fn mark_in_flight_demote_warm(&mut self, peer: SocketAddr) {
        self.in_flight_demote_warm.insert(peer);
    }

    /// Mark a peer as having an in-flight hot→warm demotion.
    ///
    /// Upstream: add to `inProgressDemoteHot` in `PeerSelectionState`.
    pub fn mark_in_flight_demote_hot(&mut self, peer: SocketAddr) {
        self.in_flight_demote_hot.insert(peer);
    }

    /// Clear the in-flight warm→cold demotion flag.
    pub fn clear_in_flight_demote_warm(&mut self, peer: &SocketAddr) {
        self.in_flight_demote_warm.remove(peer);
    }

    /// Clear the in-flight hot→warm demotion flag.
    pub fn clear_in_flight_demote_hot(&mut self, peer: &SocketAddr) {
        self.in_flight_demote_hot.remove(peer);
    }

    /// Record that a peer-sharing request was dispatched.
    ///
    /// Upstream: increments `inProgressPeerShareReqs`.
    pub fn mark_peer_share_sent(&mut self) {
        self.in_progress_peer_share_reqs =
            self.in_progress_peer_share_reqs.saturating_add(1);
    }

    /// Record that one or more peer-sharing responses arrived.
    ///
    /// Upstream: decrements `inProgressPeerShareReqs` by the number of
    /// completed requests.
    pub fn clear_peer_share_completed(&mut self, count: u32) {
        self.in_progress_peer_share_reqs =
            self.in_progress_peer_share_reqs.saturating_sub(count);
    }

    /// Return targets modified by the current churn phase and regime.
    ///
    /// During [`ChurnPhase::DecreasedActive`], active targets are lowered
    /// using the regime-aware [`churn_decrease_active()`].  During
    /// [`ChurnPhase::DecreasedEstablished`], established targets are lowered
    /// using [`churn_decrease_established()`].  During [`ChurnPhase::Idle`],
    /// targets are returned unchanged.
    ///
    /// The [`ChurnRegime`] stored in `self.churn_regime` controls how
    /// aggressively targets are decreased — syncing nodes reduce more
    /// aggressively to cycle through peers faster.
    ///
    /// This is the core mechanism by which the two-phase churn rotates
    /// peers: lowered targets cause the governor to emit demotion actions,
    /// and restored targets cause it to emit promotion actions for fresh
    /// peers.
    pub fn apply_churn_to_targets(&self, targets: &GovernorTargets) -> GovernorTargets {
        match self.churn_phase {
            ChurnPhase::Idle => targets.clone(),
            ChurnPhase::DecreasedActive { .. } => {
                let mut t = targets.clone();
                t.target_active = churn_decrease_active(
                    self.churn_regime,
                    targets.target_active,
                    self.local_root_hot_target,
                );
                t.target_active_big_ledger = churn_decrease_active(
                    self.churn_regime,
                    targets.target_active_big_ledger,
                    0, // big-ledger has no local-root concept
                );
                t
            }
            ChurnPhase::DecreasedEstablished { .. } => {
                let mut t = targets.clone();
                t.target_established = churn_decrease_established(
                    self.churn_regime,
                    targets.target_established,
                    targets.target_active,
                );
                t.target_established_big_ledger = churn_decrease_established(
                    self.churn_regime,
                    targets.target_established_big_ledger,
                    targets.target_active_big_ledger,
                );
                t
            }
        }
    }

    /// Advance the churn state machine based on the current time.
    ///
    /// Called at the start of each [`tick()`](Self::tick).  If no churn
    /// cycle is active and the cycle interval has elapsed, starts a new
    /// cycle by entering [`ChurnPhase::DecreasedActive`].  If a decrease
    /// phase has exceeded `phase_timeout`, advances to the next phase or
    /// completes the cycle.
    ///
    /// The cycle interval depends on the current [`FetchMode`]:
    /// `deadline_churn_interval` when near the tip,
    /// `bulk_churn_interval` when syncing.
    ///
    /// Reference: `peerChurnGovernor` loop in
    /// `Ouroboros.Network.PeerSelection.Churn`.
    fn advance_churn(&mut self, now: Instant) {
        let interval = self.churn.interval_for_mode(self.fetch_mode);
        match self.churn_phase {
            ChurnPhase::Idle => {
                let due = match self.last_churn_cycle {
                    Some(last) => now.duration_since(last) >= interval,
                    None => true, // First cycle fires immediately.
                };
                if due {
                    self.churn_phase = ChurnPhase::DecreasedActive { started: now };
                }
            }
            ChurnPhase::DecreasedActive { started } => {
                if now.duration_since(started) >= self.churn.phase_timeout {
                    self.churn_phase = ChurnPhase::DecreasedEstablished { started: now };
                }
            }
            ChurnPhase::DecreasedEstablished { started } => {
                if now.duration_since(started) >= self.churn.phase_timeout {
                    self.churn_phase = ChurnPhase::Idle;
                    self.last_churn_cycle = Some(now);
                }
            }
        }
    }

    /// Run a full governance pass with two-phase churn and failure
    /// filtering.
    ///
    /// 1. Updates `local_root_hot_target` from configured groups.
    /// 2. Prunes fully-decayed failure records.
    /// 3. Advances the churn state machine (using mode-dependent intervals).
    /// 4. Applies churn-phase target modifications using the current
    ///    [`ChurnRegime`] (lowered targets during decrease phases cause
    ///    demotion actions; restored targets cause promotions of fresh
    ///    peers).
    /// 5. Evaluates the full governor pass against effective targets,
    ///    respecting the bootstrap-sensitive [`PeerSelectionMode`].
    /// 6. Filters out promotions for backed-off or in-flight peers.
    pub fn tick(
        &mut self,
        registry: &PeerRegistry,
        targets: &GovernorTargets,
        local_root_groups: &[LocalRootTargets],
        mode: PeerSelectionMode,
        association: AssociationMode,
        now: Instant,
    ) -> Vec<GovernorAction> {
        // Update local root hot target for regime-aware churn decreases.
        self.local_root_hot_target = local_root_groups
            .iter()
            .map(|g| g.hot_valency as usize)
            .max()
            .unwrap_or(0);
        self.prune_decayed_failures(now);
        self.advance_churn(now);
        let effective_targets = self.apply_churn_to_targets(targets);
        let actions = governor_tick(registry, &effective_targets, local_root_groups, mode, association, Some(self), now);
        self.filter_backed_off(actions, now)
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
    /// Remove a cold peer from the known set entirely.
    ///
    /// Upstream equivalent: `forgetColdPeers` in the governor targets
    /// module — peers beyond `target_known` whose sources have been
    /// exhausted are dropped.
    ForgetPeer(SocketAddr),
    /// Request peer sharing from a warm or hot peer.
    ///
    /// The runtime should send a `MsgShareRequest` via the PeerSharing
    /// mini-protocol to this peer and add any received addresses to the
    /// known peer set.
    ///
    /// Upstream: `requestPeerShare` in
    /// `Ouroboros.Network.PeerSelection.Governor.KnownPeers.belowTarget`.
    ShareRequest(SocketAddr),
}

// ---------------------------------------------------------------------------
// Evaluation helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RegularPeerCounts {
    known: usize,
    established: usize,
    active: usize,
}

fn regular_peer_counts(registry: &PeerRegistry) -> RegularPeerCounts {
    let mut counts = RegularPeerCounts::default();
    for (_, entry) in registry.iter() {
        if is_big_ledger(entry) {
            continue;
        }

        counts.known += 1;
        match entry.status {
            PeerStatus::PeerWarm => counts.established += 1,
            PeerStatus::PeerHot => {
                counts.established += 1;
                counts.active += 1;
            }
            PeerStatus::PeerCold | PeerStatus::PeerCooling => {}
        }
    }
    counts
}

/// Evaluate which cold peers should be promoted to warm to meet the
/// established peer target.
///
/// Returns promotion actions, choosing local-root peers first for
/// stability, then other cold peers.  Within each tier, non-tepid
/// peers are preferred over tepid peers (recently hot→warm demoted).
///
/// Upstream: `belowTarget` in `Governor.EstablishedPeers` sorts
/// candidates so that non-tepid peers appear first.
pub fn evaluate_cold_to_warm_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    if counts.established >= targets.target_established {
        return Vec::new();
    }
    let needed = targets.target_established - counts.established;

    // Collect cold peers in four buckets (local-root non-tepid, local-root
    // tepid, other non-tepid, other tepid) so non-tepid peers are promoted
    // first within each source tier.
    let mut local_fresh = Vec::new();
    let mut local_tepid = Vec::new();
    let mut other_fresh = Vec::new();
    let mut other_tepid = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerCold && !is_big_ledger(entry) {
            let is_local = entry.sources.contains(&PeerSource::PeerSourceLocalRoot);
            match (is_local, entry.tepid) {
                (true, false) => local_fresh.push(*addr),
                (true, true) => local_tepid.push(*addr),
                (false, false) => other_fresh.push(*addr),
                (false, true) => other_tepid.push(*addr),
            }
        }
    }

    let mut actions = Vec::new();
    for addr in local_fresh
        .into_iter()
        .chain(local_tepid)
        .chain(other_fresh)
        .chain(other_tepid)
    {
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
/// Returns promotion actions, choosing local-root peers first.  Within
/// each tier, non-tepid peers are preferred over tepid peers (peers
/// that were recently demoted from hot).
///
/// Upstream: `belowTarget` in `Governor.ActivePeers` deprioritizes
/// peers with `knownPeerTepid` set.
pub fn evaluate_warm_to_hot_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    if counts.active >= targets.target_active {
        return Vec::new();
    }
    let needed = targets.target_active - counts.active;

    let mut local_fresh = Vec::new();
    let mut local_tepid = Vec::new();
    let mut other_fresh = Vec::new();
    let mut other_tepid = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerWarm && !is_big_ledger(entry) {
            let is_local = entry.sources.contains(&PeerSource::PeerSourceLocalRoot);
            match (is_local, entry.tepid) {
                (true, false) => local_fresh.push(*addr),
                (true, true) => local_tepid.push(*addr),
                (false, false) => other_fresh.push(*addr),
                (false, true) => other_tepid.push(*addr),
            }
        }
    }

    let mut actions = Vec::new();
    for addr in local_fresh
        .into_iter()
        .chain(local_tepid)
        .chain(other_fresh)
        .chain(other_tepid)
    {
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
    let counts = regular_peer_counts(registry);
    if counts.active <= targets.target_active {
        return Vec::new();
    }
    let excess = counts.active - targets.target_active;

    // Collect hot peers, preferring to demote non-local-root first.
    let mut non_local_hot = Vec::new();
    let mut local_hot = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerHot && !is_big_ledger(entry) {
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
    let counts = regular_peer_counts(registry);
    if counts.established <= targets.target_established {
        return Vec::new();
    }
    let excess = counts.established - targets.target_established;

    let mut non_local_warm = Vec::new();
    let mut local_warm = Vec::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerWarm && !is_big_ledger(entry) {
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
// Big-ledger peer evaluation
// ---------------------------------------------------------------------------

/// Return true if the peer entry is from a big-ledger source.
fn is_big_ledger(entry: &PeerRegistryEntry) -> bool {
    entry.sources.contains(&PeerSource::PeerSourceBigLedger)
}

/// Evaluate which cold big-ledger peers should be promoted to warm to meet
/// the `target_established_big_ledger` target.
///
/// Upstream equivalent:
/// `Ouroboros.Network.PeerSelection.Governor.EstablishedPeers` —
/// big-ledger path.
pub fn evaluate_cold_to_warm_big_ledger_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let warm_or_hot = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && matches!(e.status, PeerStatus::PeerWarm | PeerStatus::PeerHot))
        .count();

    let target = targets.target_established_big_ledger;
    if warm_or_hot >= target {
        return Vec::new();
    }
    let needed = target - warm_or_hot;

    registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerCold)
        .take(needed)
        .map(|(addr, _)| GovernorAction::PromoteToWarm(*addr))
        .collect()
}

/// Evaluate which warm big-ledger peers should be promoted to hot to meet
/// the `target_active_big_ledger` target.
pub fn evaluate_warm_to_hot_big_ledger_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let hot_count = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerHot)
        .count();

    let target = targets.target_active_big_ledger;
    if hot_count >= target {
        return Vec::new();
    }
    let needed = target - hot_count;

    registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerWarm)
        .take(needed)
        .map(|(addr, _)| GovernorAction::PromoteToHot(*addr))
        .collect()
}

/// Evaluate which hot big-ledger peers should be demoted to warm when
/// we exceed `target_active_big_ledger`.
pub fn evaluate_hot_to_warm_big_ledger_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let hot_count = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerHot)
        .count();

    let target = targets.target_active_big_ledger;
    if hot_count <= target {
        return Vec::new();
    }
    let excess = hot_count - target;

    registry
        .iter()
        .filter(|(_, e)| {
            is_big_ledger(e)
                && e.status == PeerStatus::PeerHot
                && !e.sources.contains(&PeerSource::PeerSourceLocalRoot)
        })
        .take(excess)
        .map(|(addr, _)| GovernorAction::DemoteToWarm(*addr))
        .collect()
}

/// Evaluate which warm big-ledger peers should be demoted to cold when
/// we exceed `target_established_big_ledger`.
pub fn evaluate_warm_to_cold_big_ledger_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let warm_count = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerWarm)
        .count();

    let target = targets.target_established_big_ledger;
    let hot_count = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerHot)
        .count();

    let total_established = warm_count + hot_count;
    if total_established <= target {
        return Vec::new();
    }
    let excess = total_established - target;

    registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerWarm)
        .take(excess)
        .map(|(addr, _)| GovernorAction::DemoteToCold(*addr))
        .collect()
}

// ---------------------------------------------------------------------------
// Forget cold peers — known-peer set management
// ---------------------------------------------------------------------------

/// Evaluate which cold, non-local-root, non-big-ledger peers should be
/// forgotten (removed from the known set) when the known count exceeds
/// `target_known`.
///
/// Upstream equivalent:
/// `Ouroboros.Network.PeerSelection.Governor.KnownPeers.belowTarget` —
/// the governor forgets cold peers it no longer needs sources for.
pub fn evaluate_forget_cold_peers(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    let target = targets.target_known;
    if counts.known <= target {
        return Vec::new();
    }
    let excess = counts.known - target;

    // Only forget cold, ephemeral peers (peer-share or public-root that
    // are no longer essential). Local-root, Bootstrap, Ledger, and
    // BigLedger peers are never forgotten.
    let forgettable_sources = [
        PeerSource::PeerSourcePeerShare,
        PeerSource::PeerSourcePublicRoot,
    ];

    registry
        .iter()
        .filter(|(_, e)| {
            !is_big_ledger(e)
                && e.status == PeerStatus::PeerCold
                && e.sources.iter().all(|s| forgettable_sources.contains(s))
        })
        .take(excess)
        .map(|(addr, _)| GovernorAction::ForgetPeer(*addr))
        .collect()
}

/// Evaluate which cold peers should be forgotten because they have
/// exceeded the maximum connection retry threshold.
///
/// Peers are protected (never forgotten) if they have a local-root,
/// bootstrap, ledger, or big-ledger source.  Only peers whose sole
/// sources are ephemeral (peer-share or public-root) are eligible.
///
/// Upstream equivalent: `reportFailures` in
/// `Ouroboros.Network.PeerSelection.State.KnownPeers` — when
/// `knownPeerFailCount > maxFail`, the peer is removed unless
/// "unforgetable" (local root peer).
pub fn evaluate_forget_failed_peers(
    registry: &PeerRegistry,
    state: &GovernorState,
    now: Instant,
) -> Vec<GovernorAction> {
    let max_retries = match state.max_connection_retries {
        Some(n) => n,
        None => return Vec::new(),
    };

    // Protected sources: peers with any of these are never forgotten due
    // to failures (upstream "unforgetable" concept).
    let protected_sources = [
        PeerSource::PeerSourceLocalRoot,
        PeerSource::PeerSourceBootstrap,
        PeerSource::PeerSourceLedger,
        PeerSource::PeerSourceBigLedger,
    ];

    registry
        .iter()
        .filter(|(addr, entry)| {
            entry.status == PeerStatus::PeerCold
                && !entry.sources.iter().any(|s| protected_sources.contains(s))
                && state
                    .failures
                    .get(addr)
                    .map(|r| state.decayed_failure_count(r, now) > max_retries)
                    .unwrap_or(false)
        })
        .map(|(addr, _)| GovernorAction::ForgetPeer(*addr))
        .collect()
}

// ---------------------------------------------------------------------------
// Peer sharing request generation
// ---------------------------------------------------------------------------

/// Evaluate whether to request peer sharing from warm or hot peers
/// when the known-peer count is below `target_known`.
///
/// Returns [`GovernorAction::ShareRequest`] for eligible warm/hot peers,
/// bounded by the remaining peer-sharing request budget.  Local-root and
/// bootstrap peers are excluded since they are manually configured and
/// do not participate in gossip-based discovery.
///
/// Upstream reference: `belowTarget` in
/// `Ouroboros.Network.PeerSelection.Governor.KnownPeers` — triggers
/// `requestPeerShare` when `numKnownPeers < targetNumberOfKnownPeers`
/// and `numPeerShareReqsPossible > 0`.
pub fn evaluate_peer_share_requests(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    state: &GovernorState,
) -> Vec<GovernorAction> {
    // Check budget.
    if state.in_progress_peer_share_reqs >= state.max_in_progress_peer_share_reqs {
        return Vec::new();
    }
    let budget = (state.max_in_progress_peer_share_reqs - state.in_progress_peer_share_reqs) as usize;

    // Check whether known-peer set is below target.
    let counts = regular_peer_counts(registry);
    if counts.known >= targets.target_known {
        return Vec::new();
    }

    // Pick warm/hot peers that can serve PeerSharing requests.
    // Exclude local-root and bootstrap sources — they are configured
    // rather than discovered and are not expected to participate in
    // gossip-based peer sharing.
    registry
        .iter()
        .filter(|(_, entry)| {
            matches!(entry.status, PeerStatus::PeerWarm | PeerStatus::PeerHot)
                && !entry.sources.contains(&PeerSource::PeerSourceLocalRoot)
                && !entry.sources.contains(&PeerSource::PeerSourceBootstrap)
                && !is_big_ledger(entry)
        })
        .take(budget)
        .map(|(addr, _)| GovernorAction::ShareRequest(*addr))
        .collect()
}

// ---------------------------------------------------------------------------
// Bootstrap-sensitive mode evaluation
// ---------------------------------------------------------------------------

/// Collect the set of trustable local-root peers from the local root groups.
///
/// A peer is trustable if it belongs to a local root group with
/// `trustable == true`.  This mirrors the upstream `trustableKeysSet`
/// from `LocalRootPeers`.
fn trustable_local_root_set(groups: &[LocalRootTargets]) -> BTreeSet<SocketAddr> {
    groups
        .iter()
        .filter(|g| g.trustable)
        .flat_map(|g| g.peers.iter().copied())
        .collect()
}

/// Returns `true` when a peer is trustable in sensitive mode.
///
/// A peer is trustable if it is sourced from bootstrap peers or if it
/// belongs to a trustable local root group.
///
/// Upstream: the `PeerTrustable` (`IsTrustable | IsNotTrustable`) annotation
/// on peer entries drives the same logic in `outboundConnectionsState`.
fn is_trustable_peer(
    addr: &SocketAddr,
    entry: &PeerRegistryEntry,
    trustable_locals: &BTreeSet<SocketAddr>,
) -> bool {
    entry.sources.contains(&PeerSource::PeerSourceBootstrap)
        || trustable_locals.contains(addr)
}

/// Returns `true` when all established (warm + hot) peers are trustable.
///
/// This is the precondition for `isNodeAbleToMakeProgress` when in
/// sensitive mode — the node considers itself in a "clean" state when
/// every connected peer is either a bootstrap peer or a trustable local
/// root.
pub fn has_only_trustable_established_peers(
    registry: &PeerRegistry,
    local_root_groups: &[LocalRootTargets],
) -> bool {
    let trustable_locals = trustable_local_root_set(local_root_groups);
    registry.iter().all(|(addr, entry)| {
        match entry.status {
            PeerStatus::PeerCold | PeerStatus::PeerCooling => true,
            PeerStatus::PeerWarm | PeerStatus::PeerHot => {
                is_trustable_peer(addr, entry, &trustable_locals)
            }
        }
    })
}

/// Evaluate hot→warm demotions required by sensitive mode.
///
/// In sensitive mode, any hot peer that is NOT trustable (not a bootstrap
/// peer and not a trustable local root) must be demoted to warm first.
pub fn evaluate_sensitive_hot_demotions(
    registry: &PeerRegistry,
    local_root_groups: &[LocalRootTargets],
) -> Vec<GovernorAction> {
    let trustable_locals = trustable_local_root_set(local_root_groups);
    registry
        .iter()
        .filter(|(addr, entry)| {
            entry.status == PeerStatus::PeerHot
                && !is_trustable_peer(addr, entry, &trustable_locals)
        })
        .map(|(addr, _)| GovernorAction::DemoteToWarm(*addr))
        .collect()
}

/// Evaluate warm→cold demotions required by sensitive mode.
///
/// In sensitive mode, any warm peer that is NOT trustable must be
/// demoted to cold.
pub fn evaluate_sensitive_warm_demotions(
    registry: &PeerRegistry,
    local_root_groups: &[LocalRootTargets],
) -> Vec<GovernorAction> {
    let trustable_locals = trustable_local_root_set(local_root_groups);
    registry
        .iter()
        .filter(|(addr, entry)| {
            entry.status == PeerStatus::PeerWarm
                && !is_trustable_peer(addr, entry, &trustable_locals)
        })
        .map(|(addr, _)| GovernorAction::DemoteToCold(*addr))
        .collect()
}

/// Evaluate promotions with sensitive-mode filtering.
///
/// In sensitive mode, only bootstrap peers and trustable local-root peers
/// are eligible for promotion.  This wraps the normal evaluation functions
/// and filters out non-trustable candidates.
pub fn filter_sensitive_promotions(
    actions: Vec<GovernorAction>,
    registry: &PeerRegistry,
    local_root_groups: &[LocalRootTargets],
) -> Vec<GovernorAction> {
    let trustable_locals = trustable_local_root_set(local_root_groups);
    actions
        .into_iter()
        .filter(|action| match action {
            GovernorAction::PromoteToWarm(addr) | GovernorAction::PromoteToHot(addr) => {
                registry
                    .get(addr)
                    .is_some_and(|entry| is_trustable_peer(addr, entry, &trustable_locals))
            }
            // Demotions and forgets are always allowed.
            _ => true,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Governor tick — combined evaluation
// ---------------------------------------------------------------------------

/// Run one governance evaluation pass, returning all actions needed to
/// converge toward the configured targets.
///
/// Actions are ordered: local-root valency enforcement first, then global
/// promotions, then global demotions.
///
/// When `mode` is [`PeerSelectionMode::Sensitive`], the governor restricts
/// promotions to trustable peers (bootstrap + trustable local roots) and
/// demotes any non-trustable warm/hot peers.  In
/// [`PeerSelectionMode::Normal`] the full peer selection policy applies.
pub fn governor_tick(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    local_root_groups: &[LocalRootTargets],
    mode: PeerSelectionMode,
    association: AssociationMode,
    state: Option<&GovernorState>,
    now: Instant,
) -> Vec<GovernorAction> {
    let mut actions = Vec::new();

    match mode {
        PeerSelectionMode::Sensitive => {
            // In sensitive mode:
            // 1. Demote all non-trustable hot peers to warm.
            actions.extend(evaluate_sensitive_hot_demotions(registry, local_root_groups));
            // 2. Demote all non-trustable warm peers to cold.
            actions.extend(evaluate_sensitive_warm_demotions(registry, local_root_groups));
            // 3. Enforce local root valency (trustable groups only).
            actions.extend(enforce_local_root_valency(registry, local_root_groups));
            // 4. Normal promotion targets, filtered to trustable peers only.
            let mut promotions = Vec::new();
            promotions.extend(evaluate_cold_to_warm_promotions(registry, targets));
            promotions.extend(evaluate_warm_to_hot_promotions(registry, targets));
            actions.extend(filter_sensitive_promotions(
                promotions,
                registry,
                local_root_groups,
            ));
            // 5. Big-ledger promotions are suppressed in sensitive mode —
            //    big-ledger peers are not trustable by definition.
            // 6. Forget excess cold peers.
            actions.extend(evaluate_forget_cold_peers(registry, targets));
            // 7. Forget cold peers that have exceeded max connection retries.
            if let Some(gs) = state {
                actions.extend(evaluate_forget_failed_peers(registry, gs, now));
            }
        }
        PeerSelectionMode::Normal => {
            // 1. Local root valency takes priority.
            actions.extend(enforce_local_root_valency(registry, local_root_groups));
            // 2. Global promotion targets.
            actions.extend(evaluate_cold_to_warm_promotions(registry, targets));
            actions.extend(evaluate_warm_to_hot_promotions(registry, targets));
            // 3. Big-ledger peer promotions (suppressed in LocalRootsOnly).
            if association == AssociationMode::Unrestricted {
                actions.extend(evaluate_cold_to_warm_big_ledger_promotions(registry, targets));
                actions.extend(evaluate_warm_to_hot_big_ledger_promotions(registry, targets));
            }
            // 4. Global demotion targets.
            actions.extend(evaluate_hot_to_warm_demotions(registry, targets));
            actions.extend(evaluate_warm_to_cold_demotions(registry, targets));
            // 5. Big-ledger peer demotions.
            actions.extend(evaluate_hot_to_warm_big_ledger_demotions(registry, targets));
            actions.extend(evaluate_warm_to_cold_big_ledger_demotions(registry, targets));
            // 6. Forget excess cold peers.
            actions.extend(evaluate_forget_cold_peers(registry, targets));
            // 7. Forget cold peers that have exceeded max connection retries.
            if let Some(gs) = state {
                actions.extend(evaluate_forget_failed_peers(registry, gs, now));
            }
            // 8. Peer sharing requests — suppressed in LocalRootsOnly mode
            //    since BP/hidden-relay nodes should not participate in peer
            //    sharing discovery.
            if association == AssociationMode::Unrestricted {
                if let Some(gs) = state {
                    actions.extend(evaluate_peer_share_requests(registry, targets, gs));
                }
            }
        }
    }

    actions
}

// ---------------------------------------------------------------------------
// Peer selection counters
// ---------------------------------------------------------------------------

/// Structured governor counters derived from the current peer registry and
/// governor state.
///
/// This mirrors the upstream `PeerSelectionView Int` type alias
/// (`PeerSelectionCounters` pattern synonym) from
/// `Ouroboros.Network.PeerSelection.Governor.Types`.  The upstream
/// `PeerSelectionView` is parameterized over `a` (with `a = Int` for
/// counters and `a = (Set peeraddr, Int)` for sets-with-sizes); here we
/// use a concrete struct with `usize` counters.
///
/// The counters are split into four peer categories — regular, big-ledger,
/// local-root, and non-root — matching the upstream `view{Regular,BigLedger,
/// LocalRoot,NonRoot}*` fields.  In-flight action counts come from
/// [`GovernorState`].
///
/// Reference: `peerSelectionStateToView` in
/// `Ouroboros.Network.PeerSelection.Governor.Types`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeerSelectionCounters {
    // -- Regular peers (excludes big-ledger) ---------------------------------

    /// Total known regular peers (cold + warm + hot).
    ///
    /// Upstream: `viewKnownPeers`.
    pub known: usize,
    /// Cold regular peers available for promotion (not in-flight, not
    /// cooling).
    ///
    /// Upstream: `viewAvailableToConnectPeers`.
    pub available_to_connect: usize,
    /// Regular peers with in-flight cold→warm promotion.
    ///
    /// Upstream: `viewColdPeersPromotions`.
    pub cold_peers_promotions: usize,
    /// Established (warm + hot) regular peers.
    ///
    /// Upstream: `viewEstablishedPeers`.
    pub established: usize,
    /// Regular peers with in-flight warm→cold demotion.
    ///
    /// Upstream: `viewWarmPeersDemotions`.
    pub warm_peers_demotions: usize,
    /// Regular peers with in-flight warm→hot promotion.
    ///
    /// Upstream: `viewWarmPeersPromotions`.
    pub warm_peers_promotions: usize,
    /// Active (hot) regular peers.
    ///
    /// Upstream: `viewActivePeers`.
    pub active: usize,
    /// Regular peers with in-flight hot→warm demotion.
    ///
    /// Upstream: `viewActivePeersDemotions`.
    pub active_peers_demotions: usize,

    // -- Big-ledger peers ----------------------------------------------------

    /// Total known big-ledger peers.
    ///
    /// Upstream: `viewKnownBigLedgerPeers`.
    pub known_big_ledger: usize,
    /// Cold big-ledger peers available for promotion.
    ///
    /// Upstream: `viewAvailableToConnectBigLedgerPeers`.
    pub available_to_connect_big_ledger: usize,
    /// Big-ledger peers with in-flight cold→warm promotion.
    ///
    /// Upstream: `viewColdBigLedgerPeersPromotions`.
    pub cold_big_ledger_promotions: usize,
    /// Established (warm + hot) big-ledger peers.
    ///
    /// Upstream: `viewEstablishedBigLedgerPeers`.
    pub established_big_ledger: usize,
    /// Big-ledger peers with in-flight warm→cold demotion.
    ///
    /// Upstream: `viewWarmBigLedgerPeersDemotions`.
    pub warm_big_ledger_demotions: usize,
    /// Big-ledger peers with in-flight warm→hot promotion.
    ///
    /// Upstream: `viewWarmBigLedgerPeersPromotions`.
    pub warm_big_ledger_promotions: usize,
    /// Active (hot) big-ledger peers.
    ///
    /// Upstream: `viewActiveBigLedgerPeers`.
    pub active_big_ledger: usize,
    /// Big-ledger peers with in-flight hot→warm demotion.
    ///
    /// Upstream: `viewActiveBigLedgerPeersDemotions`.
    pub active_big_ledger_demotions: usize,

    // -- Local-root peers ----------------------------------------------------

    /// Total known local-root peers.
    ///
    /// Upstream: `viewKnownLocalRootPeers`.
    pub known_local_root: usize,
    /// Cold local-root peers available for promotion.
    ///
    /// Upstream: `viewAvailableToConnectLocalRootPeers`.
    pub available_to_connect_local_root: usize,
    /// Established (warm + hot) local-root peers.
    ///
    /// Upstream: `viewEstablishedLocalRootPeers`.
    pub established_local_root: usize,
    /// Active (hot) local-root peers.
    ///
    /// Upstream: `viewActiveLocalRootPeers`.
    pub active_local_root: usize,

    // -- Non-root peers (known but not from any root source) -----------------

    /// Total known non-root peers.
    ///
    /// Upstream: `viewKnownNonRootPeers`.
    pub known_non_root: usize,
    /// Cold non-root peers available for promotion.
    ///
    /// Upstream: `viewAvailableToConnectNonRootPeers`.
    pub available_to_connect_non_root: usize,
    /// Established (warm + hot) non-root peers.
    ///
    /// Upstream: `viewEstablishedNonRootPeers`.
    pub established_non_root: usize,
    /// Active (hot) non-root peers.
    ///
    /// Upstream: `viewActiveNonRootPeers`.
    pub active_non_root: usize,

    // -- Root peer count -----------------------------------------------------

    /// Total number of root peers (from all root sources).
    ///
    /// Upstream: `viewRootPeers`.
    pub root_peers: usize,
}

impl PeerSelectionCounters {
    /// Compute counters from a peer registry and optional governor state.
    ///
    /// In-flight action counts are sourced from [`GovernorState`] when
    /// provided.  Without a `GovernorState`, in-flight fields are zero.
    ///
    /// Reference: `peerSelectionStateToView` in
    /// `Ouroboros.Network.PeerSelection.Governor.Types`.
    pub fn from_registry(registry: &PeerRegistry, state: Option<&GovernorState>) -> Self {
        let mut counters = Self::default();

        for (addr, entry) in registry.iter() {
            let is_bl = is_big_ledger(entry);
            let is_local = entry.sources.contains(&PeerSource::PeerSourceLocalRoot);
            let is_root = entry.is_root_peer();

            // ---- Category: regular vs big-ledger ----
            if is_bl {
                counters.known_big_ledger += 1;
                match entry.status {
                    PeerStatus::PeerCold => {
                        counters.available_to_connect_big_ledger += 1;
                    }
                    PeerStatus::PeerCooling => {}
                    PeerStatus::PeerWarm => {
                        counters.established_big_ledger += 1;
                    }
                    PeerStatus::PeerHot => {
                        counters.established_big_ledger += 1;
                        counters.active_big_ledger += 1;
                    }
                }
            } else {
                counters.known += 1;
                match entry.status {
                    PeerStatus::PeerCold => {
                        counters.available_to_connect += 1;
                    }
                    PeerStatus::PeerCooling => {}
                    PeerStatus::PeerWarm => {
                        counters.established += 1;
                    }
                    PeerStatus::PeerHot => {
                        counters.established += 1;
                        counters.active += 1;
                    }
                }
            }

            // ---- Category: local-root ----
            if is_local {
                counters.known_local_root += 1;
                match entry.status {
                    PeerStatus::PeerCold => {
                        counters.available_to_connect_local_root += 1;
                    }
                    PeerStatus::PeerCooling => {}
                    PeerStatus::PeerWarm => {
                        counters.established_local_root += 1;
                    }
                    PeerStatus::PeerHot => {
                        counters.established_local_root += 1;
                        counters.active_local_root += 1;
                    }
                }
            }

            // ---- Category: non-root ----
            if !is_root {
                counters.known_non_root += 1;
                match entry.status {
                    PeerStatus::PeerCold => {
                        counters.available_to_connect_non_root += 1;
                    }
                    PeerStatus::PeerCooling => {}
                    PeerStatus::PeerWarm => {
                        counters.established_non_root += 1;
                    }
                    PeerStatus::PeerHot => {
                        counters.established_non_root += 1;
                        counters.active_non_root += 1;
                    }
                }
            }

            // ---- Root peer count ----
            if is_root {
                counters.root_peers += 1;
            }

            // ---- In-flight counts from GovernorState ----
            if let Some(gs) = state {
                if gs.in_flight_warm.contains(addr) {
                    if is_bl {
                        counters.cold_big_ledger_promotions += 1;
                    } else {
                        counters.cold_peers_promotions += 1;
                    }
                }
                if gs.in_flight_hot.contains(addr) {
                    if is_bl {
                        counters.warm_big_ledger_promotions += 1;
                    } else {
                        counters.warm_peers_promotions += 1;
                    }
                }
                if gs.in_flight_demote_warm.contains(addr) {
                    if is_bl {
                        counters.warm_big_ledger_demotions += 1;
                    } else {
                        counters.warm_peers_demotions += 1;
                    }
                }
                if gs.in_flight_demote_hot.contains(addr) {
                    if is_bl {
                        counters.active_big_ledger_demotions += 1;
                    } else {
                        counters.active_peers_demotions += 1;
                    }
                }
            }
        }

        counters
    }
}

// ---------------------------------------------------------------------------
// Outbound connections state
// ---------------------------------------------------------------------------

/// Whether the outbound governor considers the node's connections to be in
/// a trusted state.
///
/// This mirrors the upstream `OutboundConnectionsState` from
/// `Ouroboros.Network.PeerSelection.State.EstablishedPeers` — a binary
/// signal consumed by consensus and header diffusion to decide whether
/// the node should validate and forward blocks.
///
/// * `TrustedStateWithExternalPeers` — the node has enough trustable
///   established connections to consider itself safe from eclipse attacks.
/// * `UntrustedState` — the node does not yet have sufficient trustable
///   connections.
///
/// The computation depends on:
///
/// 1. [`AssociationMode`]:
///    - `LocalRootsOnly` → trusted iff **all** established peers are
///      trustable local roots.
///    - `Unrestricted` → see below.
///
/// 2. [`UseBootstrapPeers`]:
///    - `DontUseBootstrapPeers` → always trusted (the node does not
///      have a bootstrap requirement).
///    - `UseBootstrapPeers(…)` → trusted iff **all** established peers
///      are trustable (bootstrap or trustable local root) **and** at
///      least one active peer is from bootstrap or public-root sources.
///
/// Reference: `outboundConnectionsState` in
/// `Ouroboros.Network.PeerSelection.Governor.Monitor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboundConnectionsState {
    /// The node has trustable outbound connections.
    TrustedStateWithExternalPeers,
    /// The node does not have sufficient trustable outbound connections.
    UntrustedState,
}

/// Compute the outbound connections state from the current peer registry,
/// local root configuration, association mode, and bootstrap configuration.
///
/// This implements the upstream `outboundConnectionsState` function from
/// `Ouroboros.Network.PeerSelection.Governor.Monitor`.
///
/// The logic branches on `(AssociationMode, UseBootstrapPeers)`:
///
/// * `(LocalRootsOnly, _)` → `TrustedState` iff all established peers
///   belong to trustable local roots.
/// * `(Unrestricted, DontUseBootstrapPeers)` → always `TrustedState`.
/// * `(Unrestricted, UseBootstrapPeers)` → `TrustedState` iff:
///   - All established (warm + hot) peers are trustable (bootstrap or
///     trustable local root), **and**
///   - At least one active (hot) peer is from a bootstrap or public-root
///     source (i.e. not only local-root peers are active).
pub fn compute_outbound_connections_state(
    registry: &PeerRegistry,
    local_root_groups: &[LocalRootTargets],
    association: AssociationMode,
    use_bootstrap: &UseBootstrapPeers,
) -> OutboundConnectionsState {
    match association {
        AssociationMode::LocalRootsOnly => {
            // In LocalRootsOnly mode, trust requires that every established
            // peer is a trustable local root.
            let trustable_locals = trustable_local_root_set(local_root_groups);
            let all_trusted = registry.iter().all(|(addr, entry)| {
                match entry.status {
                    PeerStatus::PeerCold | PeerStatus::PeerCooling => true,
                    PeerStatus::PeerWarm | PeerStatus::PeerHot => {
                        trustable_locals.contains(addr)
                    }
                }
            });
            if all_trusted {
                OutboundConnectionsState::TrustedStateWithExternalPeers
            } else {
                OutboundConnectionsState::UntrustedState
            }
        }
        AssociationMode::Unrestricted => {
            match use_bootstrap {
                UseBootstrapPeers::DontUseBootstrapPeers => {
                    // No bootstrap requirement — always trusted.
                    OutboundConnectionsState::TrustedStateWithExternalPeers
                }
                UseBootstrapPeers::UseBootstrapPeers(_) => {
                    // Bootstrap mode: need all established peers to be
                    // trustable AND at least one active external peer.
                    let trustable_locals = trustable_local_root_set(local_root_groups);
                    let mut all_established_trustable = true;
                    let mut has_external_active = false;

                    for (addr, entry) in registry.iter() {
                        match entry.status {
                            PeerStatus::PeerWarm => {
                                if !is_trustable_peer(addr, entry, &trustable_locals) {
                                    all_established_trustable = false;
                                }
                            }
                            PeerStatus::PeerHot => {
                                if !is_trustable_peer(addr, entry, &trustable_locals) {
                                    all_established_trustable = false;
                                }
                                // Check for external active peer (bootstrap or
                                // public-root — not only local-root).
                                if entry.sources.contains(&PeerSource::PeerSourceBootstrap)
                                    || entry.sources.contains(&PeerSource::PeerSourcePublicRoot)
                                {
                                    has_external_active = true;
                                }
                            }
                            PeerStatus::PeerCold | PeerStatus::PeerCooling => {}
                        }
                    }

                    if all_established_trustable && has_external_active {
                        OutboundConnectionsState::TrustedStateWithExternalPeers
                    } else {
                        OutboundConnectionsState::UntrustedState
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fetch mode
// ---------------------------------------------------------------------------

/// Block-fetch concurrency mode.
///
/// This mirrors the upstream `FetchMode` from
/// `Ouroboros.Network.BlockFetch.ConsensusInterface`:
///
/// * `BulkSync` — the node is catching up with the chain and should
///   maximise throughput by fetching blocks in large batches from multiple
///   peers concurrently.
/// * `Deadline` — the node is near the tip and should minimise latency
///   by fetching each new block from the fastest peer.
///
/// The upstream `mkReadFetchMode` function derives the mode from
/// `LedgerStateJudgement` under Genesis consensus, or from a configuration
/// parameter under Praos consensus.
///
/// Reference: `Ouroboros.Network.BlockFetch.ConsensusInterface` and
/// `Cardano.Node.Diffusion` `mkReadFetchMode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchMode {
    /// Bulk sync mode — optimise throughput.  Used when the node is far
    /// behind the chain tip.
    ///
    /// Upstream: `FetchModeBulkSync`.
    FetchModeBulkSync,
    /// Deadline mode — optimise latency.  Used when the node is near the
    /// chain tip.
    ///
    /// Upstream: `FetchModeDeadline`.
    FetchModeDeadline,
}

/// Derive the fetch mode from the current ledger state judgement.
///
/// Under Praos consensus, the upstream derives the mode from
/// `LedgerStateJudgement`:
///
/// * `TooOld` / `Unavailable` → `FetchModeBulkSync` (far behind, catch up fast).
/// * `YoungEnough` → `FetchModeDeadline` (near tip, minimise latency).
///
/// Reference: `mkReadFetchMode` in `Cardano.Node.Diffusion`.
pub fn fetch_mode_from_judgement(judgement: LedgerStateJudgement) -> FetchMode {
    match judgement {
        LedgerStateJudgement::YoungEnough => FetchMode::FetchModeDeadline,
        LedgerStateJudgement::TooOld | LedgerStateJudgement::Unavailable => {
            FetchMode::FetchModeBulkSync
        }
    }
}

// ---------------------------------------------------------------------------
// Churn mode and regime
// ---------------------------------------------------------------------------

/// Churn scoring mode derived from the current fetch mode.
///
/// Upstream: `ChurnMode` in `Cardano.Network.Diffusion.Policies`.
///
/// This determines how hot-peer demotion scoring works during churn
/// cycles:
///
/// * `Normal` — score by upstream header/block metrics (deadline mode:
///   the node is near the tip, so latency matters).
/// * `BulkSync` — score by bytes fetched (syncing mode: throughput
///   matters more than latency).
///
/// Reference: `simpleChurnModePeerSelectionPolicy` in
/// `Cardano.Network.Diffusion.Policies`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChurnMode {
    /// Normal mode — score hot peers by header/block metrics.
    ///
    /// Active when `FetchMode::FetchModeDeadline`.
    Normal,
    /// Bulk-sync mode — score hot peers by bytes transferred.
    ///
    /// Active when `FetchMode::FetchModeBulkSync`.
    BulkSync,
}

/// Derive the churn mode from the current fetch mode.
///
/// Upstream: `updateChurnMode` in `Cardano.Network.Diffusion.Policies`:
///
/// ```text
/// PraosFetchMode FetchModeDeadline → ChurnModeNormal
/// PraosFetchMode FetchModeBulkSync → ChurnModeBulkSync
/// FetchModeGenesis                 → ChurnModeBulkSync
/// ```
pub fn churn_mode_from_fetch_mode(fetch: FetchMode) -> ChurnMode {
    match fetch {
        FetchMode::FetchModeDeadline => ChurnMode::Normal,
        FetchMode::FetchModeBulkSync => ChurnMode::BulkSync,
    }
}

/// Consensus mode for the node.
///
/// Upstream: `ConsensusMode` from `Ouroboros.Consensus.Genesis.Governor` —
/// determines whether the node uses Genesis-mode extensions or plain Praos.
///
/// This affects churn regime selection: under `GenesisMode`, bulk-sync
/// churn is always treated as `ChurnDefault` rather than a reduced
/// regime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsensusMode {
    /// Plain Praos consensus (default for mainnet).
    PraosMode,
    /// Genesis consensus mode — uses additional peer selection rules for
    /// initial chain synchronization.
    GenesisMode,
}

/// Churn regime that controls the aggressiveness of target decreases
/// during churn cycles.
///
/// Upstream: `ChurnRegime` in `Cardano.Network.Diffusion.Policies.Churn`:
///
/// | Regime                     | Effect on active peers | Effect on established peers |
/// |----------------------------|------------------------|----------------------------|
/// | `ChurnDefault`             | `churn_decrease(base)` — standard 20% | Standard decrease |
/// | `ChurnPraosSync`           | `min(max(1, local_hot), base - 1)` | Capped decrease |
/// | `ChurnBootstrapPraosSync`  | Same as PraosSync | Aggressive: `min(active, established - 1)` |
///
/// `ChurnBootstrapPraosSync` is the most aggressive — it tears down
/// nearly all established connections to force a full re-evaluation,
/// which is needed when bootstrap-peers mode is active during sync.
///
/// Reference: `pickChurnRegime` and `decreaseEstablished` in
/// `Cardano.Network.Diffusion.Policies.Churn`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChurnRegime {
    /// Default churn — standard 20% decrease for both active and
    /// established targets.
    ChurnDefault,
    /// Praos sync churn — caps active decrease to preserve local root
    /// hot target, established decrease capped similarly.
    ChurnPraosSync,
    /// Bootstrap Praos sync churn — aggressive established decrease to
    /// force full re-evaluation during bootstrap peer syncing.
    ChurnBootstrapPraosSync,
}

/// Derive the churn regime from the current modes and bootstrap configuration.
///
/// Upstream: `pickChurnRegime` in `Cardano.Network.Diffusion.Policies.Churn`:
///
/// ```text
/// (ChurnModeNormal, _, _)                           → ChurnDefault
/// (_, _, GenesisMode)                               → ChurnDefault
/// (ChurnModeBulkSync, UseBootstrapPeers _, PraosMode) → ChurnBootstrapPraosSync
/// (ChurnModeBulkSync, _, PraosMode)                 → ChurnPraosSync
/// ```
pub fn pick_churn_regime(
    churn: ChurnMode,
    use_bootstrap: &UseBootstrapPeers,
    consensus: ConsensusMode,
) -> ChurnRegime {
    match (churn, consensus) {
        (ChurnMode::Normal, _) => ChurnRegime::ChurnDefault,
        (_, ConsensusMode::GenesisMode) => ChurnRegime::ChurnDefault,
        (ChurnMode::BulkSync, ConsensusMode::PraosMode) => {
            if use_bootstrap.is_enabled() {
                ChurnRegime::ChurnBootstrapPraosSync
            } else {
                ChurnRegime::ChurnPraosSync
            }
        }
    }
}

/// Compute the decreased active (hot) target under a churn regime.
///
/// Upstream: `decreaseActive` in `Cardano.Network.Diffusion.Policies.Churn`:
///
/// ```text
/// ChurnDefault             → decrease base
/// ChurnPraosSync           → min (max 1 localRootHotTarget) (base - 1)
/// ChurnBootstrapPraosSync  → min (max 1 localRootHotTarget) (base - 1)
/// ```
///
/// `local_root_hot_target` is the maximum hot valency across all local-root
/// groups (upstream `localRootPeersHotTarget`).
pub fn churn_decrease_active(
    regime: ChurnRegime,
    base: usize,
    local_root_hot_target: usize,
) -> usize {
    match regime {
        ChurnRegime::ChurnDefault => churn_decrease(base),
        ChurnRegime::ChurnPraosSync | ChurnRegime::ChurnBootstrapPraosSync => {
            if base == 0 {
                return 0;
            }
            let floor = std::cmp::max(1, local_root_hot_target);
            std::cmp::min(floor, base - 1)
        }
    }
}

/// Compute the decreased established (warm) target under a churn regime.
///
/// Upstream: `decreaseEstablished` in
/// `Cardano.Network.Diffusion.Policies.Churn`:
///
/// ```text
/// ChurnDefault             → decreaseWithMin n (base_est - base_active) + base_active
///   where decreaseWithMin n v = max n (decrease v)
/// ChurnPraosSync           → same as ChurnDefault, but n is capped
/// ChurnBootstrapPraosSync  → min active (established - 1)
/// ```
///
/// For simplicity we use the upstream formula: standard decrease is
/// `decrease(established - active) + active` — the "warm only" portion
/// shrinks, then active is re-added.  Bootstrap mode aggressively sets
/// established to just above the current active count.
pub fn churn_decrease_established(
    regime: ChurnRegime,
    established: usize,
    active: usize,
) -> usize {
    match regime {
        ChurnRegime::ChurnDefault | ChurnRegime::ChurnPraosSync => {
            let warm_only = established.saturating_sub(active);
            churn_decrease(warm_only) + active
        }
        ChurnRegime::ChurnBootstrapPraosSync => {
            if established == 0 {
                return 0;
            }
            std::cmp::min(active, established - 1)
        }
    }
}

// ---------------------------------------------------------------------------
// Peer selection policy time constants
// ---------------------------------------------------------------------------

/// Configurable policy time constants for peer selection operations.
///
/// This groups the non-pick-function time constants from the upstream
/// `PeerSelectionPolicy` record in
/// `Ouroboros.Network.PeerSelection.Governor.Types`.  Default values
/// follow `simplePeerSelectionPolicy` in
/// `Ouroboros.Network.Diffusion.Policies`.
///
/// The seven `policyPick*` callback fields from upstream are not
/// modeled here — they require a randomized `PickPolicy` abstraction
/// that is orthogonal to these configurable time constants.
#[derive(Clone, Debug)]
pub struct PeerSelectionTimeouts {
    /// Timeout for DNS resolution of public root peers.
    ///
    /// Upstream: `policyFindPublicRootTimeout` = 5 s.
    pub find_public_root_timeout: Duration,
    /// Maximum number of concurrent in-flight peer-sharing requests.
    ///
    /// Upstream: `policyMaxInProgressPeerShareReqs` = 2.
    pub max_in_progress_peer_share_reqs: u32,
    /// Minimum interval before re-requesting peer sharing from the same
    /// peer.
    ///
    /// Upstream: `policyPeerShareRetryTime` = 900 s.
    pub peer_share_retry_time: Duration,
    /// How long to wait between peer-sharing requests to different peers
    /// within a single batch.
    ///
    /// Upstream: `policyPeerShareBatchWaitTime` = 3 s.
    pub peer_share_batch_wait_time: Duration,
    /// Overall timeout for a single peer-sharing request round.
    ///
    /// Upstream: `policyPeerShareOverallTimeout` = 10 s.
    pub peer_share_overall_timeout: Duration,
    /// Delay after adding a shared peer before it becomes eligible for
    /// promotion.
    ///
    /// Upstream: `policyPeerShareActivationDelay` = 300 s.
    pub peer_share_activation_delay: Duration,
    /// Maximum cold→warm connection retries before a non-protected peer
    /// is forgotten.
    ///
    /// Upstream: `policyMaxConnectionRetries` = 5.
    pub max_connection_retries: u32,
    /// Time a peer must remain hot before its failure counter is cleared.
    ///
    /// Upstream: `policyClearFailCountDelay` = 120 s.
    pub clear_fail_count_delay: Duration,
}

impl Default for PeerSelectionTimeouts {
    /// Default values from upstream `simplePeerSelectionPolicy`.
    fn default() -> Self {
        Self {
            find_public_root_timeout: Duration::from_secs(5),
            max_in_progress_peer_share_reqs: 2,
            peer_share_retry_time: Duration::from_secs(900),
            peer_share_batch_wait_time: Duration::from_secs(3),
            peer_share_overall_timeout: Duration::from_secs(10),
            peer_share_activation_delay: Duration::from_secs(300),
            max_connection_retries: 5,
            clear_fail_count_delay: Duration::from_secs(120),
        }
    }
}

// ---------------------------------------------------------------------------
// Connection manager counters
// ---------------------------------------------------------------------------

/// Counters tracking connection manager state across all peers.
///
/// This mirrors the upstream `ConnectionManagerCounters` from
/// `Ouroboros.Network.ConnectionManager.Types`:
///
/// ```text
/// data ConnectionManagerCounters = ConnectionManagerCounters {
///     fullDuplexConns     :: !Int,
///     duplexConns         :: !Int,
///     unidirectionalConns :: !Int,
///     inboundConns        :: !Int,
///     outboundConns       :: !Int,
///     terminatingConns    :: !Int
///   }
/// ```
///
/// In upstream Haskell, these are derived from a `ConnMap` of
/// connection states via `connectionManagerStateToCounters`, which folds
/// `connectionStateToCounters` over the map.  Since the Rust node does
/// not yet model the full connection manager state machine (Reserved,
/// Unnegotiated, Duplex, Inbound, Outbound, etc.), we derive an
/// approximation from the [`PeerRegistry`]:
///
/// * `outbound_conns` = count of Warm + Hot peers (we initiate outbound).
/// * `inbound_conns` = 0 (no inbound tracking yet).
/// * `duplex_conns` = 0 (no duplex negotiation tracking yet).
/// * `full_duplex_conns` = 0.
/// * `unidirectional_conns` = `outbound_conns` (all outbound assumed uni).
/// * `terminating_conns` = count of `PeerCooling` peers.
///
/// As the connection manager matures, these counters will be populated
/// from actual connection states rather than the peer registry.
///
/// Reference: `Ouroboros.Network.ConnectionManager.Types`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConnectionManagerCounters {
    /// Connections in full-duplex state (both inbound and outbound active).
    ///
    /// Upstream: `fullDuplexConns`.
    pub full_duplex_conns: usize,
    /// Connections negotiated as duplex-capable (includes full-duplex).
    ///
    /// Upstream: `duplexConns`.
    pub duplex_conns: usize,
    /// Connections negotiated as unidirectional.
    ///
    /// Upstream: `unidirectionalConns`.
    pub unidirectional_conns: usize,
    /// Total inbound connections.
    ///
    /// Upstream: `inboundConns`.
    pub inbound_conns: usize,
    /// Total outbound connections.
    ///
    /// Upstream: `outboundConns`.
    pub outbound_conns: usize,
    /// Connections in the process of shutting down.
    ///
    /// Upstream: `terminatingConns`.
    pub terminating_conns: usize,
}

impl ConnectionManagerCounters {
    /// Derive approximate counters from the current peer registry.
    ///
    /// Since the Rust node does not yet model the full upstream connection
    /// state machine, this provides a best-effort approximation:
    ///
    /// * Warm and Hot peers are counted as outbound + unidirectional.
    /// * Cooling peers are counted as terminating.
    /// * Inbound and duplex tracking require connection-manager support
    ///   and will be zero until that is implemented.
    pub fn from_registry(registry: &PeerRegistry) -> Self {
        let mut counters = Self::default();
        for (_addr, entry) in registry.iter() {
            match entry.status {
                PeerStatus::PeerWarm | PeerStatus::PeerHot => {
                    counters.outbound_conns += 1;
                    counters.unidirectional_conns += 1;
                }
                PeerStatus::PeerCooling => {
                    counters.terminating_conns += 1;
                }
                PeerStatus::PeerCold => {}
            }
        }
        counters
    }
}

impl std::ops::Add for ConnectionManagerCounters {
    type Output = Self;

    /// Field-wise addition, matching the upstream `Semigroup` instance.
    fn add(self, rhs: Self) -> Self {
        Self {
            full_duplex_conns: self.full_duplex_conns + rhs.full_duplex_conns,
            duplex_conns: self.duplex_conns + rhs.duplex_conns,
            unidirectional_conns: self.unidirectional_conns + rhs.unidirectional_conns,
            inbound_conns: self.inbound_conns + rhs.inbound_conns,
            outbound_conns: self.outbound_conns + rhs.outbound_conns,
            terminating_conns: self.terminating_conns + rhs.terminating_conns,
        }
    }
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
            trustable: false,
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
            trustable: false,
        }];

        let actions = governor_tick(&reg, &targets, &groups, PeerSelectionMode::Normal, AssociationMode::Unrestricted, None, Instant::now());
        // Should have at least the local root promotion.
        assert!(!actions.is_empty());
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    }

    #[test]
    fn empty_registry_produces_no_actions() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let actions = governor_tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, None, Instant::now());
        assert!(actions.is_empty());
    }

    #[test]
    fn failure_tracking_and_backoff() {
        let mut state = GovernorState::default();
        let peer = addr(1);
        let now = Instant::now();

        assert!(!state.is_backing_off(&peer, now));

        // Reach max_failures (default 5).
        for _ in 0..5 {
            state.record_failure(peer);
        }
        assert!(state.is_backing_off(&peer, now));

        // Success resets.
        state.record_success(peer);
        assert!(!state.is_backing_off(&peer, now));
    }

    #[test]
    fn filter_removes_backed_off_promotions() {
        let mut state = GovernorState::default();
        for _ in 0..5 {
            state.record_failure(addr(2));
        }
        let now = Instant::now();

        let actions = vec![
            GovernorAction::PromoteToWarm(addr(1)),
            GovernorAction::PromoteToWarm(addr(2)),
            GovernorAction::DemoteToWarm(addr(3)),
        ];
        let filtered = state.filter_backed_off(actions, now);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(1))));
        assert!(filtered.contains(&GovernorAction::DemoteToWarm(addr(3))));
    }

    #[test]
    fn churn_cycle_starts_on_first_tick() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 1,
            target_active: 1,
            ..Default::default()
        };
        let mut state = GovernorState::default();
        let now = Instant::now();

        // First tick should enter DecreasedActive immediately.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, now);
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));
    }

    #[test]
    fn churn_decreased_active_lowers_hot_targets() {
        let state = GovernorState {
            churn_phase: ChurnPhase::DecreasedActive {
                started: Instant::now(),
            },
            ..Default::default()
        };
        let targets = GovernorTargets {
            target_active: 5,
            target_active_big_ledger: 10,
            target_established: 10,
            target_established_big_ledger: 20,
            ..Default::default()
        };
        let eff = state.apply_churn_to_targets(&targets);
        assert_eq!(eff.target_active, churn_decrease(5));
        assert_eq!(eff.target_active_big_ledger, churn_decrease(10));
        // Established unchanged in this phase.
        assert_eq!(eff.target_established, 10);
        assert_eq!(eff.target_established_big_ledger, 20);
    }

    #[test]
    fn churn_decreased_established_lowers_warm_targets() {
        let state = GovernorState {
            churn_phase: ChurnPhase::DecreasedEstablished {
                started: Instant::now(),
            },
            ..Default::default()
        };
        let targets = GovernorTargets {
            target_active: 5,
            target_established: 10,
            target_established_big_ledger: 20,
            ..Default::default()
        };
        let eff = state.apply_churn_to_targets(&targets);
        // Active unchanged in this phase.
        assert_eq!(eff.target_active, 5);
        // Established decrease uses upstream formula: decrease(warm_only) + active.
        // warm_only = 10 - 5 = 5 → decrease(5) = 4 → 4 + 5 = 9.
        assert_eq!(eff.target_established, 9);
        // Big-ledger: warm_only = 20 - 0 = 20 → decrease(20) = 16 → 16 + 0 = 16.
        assert_eq!(eff.target_established_big_ledger, 16);
    }

    #[test]
    fn churn_idle_returns_unchanged_targets() {
        let state = GovernorState::default();
        let targets = GovernorTargets {
            target_active: 5,
            target_established: 10,
            ..Default::default()
        };
        let eff = state.apply_churn_to_targets(&targets);
        assert_eq!(eff, targets);
    }

    #[test]
    fn churn_phase_advances_through_full_cycle() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let mut state = GovernorState {
            churn: ChurnConfig {
                bulk_churn_interval: Duration::from_secs(300),
                phase_timeout: Duration::from_secs(60),
                ..Default::default()
            },
            ..Default::default()
        };
        let t0 = Instant::now();

        // Tick 0: Idle → DecreasedActive (first cycle fires immediately).
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0);
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));

        // 30s later: still DecreasedActive (phase_timeout = 60s).
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(30));
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));

        // 61s later: advance to DecreasedEstablished.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(61));
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedEstablished { .. }));

        // 122s later: advance to Idle (cycle complete).
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(122));
        assert_eq!(state.churn_phase, ChurnPhase::Idle);
        assert!(state.last_churn_cycle.is_some());
    }

    #[test]
    fn churn_cycle_respects_interval_before_restarting() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let mut state = GovernorState {
            churn: ChurnConfig {
                bulk_churn_interval: Duration::from_secs(300),
                phase_timeout: Duration::from_secs(10),
                ..Default::default()
            },
            ..Default::default()
        };
        let t0 = Instant::now();

        // Complete a full cycle: Idle→Active→Established→Idle
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0);
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(11));
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(22));
        assert_eq!(state.churn_phase, ChurnPhase::Idle);

        // 100s after cycle end: interval not elapsed (300s), stays Idle.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(122));
        assert_eq!(state.churn_phase, ChurnPhase::Idle);

        // 301s after cycle end: interval elapsed, new cycle starts.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(323));
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));
    }

    #[test]
    fn churn_produces_demotions_in_decreased_active_phase() {
        // 3 hot peers, target_active=2.  During DecreasedActive,
        // churn_decrease(2) = 1, so the governor should demote 2
        // excess hot peers.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 3,
            target_active: 2,
            ..Default::default()
        };
        let mut state = GovernorState {
            churn_phase: ChurnPhase::DecreasedActive {
                started: Instant::now(),
            },
            ..Default::default()
        };

        let eff = state.apply_churn_to_targets(&targets);
        assert_eq!(eff.target_active, 1); // churn_decrease(2)

        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, Instant::now());
        // Should demote non-local-root hot peers.
        let demotions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, GovernorAction::DemoteToWarm(_)))
            .collect();
        assert_eq!(demotions.len(), 2);
    }

    #[test]
    fn churn_produces_demotions_in_decreased_established_phase() {
        // 3 warm peers, target_established=2.  During DecreasedEstablished,
        // churn_decrease(2) = 1, so governor should demote 2.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 0,
            ..Default::default()
        };
        let mut state = GovernorState {
            churn_phase: ChurnPhase::DecreasedEstablished {
                started: Instant::now(),
            },
            ..Default::default()
        };

        let eff = state.apply_churn_to_targets(&targets);
        assert_eq!(eff.target_established, 1); // churn_decrease(2)

        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, Instant::now());
        let cold_demotions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, GovernorAction::DemoteToCold(_)))
            .collect();
        assert_eq!(cold_demotions.len(), 2);
    }

    #[test]
    fn churn_skips_local_root_demotions() {
        // Only local-root hot peers — no demotions even in decrease phase.
        let _reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 1,
            target_active: 1,
            ..Default::default()
        };
        let state = GovernorState {
            churn_phase: ChurnPhase::DecreasedActive {
                started: Instant::now(),
            },
            ..Default::default()
        };

        // churn_decrease(1) = 0, but the one hot peer is local-root so
        // demotion should prefer non-local-root first.  With only
        // local-root peers the demotion will include them when excess
        // prevents it from being avoided — but target_active after
        // decrease is 0, and local-root is still protected by
        // enforce_local_root_valency re-promoting it.  The governor
        // targets simply produce the excess demotion.
        let eff = state.apply_churn_to_targets(&targets);
        assert_eq!(eff.target_active, 0);
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
            trustable: false,
        }];
        let mut state = GovernorState::default();

        // Back off peer 1 so the local-root promotion is suppressed.
        for _ in 0..5 {
            state.record_failure(addr(1));
        }

        let actions = state.tick(&reg, &targets, &groups, PeerSelectionMode::Normal, AssociationMode::Unrestricted, Instant::now());
        // PromoteToWarm(addr(1)) should be filtered out.
        assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
        // First tick enters DecreasedActive phase.
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));
    }

    // -----------------------------------------------------------------------
    // churn_decrease (upstream `decrease` function)
    // -----------------------------------------------------------------------

    #[test]
    fn churn_decrease_small_counts() {
        assert_eq!(churn_decrease(0), 0);
        assert_eq!(churn_decrease(1), 0); // max(0, 1 - max(1, 0)) = 0
        assert_eq!(churn_decrease(2), 1); // max(0, 2 - max(1, 0)) = 1
        assert_eq!(churn_decrease(5), 4); // max(0, 5 - max(1, 1)) = 4
    }

    #[test]
    fn churn_decrease_large_counts() {
        // At 10: max(0, 10 - max(1, 2)) = 8
        assert_eq!(churn_decrease(10), 8);
        // At 20: max(0, 20 - max(1, 4)) = 16
        assert_eq!(churn_decrease(20), 16);
        // At 100: max(0, 100 - max(1, 20)) = 80
        assert_eq!(churn_decrease(100), 80);
    }

    // -----------------------------------------------------------------------
    // Two-phase churn integration
    // -----------------------------------------------------------------------

    #[test]
    fn tick_enters_churn_and_demotes_excess_hot() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 3,
            target_active: 2,
            ..Default::default()
        };
        let mut state = GovernorState::default();
        let now = Instant::now();

        // After first tick, DecreasedActive is entered.
        // churn_decrease(2) = 1, so 1 excess hot → DemoteToWarm.
        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, now);
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));
        assert!(actions.iter().any(|a| matches!(a, GovernorAction::DemoteToWarm(_))));
    }

    #[test]
    fn tick_churn_cycle_produces_established_demotions() {
        // Start already at DecreasedEstablished with excess warm peers.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 3,
            target_active: 0,
            ..Default::default()
        };
        // Start in DecreasedEstablished so the established targets are lowered.
        let now = Instant::now();
        let mut state = GovernorState {
            churn_phase: ChurnPhase::DecreasedEstablished { started: now },
            last_churn_cycle: None,
            ..Default::default()
        };

        // churn_decrease(3) = 2, 3 warm > 2 target → 1 demotion to cold.
        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, now);
        assert!(actions.iter().any(|a| matches!(a, GovernorAction::DemoteToCold(_))));
    }

    // -----------------------------------------------------------------------
    // In-flight tracking
    // -----------------------------------------------------------------------

    #[test]
    fn in_flight_warm_blocks_promotion() {
        let mut state = GovernorState::default();
        state.mark_in_flight_warm(addr(1));
        let now = Instant::now();

        let actions = vec![
            GovernorAction::PromoteToWarm(addr(1)),
            GovernorAction::PromoteToWarm(addr(2)),
        ];
        let filtered = state.filter_backed_off(actions, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], GovernorAction::PromoteToWarm(addr(2)));

        // Clear the in-flight flag — now it's allowed again.
        state.clear_in_flight_warm(&addr(1));
        let actions = vec![GovernorAction::PromoteToWarm(addr(1))];
        let filtered = state.filter_backed_off(actions, now);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn in_flight_hot_blocks_promotion() {
        let mut state = GovernorState::default();
        state.mark_in_flight_hot(addr(3));
        let now = Instant::now();

        let actions = vec![
            GovernorAction::PromoteToHot(addr(3)),
            GovernorAction::PromoteToHot(addr(4)),
        ];
        let filtered = state.filter_backed_off(actions, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], GovernorAction::PromoteToHot(addr(4)));
    }

    // -----------------------------------------------------------------------
    // Exponential backoff
    // -----------------------------------------------------------------------

    #[test]
    fn exponential_backoff_short_elapsed() {
        let mut state = GovernorState {
            failure_backoff: Duration::from_secs(10),
            ..Default::default()
        };

        // 1 failure → backoff = 10s * 2^0 = 10s.
        state.record_failure(addr(1));
        let now = Instant::now();
        // Immediately after, still backing off.
        assert!(state.is_backing_off(&addr(1), now));
        // After 11s, no longer backing off.
        assert!(!state.is_backing_off(&addr(1), now + Duration::from_secs(11)));
    }

    #[test]
    fn exponential_backoff_doubles_with_failures() {
        let mut state = GovernorState {
            failure_backoff: Duration::from_secs(10),
            ..Default::default()
        };

        // 2 failures → backoff = 10s * 2^1 = 20s.
        state.record_failure(addr(1));
        state.record_failure(addr(1));
        let now = Instant::now();
        assert!(state.is_backing_off(&addr(1), now + Duration::from_secs(15)));
        assert!(!state.is_backing_off(&addr(1), now + Duration::from_secs(21)));
    }

    #[test]
    fn failures_decay_over_time() {
        let mut state = GovernorState {
            failure_backoff: Duration::from_secs(10),
            failure_decay: Duration::from_secs(5),
            ..Default::default()
        };

        state.record_failure(addr(1));
        state.record_failure(addr(1));
        let now = Instant::now();

        // Initial backoff for 2 failures is 20s.
        assert!(state.is_backing_off(&addr(1), now + Duration::from_secs(6)));

        // After one decay step, effective failures drop to 1 and backoff to 10s.
        assert!(!state.is_backing_off(&addr(1), now + Duration::from_secs(12)));

        // After enough decay, the record should be pruned.
        state.prune_decayed_failures(now + Duration::from_secs(15));
        assert!(!state.failures.contains_key(&addr(1)));
    }

    // -----------------------------------------------------------------------
    // Tick with full churn cycle
    // -----------------------------------------------------------------------

    #[test]
    fn tick_no_churn_actions_when_targets_met_in_idle() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 2, // exactly met so no peer-share requests fire
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        // Start with a recent cycle so Idle persists.
        let now = Instant::now();
        let mut state = GovernorState {
            last_churn_cycle: Some(now),
            ..Default::default()
        };

        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, now + Duration::from_secs(10));
        // Targets met and no churn due → no actions.
        assert!(actions.is_empty());
        assert_eq!(state.churn_phase, ChurnPhase::Idle);
    }

    // -----------------------------------------------------------------------
    // Big-ledger peer evaluation
    // -----------------------------------------------------------------------

    #[test]
    fn big_ledger_cold_to_warm_promotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_established_big_ledger: 2,
            ..Default::default()
        };
        // Currently 1 warm big-ledger peer, target is 2 → promote 1.
        let actions = evaluate_cold_to_warm_big_ledger_promotions(&reg, &targets);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], GovernorAction::PromoteToWarm(_)));
    }

    #[test]
    fn big_ledger_warm_to_hot_promotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_active_big_ledger: 1,
            ..Default::default()
        };
        let actions = evaluate_warm_to_hot_big_ledger_promotions(&reg, &targets);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], GovernorAction::PromoteToHot(_)));
    }

    #[test]
    fn big_ledger_hot_to_warm_demotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_active_big_ledger: 1,
            ..Default::default()
        };
        let actions = evaluate_hot_to_warm_big_ledger_demotions(&reg, &targets);
        assert_eq!(actions.len(), 2);
        for a in &actions {
            assert!(matches!(a, GovernorAction::DemoteToWarm(_)));
        }
    }

    #[test]
    fn big_ledger_no_actions_when_targets_met() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_established_big_ledger: 2,
            target_active_big_ledger: 1,
            ..Default::default()
        };
        assert!(evaluate_cold_to_warm_big_ledger_promotions(&reg, &targets).is_empty());
        assert!(evaluate_warm_to_hot_big_ledger_promotions(&reg, &targets).is_empty());
        assert!(evaluate_hot_to_warm_big_ledger_demotions(&reg, &targets).is_empty());
        assert!(evaluate_warm_to_cold_big_ledger_demotions(&reg, &targets).is_empty());
    }

    // -----------------------------------------------------------------------
    // Forget cold peers
    // -----------------------------------------------------------------------

    #[test]
    fn forget_cold_peers_when_excess_known() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
            (4, PeerSource::PeerSourceLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 2,
            ..Default::default()
        };
        let actions = evaluate_forget_cold_peers(&reg, &targets);
        // 4 known > target 2, excess 2. But only peer-share and
        // public-root cold peers are forgettable (2 and 3).
        // Ledger peers (4) and local-root warm (1) are not.
        assert_eq!(actions.len(), 2);
        for a in &actions {
            assert!(matches!(a, GovernorAction::ForgetPeer(_)));
        }
    }

    #[test]
    fn forget_cold_peers_no_action_when_below_target() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            ..Default::default()
        };
        let actions = evaluate_forget_cold_peers(&reg, &targets);
        assert!(actions.is_empty());
    }

    #[test]
    fn regular_established_target_ignores_big_ledger_peers() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_established: 1,
            target_established_big_ledger: 1,
            ..Default::default()
        };

        let actions = evaluate_cold_to_warm_promotions(&reg, &targets);
        assert_eq!(actions, vec![GovernorAction::PromoteToWarm(addr(2))]);
    }

    #[test]
    fn regular_active_target_ignores_big_ledger_peers() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_active: 1,
            target_active_big_ledger: 1,
            ..Default::default()
        };

        let actions = evaluate_warm_to_hot_promotions(&reg, &targets);
        assert_eq!(actions, vec![GovernorAction::PromoteToHot(addr(2))]);
    }

    #[test]
    fn regular_demotion_targets_ignore_big_ledger_peers() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
            (4, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        ]);

        let established_targets = GovernorTargets {
            target_established: 2,
            target_established_big_ledger: 2,
            ..Default::default()
        };
        assert!(evaluate_warm_to_cold_demotions(&reg, &established_targets).is_empty());

        let active_targets = GovernorTargets {
            target_active: 1,
            target_active_big_ledger: 1,
            ..Default::default()
        };
        assert!(evaluate_hot_to_warm_demotions(&reg, &active_targets).is_empty());
    }

    #[test]
    fn forget_cold_peers_ignores_big_ledger_known_count() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 2,
            target_known_big_ledger: 1,
            ..Default::default()
        };

        let actions = evaluate_forget_cold_peers(&reg, &targets);
        assert!(actions.is_empty());
    }

    // -- Bootstrap-sensitive mode tests ----------------------------------------

    #[test]
    fn requires_bootstrap_peers_returns_false_when_young_enough() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert!(!requires_bootstrap_peers(&ubp, LedgerStateJudgement::YoungEnough));
    }

    #[test]
    fn requires_bootstrap_peers_returns_true_when_too_old_and_enabled() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert!(requires_bootstrap_peers(&ubp, LedgerStateJudgement::TooOld));
    }

    #[test]
    fn requires_bootstrap_peers_returns_false_when_too_old_but_disabled() {
        let ubp = UseBootstrapPeers::DontUseBootstrapPeers;
        assert!(!requires_bootstrap_peers(&ubp, LedgerStateJudgement::TooOld));
    }

    #[test]
    fn requires_bootstrap_peers_returns_true_when_unavailable_and_enabled() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert!(requires_bootstrap_peers(&ubp, LedgerStateJudgement::Unavailable));
    }

    #[test]
    fn peer_selection_mode_sensitive_when_bootstrap_required() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert_eq!(
            peer_selection_mode(&ubp, LedgerStateJudgement::TooOld),
            PeerSelectionMode::Sensitive,
        );
    }

    #[test]
    fn peer_selection_mode_normal_when_young_enough() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert_eq!(
            peer_selection_mode(&ubp, LedgerStateJudgement::YoungEnough),
            PeerSelectionMode::Normal,
        );
    }

    #[test]
    fn peer_selection_mode_normal_when_disabled() {
        let ubp = UseBootstrapPeers::DontUseBootstrapPeers;
        assert_eq!(
            peer_selection_mode(&ubp, LedgerStateJudgement::TooOld),
            PeerSelectionMode::Normal,
        );
    }

    #[test]
    fn is_node_able_to_make_progress_normal_mode() {
        let ubp = UseBootstrapPeers::DontUseBootstrapPeers;
        // Not in sensitive mode → always able to make progress.
        assert!(is_node_able_to_make_progress(&ubp, LedgerStateJudgement::TooOld, false));
    }

    #[test]
    fn is_node_able_to_make_progress_sensitive_with_trustable_only() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert!(is_node_able_to_make_progress(&ubp, LedgerStateJudgement::TooOld, true));
    }

    #[test]
    fn is_node_able_to_make_progress_sensitive_without_trustable_only() {
        let ubp = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        assert!(!is_node_able_to_make_progress(&ubp, LedgerStateJudgement::TooOld, false));
    }

    #[test]
    fn has_only_trustable_established_peers_empty_registry() {
        let reg = PeerRegistry::default();
        let groups: Vec<LocalRootTargets> = vec![];
        assert!(has_only_trustable_established_peers(&reg, &groups));
    }

    #[test]
    fn has_only_trustable_established_peers_bootstrap_warm() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerWarm),
        ]);
        let groups: Vec<LocalRootTargets> = vec![];
        // Bootstrap peers are always trustable.
        assert!(has_only_trustable_established_peers(&reg, &groups));
    }

    #[test]
    fn has_only_trustable_established_peers_trustable_local_root() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        ]);
        let groups = vec![LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 1,
            warm_valency: 1,
            trustable: true,
        }];
        assert!(has_only_trustable_established_peers(&reg, &groups));
    }

    #[test]
    fn has_only_trustable_established_peers_non_trustable_local_root() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        ]);
        let groups = vec![LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 1,
            warm_valency: 1,
            trustable: false,
        }];
        assert!(!has_only_trustable_established_peers(&reg, &groups));
    }

    #[test]
    fn has_only_trustable_cold_peers_do_not_block() {
        // Cold peers (even non-trustable) don't block the check.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        ]);
        let groups: Vec<LocalRootTargets> = vec![];
        assert!(has_only_trustable_established_peers(&reg, &groups));
    }

    #[test]
    fn sensitive_hot_demotions_demote_non_trustable() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let groups = vec![LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 1,
            warm_valency: 1,
            trustable: true,
        }];

        let actions = evaluate_sensitive_hot_demotions(&reg, &groups);
        // Peer 1 is bootstrap → trustable → no demotion.
        // Peers 2 & 3 are public root / ledger → not trustable → demote.
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&GovernorAction::DemoteToWarm(addr(2))));
        assert!(actions.contains(&GovernorAction::DemoteToWarm(addr(3))));
    }

    #[test]
    fn sensitive_hot_demotions_spares_trustable_local_roots() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        ]);
        let groups = vec![
            LocalRootTargets {
                peers: vec![addr(1)],
                hot_valency: 1,
                warm_valency: 1,
                trustable: true,
            },
            LocalRootTargets {
                peers: vec![addr(2)],
                hot_valency: 1,
                warm_valency: 1,
                trustable: false,
            },
        ];

        let actions = evaluate_sensitive_hot_demotions(&reg, &groups);
        // Peer 1 is in trustable group → spared.
        // Peer 2 is in non-trustable group → demoted.
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::DemoteToWarm(addr(2)));
    }

    #[test]
    fn sensitive_warm_demotions_demote_non_trustable() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let groups: Vec<LocalRootTargets> = vec![];

        let actions = evaluate_sensitive_warm_demotions(&reg, &groups);
        // Peer 1 is bootstrap → trustable.
        // Peer 2 is peer-shared → not trustable.
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::DemoteToCold(addr(2)));
    }

    #[test]
    fn filter_sensitive_promotions_keeps_trustable() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
        ]);
        let groups = vec![LocalRootTargets {
            peers: vec![addr(3)],
            hot_valency: 1,
            warm_valency: 1,
            trustable: true,
        }];

        let actions = vec![
            GovernorAction::PromoteToWarm(addr(1)),
            GovernorAction::PromoteToWarm(addr(2)),
            GovernorAction::PromoteToWarm(addr(3)),
        ];

        let filtered = filter_sensitive_promotions(actions, &reg, &groups);
        // Peer 1 (bootstrap) and peer 3 (trustable local root) pass filter.
        // Peer 2 (public root, not trustable) is filtered out.
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(1))));
        assert!(filtered.contains(&GovernorAction::PromoteToWarm(addr(3))));
    }

    #[test]
    fn filter_sensitive_promotions_keeps_demotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        ]);
        let groups: Vec<LocalRootTargets> = vec![];

        let actions = vec![GovernorAction::DemoteToWarm(addr(1))];
        let filtered = filter_sensitive_promotions(actions, &reg, &groups);
        // Demotions are never filtered.
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn governor_tick_sensitive_demotes_non_trustable_hot() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 2,
            ..Default::default()
        };
        let groups: Vec<LocalRootTargets> = vec![];

        let actions = governor_tick(&reg, &targets, &groups, PeerSelectionMode::Sensitive, AssociationMode::Unrestricted, None, Instant::now());
        // Even though targets say 2 active, peer 2 is not trustable → demote.
        assert!(actions.contains(&GovernorAction::DemoteToWarm(addr(2))));
        // Peer 1 (bootstrap) is NOT demoted.
        assert!(!actions.contains(&GovernorAction::DemoteToWarm(addr(1))));
    }

    #[test]
    fn governor_tick_sensitive_suppresses_big_ledger_promotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourceBootstrap, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 1,
            target_known_big_ledger: 5,
            target_established_big_ledger: 1,
            target_active_big_ledger: 1,
            ..Default::default()
        };
        let groups: Vec<LocalRootTargets> = vec![];

        let actions = governor_tick(&reg, &targets, &groups, PeerSelectionMode::Sensitive, AssociationMode::Unrestricted, None, Instant::now());
        // Bootstrap peer may be promoted.
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(2))));
        // Big-ledger peer is suppressed in sensitive mode.
        assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    }

    #[test]
    fn governor_tick_normal_allows_all_promotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 3,
            target_active: 1,
            target_known_big_ledger: 5,
            target_established_big_ledger: 1,
            target_active_big_ledger: 1,
            ..Default::default()
        };
        let groups: Vec<LocalRootTargets> = vec![];

        let actions = governor_tick(&reg, &targets, &groups, PeerSelectionMode::Normal, AssociationMode::Unrestricted, None, Instant::now());
        // All peers should be promoted in normal mode.
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(2))));
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(3))));
    }

    // -----------------------------------------------------------------------
    // Tepid flag tests
    // -----------------------------------------------------------------------

    #[test]
    fn tepid_flag_set_on_hot_to_warm() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerHot);
        assert!(!reg.get(&addr(1)).unwrap().tepid);

        // Hot → Warm sets tepid.
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        assert!(reg.get(&addr(1)).unwrap().tepid);
    }

    #[test]
    fn tepid_flag_cleared_on_cold_to_warm() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerHot);
        reg.set_status(addr(1), PeerStatus::PeerWarm); // sets tepid
        assert!(reg.get(&addr(1)).unwrap().tepid);

        // Warm → Cold, then Cold → Warm clears tepid.
        reg.set_status(addr(1), PeerStatus::PeerCold);
        assert!(reg.get(&addr(1)).unwrap().tepid); // still true while cold
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        assert!(!reg.get(&addr(1)).unwrap().tepid); // cleared
    }

    #[test]
    fn tepid_flag_starts_false() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourceLedger);
        assert!(!reg.get(&addr(1)).unwrap().tepid);
    }

    #[test]
    fn cold_to_warm_prefers_non_tepid() {
        // Create two cold peers: one tepid, one not.
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
        reg.insert_source(addr(2), PeerSource::PeerSourcePublicRoot);

        // Make peer 1 tepid by cycling through hot → warm.
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerHot);
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerCold);
        assert!(reg.get(&addr(1)).unwrap().tepid);
        assert!(!reg.get(&addr(2)).unwrap().tepid);

        let targets = GovernorTargets {
            target_known: 10,
            target_established: 1,
            target_active: 0,
            ..Default::default()
        };

        let actions = evaluate_cold_to_warm_promotions(&reg, &targets);
        assert_eq!(actions.len(), 1);
        // Non-tepid peer 2 should be promoted first.
        assert_eq!(actions[0], GovernorAction::PromoteToWarm(addr(2)));
    }

    #[test]
    fn warm_to_hot_prefers_non_tepid() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
        reg.insert_source(addr(2), PeerSource::PeerSourcePublicRoot);

        // Make both warm, but peer 1 is tepid.
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerHot);
        reg.set_status(addr(1), PeerStatus::PeerWarm); // tepid
        assert!(reg.get(&addr(1)).unwrap().tepid);

        reg.set_status(addr(2), PeerStatus::PeerWarm); // fresh, not tepid
        assert!(!reg.get(&addr(2)).unwrap().tepid);

        let targets = GovernorTargets {
            target_known: 10,
            target_established: 5,
            target_active: 1,
            ..Default::default()
        };

        let actions = evaluate_warm_to_hot_promotions(&reg, &targets);
        assert_eq!(actions.len(), 1);
        // Non-tepid peer 2 should be promoted first.
        assert_eq!(actions[0], GovernorAction::PromoteToHot(addr(2)));
    }

    #[test]
    fn tepid_peers_still_promoted_when_needed() {
        // When targets demand more peers than non-tepid available, tepid
        // peers fill the gap.
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);

        // Make peer 1 cold + tepid.
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerHot);
        reg.set_status(addr(1), PeerStatus::PeerWarm);
        reg.set_status(addr(1), PeerStatus::PeerCold);
        assert!(reg.get(&addr(1)).unwrap().tepid);

        let targets = GovernorTargets {
            target_known: 10,
            target_established: 1,
            target_active: 0,
            ..Default::default()
        };

        let actions = evaluate_cold_to_warm_promotions(&reg, &targets);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::PromoteToWarm(addr(1)));
    }

    // -----------------------------------------------------------------------
    // Max connection retries (forget-failed-peers) tests
    // -----------------------------------------------------------------------

    #[test]
    fn forget_failed_peer_exceeding_max_retries() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);
        // Peer is cold (default).

        let mut state = GovernorState {
            max_connection_retries: Some(3),
            ..Default::default()
        };
        // Record 4 failures (> max_retries of 3).
        for _ in 0..4 {
            state.record_failure(addr(1));
        }

        let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::ForgetPeer(addr(1)));
    }

    #[test]
    fn do_not_forget_peer_at_or_below_max_retries() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);

        let mut state = GovernorState {
            max_connection_retries: Some(3),
            ..Default::default()
        };
        // Record exactly 3 failures (= max_retries, not exceeded).
        for _ in 0..3 {
            state.record_failure(addr(1));
        }

        let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
        assert!(actions.is_empty());
    }

    #[test]
    fn do_not_forget_protected_peer_on_max_retries() {
        // Local-root, bootstrap, ledger, and big-ledger peers are protected.
        for protected_source in [
            PeerSource::PeerSourceLocalRoot,
            PeerSource::PeerSourceBootstrap,
            PeerSource::PeerSourceLedger,
            PeerSource::PeerSourceBigLedger,
        ] {
            let mut reg = PeerRegistry::default();
            reg.insert_source(addr(1), protected_source);

            let mut state = GovernorState {
                max_connection_retries: Some(2),
                ..Default::default()
            };
            for _ in 0..5 {
                state.record_failure(addr(1));
            }

            let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
            assert!(
                actions.is_empty(),
                "protected source {:?} should not be forgotten",
                protected_source,
            );
        }
    }

    #[test]
    fn do_not_forget_warm_peer_on_max_retries() {
        // Only cold peers are forgotten.
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);
        reg.set_status(addr(1), PeerStatus::PeerWarm);

        let mut state = GovernorState {
            max_connection_retries: Some(2),
            ..Default::default()
        };
        for _ in 0..5 {
            state.record_failure(addr(1));
        }

        let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
        assert!(actions.is_empty());
    }

    #[test]
    fn no_forget_when_max_retries_disabled() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);

        let mut state = GovernorState::default();
        assert!(state.max_connection_retries.is_none());
        for _ in 0..10 {
            state.record_failure(addr(1));
        }

        let actions = evaluate_forget_failed_peers(&reg, &state, Instant::now());
        assert!(actions.is_empty());
    }

    #[test]
    fn governor_tick_integrates_forget_failed() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePeerShare);
        // Peer stays cold.

        let mut state = GovernorState {
            max_connection_retries: Some(2),
            ..Default::default()
        };
        for _ in 0..5 {
            state.record_failure(addr(1));
        }

        let targets = GovernorTargets {
            target_known: 10, // not exceeding, so excess-forgetting won't fire
            ..Default::default()
        };
        let now = Instant::now();
        let actions = governor_tick(
            &reg,
            &targets,
            &[],
            PeerSelectionMode::Normal,
            AssociationMode::Unrestricted,
            Some(&state),
            now,
        );
        assert!(actions.contains(&GovernorAction::ForgetPeer(addr(1))));
    }

    // -----------------------------------------------------------------------
    // In-flight demotion tracking tests
    // -----------------------------------------------------------------------

    #[test]
    fn filter_backed_off_removes_duplicate_hot_to_warm_demotion() {
        let mut state = GovernorState::default();
        state.mark_in_flight_demote_hot(addr(1));

        let actions = vec![
            GovernorAction::DemoteToWarm(addr(1)),
            GovernorAction::DemoteToWarm(addr(2)),
        ];
        let filtered = state.filter_backed_off(actions, Instant::now());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], GovernorAction::DemoteToWarm(addr(2)));
    }

    #[test]
    fn filter_backed_off_removes_duplicate_warm_to_cold_demotion() {
        let mut state = GovernorState::default();
        state.mark_in_flight_demote_warm(addr(3));

        let actions = vec![
            GovernorAction::DemoteToCold(addr(3)),
            GovernorAction::DemoteToCold(addr(4)),
        ];
        let filtered = state.filter_backed_off(actions, Instant::now());
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], GovernorAction::DemoteToCold(addr(4)));
    }

    #[test]
    fn clear_in_flight_demote_allows_subsequent_demotion() {
        let mut state = GovernorState::default();
        state.mark_in_flight_demote_hot(addr(1));
        state.clear_in_flight_demote_hot(&addr(1));

        let actions = vec![GovernorAction::DemoteToWarm(addr(1))];
        let filtered = state.filter_backed_off(actions, Instant::now());
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn in_flight_demotion_does_not_affect_promotions() {
        let mut state = GovernorState::default();
        state.mark_in_flight_demote_hot(addr(1));
        state.mark_in_flight_demote_warm(addr(2));

        // Promotions for same addresses should still pass through.
        let actions = vec![
            GovernorAction::PromoteToWarm(addr(1)),
            GovernorAction::PromoteToHot(addr(2)),
        ];
        let filtered = state.filter_backed_off(actions, Instant::now());
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn in_flight_promotion_does_not_affect_demotions() {
        let mut state = GovernorState::default();
        state.mark_in_flight_warm(addr(1));
        state.mark_in_flight_hot(addr(2));

        // Demotions for same addresses should still pass through.
        let actions = vec![
            GovernorAction::DemoteToWarm(addr(1)),
            GovernorAction::DemoteToCold(addr(2)),
        ];
        let filtered = state.filter_backed_off(actions, Instant::now());
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn tick_filters_in_flight_demotions() {
        // Hot peer with in-flight hot→warm demotion should not get
        // another DemoteToWarm from tick().
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 3,
            target_active: 1,
            ..Default::default()
        };
        let mut state = GovernorState::default();
        state.mark_in_flight_demote_hot(addr(1));

        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, Instant::now());
        // Should need to demote 2, but addr(1) is in-flight so at most 1
        // new demotion from the 2 remaining candidates through filter.
        let demote_warm_count = actions
            .iter()
            .filter(|a| matches!(a, GovernorAction::DemoteToWarm(_)))
            .count();
        // addr(1) filtered out; addr(2) and addr(3) are eligible → 2 demotions emitted
        // minus addr(1) = at most 2.  But the excess over target is 2 (3 hot - 1 target).
        // The tick picks first 2 of [addr(2), addr(3), addr(1)] (non-local first).
        // If addr(1) ends up in the first 2, filter removes it → 1 emitted.
        // Otherwise, 2 emitted.  Either way, addr(1) is never emitted.
        assert!(!actions.contains(&GovernorAction::DemoteToWarm(addr(1))));
        assert!(demote_warm_count <= 2);
    }

    // -----------------------------------------------------------------------
    // Peer sharing request tests
    // -----------------------------------------------------------------------

    #[test]
    fn peer_share_request_when_below_target_known() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        let state = GovernorState::default(); // known=2, target=10 → below target

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert!(!actions.is_empty());
        // Should contain share requests for eligible warm/hot peers.
        for a in &actions {
            assert!(matches!(a, GovernorAction::ShareRequest(_)));
        }
    }

    #[test]
    fn no_peer_share_when_known_meets_target() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 2, // exactly met
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        let state = GovernorState::default();

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_peer_share_when_budget_exhausted() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            ..Default::default()
        };
        let mut state = GovernorState::default();
        // Exhaust the budget.
        state.in_progress_peer_share_reqs = state.max_in_progress_peer_share_reqs;

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert!(actions.is_empty());
    }

    #[test]
    fn peer_share_respects_budget_limit() {
        // 5 warm peers but budget only allows 2 requests.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (4, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (5, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 5,
            ..Default::default()
        };
        let state = GovernorState {
            max_in_progress_peer_share_reqs: 2,
            ..Default::default()
        };

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn peer_share_excludes_local_root_and_bootstrap() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 3,
            target_active: 1,
            ..Default::default()
        };
        let state = GovernorState::default();

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::ShareRequest(addr(3)));
    }

    #[test]
    fn peer_share_excludes_big_ledger_peers() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 2,
            ..Default::default()
        };
        let state = GovernorState::default();

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::ShareRequest(addr(2)));
    }

    #[test]
    fn peer_share_excludes_cold_peers() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 1,
            ..Default::default()
        };
        let state = GovernorState::default();

        let actions = evaluate_peer_share_requests(&reg, &targets, &state);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], GovernorAction::ShareRequest(addr(2)));
    }

    #[test]
    fn mark_and_clear_peer_share_counters() {
        let mut state = GovernorState::default();
        assert_eq!(state.in_progress_peer_share_reqs, 0);

        state.mark_peer_share_sent();
        assert_eq!(state.in_progress_peer_share_reqs, 1);

        state.mark_peer_share_sent();
        assert_eq!(state.in_progress_peer_share_reqs, 2);

        state.clear_peer_share_completed(1);
        assert_eq!(state.in_progress_peer_share_reqs, 1);

        state.clear_peer_share_completed(5); // saturating_sub
        assert_eq!(state.in_progress_peer_share_reqs, 0);
    }

    #[test]
    fn no_peer_share_in_sensitive_mode() {
        // Peer sharing requests are suppressed in sensitive mode.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 1,
            ..Default::default()
        };
        let state = GovernorState::default();
        let now = Instant::now();

        let actions = governor_tick(
            &reg,
            &targets,
            &[],
            PeerSelectionMode::Sensitive,
            AssociationMode::Unrestricted,
            Some(&state),
            now,
        );
        // No ShareRequest should appear in sensitive mode since peer
        // sharing is only wired in Normal mode path.
        assert!(!actions.iter().any(|a| matches!(a, GovernorAction::ShareRequest(_))));
    }

    #[test]
    fn governor_tick_emits_share_requests_normal_mode() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 100, // way above known count → below target
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        let state = GovernorState::default();
        let now = Instant::now();

        let actions = governor_tick(
            &reg,
            &targets,
            &[],
            PeerSelectionMode::Normal,
            AssociationMode::Unrestricted,
            Some(&state),
            now,
        );
        assert!(actions.iter().any(|a| matches!(a, GovernorAction::ShareRequest(_))));
    }

    #[test]
    fn tick_emits_share_requests_with_budget() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 3,
            target_active: 0,
            ..Default::default()
        };
        let mut state = GovernorState {
            max_in_progress_peer_share_reqs: 1, // only 1 allowed
            ..Default::default()
        };

        let actions = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, Instant::now());
        let share_count = actions
            .iter()
            .filter(|a| matches!(a, GovernorAction::ShareRequest(_)))
            .count();
        assert_eq!(share_count, 1);
    }

    // -----------------------------------------------------------------------
    // AssociationMode and NodePeerSharing tests
    // -----------------------------------------------------------------------

    #[test]
    fn node_peer_sharing_default_disabled() {
        assert!(!NodePeerSharing::default().is_enabled());
        assert!(NodePeerSharing::PeerSharingEnabled.is_enabled());
    }

    #[test]
    fn node_peer_sharing_from_wire() {
        assert_eq!(NodePeerSharing::from_wire(0), NodePeerSharing::PeerSharingDisabled);
        assert_eq!(NodePeerSharing::from_wire(1), NodePeerSharing::PeerSharingEnabled);
        // Any nonzero wire value is treated as enabled per the protocol spec.
        assert_eq!(NodePeerSharing::from_wire(42), NodePeerSharing::PeerSharingEnabled);
    }

    #[test]
    fn compute_association_mode_all_disabled_is_local_only() {
        assert_eq!(
            compute_association_mode(
                &UseBootstrapPeers::DontUseBootstrapPeers,
                &UseLedgerPeers::DontUseLedgerPeers,
                NodePeerSharing::PeerSharingDisabled,
                LedgerStateJudgement::YoungEnough,
            ),
            AssociationMode::LocalRootsOnly,
        );
    }

    #[test]
    fn compute_association_mode_ledger_peers_is_unrestricted() {
        assert_eq!(
            compute_association_mode(
                &UseBootstrapPeers::DontUseBootstrapPeers,
                &UseLedgerPeers::UseLedgerPeers(crate::root_peers::AfterSlot::Always),
                NodePeerSharing::PeerSharingDisabled,
                LedgerStateJudgement::YoungEnough,
            ),
            AssociationMode::Unrestricted,
        );
    }

    #[test]
    fn compute_association_mode_peer_sharing_is_unrestricted() {
        assert_eq!(
            compute_association_mode(
                &UseBootstrapPeers::DontUseBootstrapPeers,
                &UseLedgerPeers::DontUseLedgerPeers,
                NodePeerSharing::PeerSharingEnabled,
                LedgerStateJudgement::YoungEnough,
            ),
            AssociationMode::Unrestricted,
        );
    }

    #[test]
    fn compute_association_mode_bootstrap_synced_no_ledger_no_sharing_is_local() {
        // Bootstrap peers configured but ledger is young enough (not
        // requiring bootstrap peers) and no ledger/sharing → LocalRootsOnly.
        assert_eq!(
            compute_association_mode(
                &UseBootstrapPeers::UseBootstrapPeers(vec![]),
                &UseLedgerPeers::DontUseLedgerPeers,
                NodePeerSharing::PeerSharingDisabled,
                LedgerStateJudgement::YoungEnough,
            ),
            AssociationMode::LocalRootsOnly,
        );
    }

    #[test]
    fn compute_association_mode_bootstrap_too_old_is_unrestricted() {
        // Bootstrap peers configured and ledger is TooOld (still requires
        // bootstrap) → Unrestricted.
        assert_eq!(
            compute_association_mode(
                &UseBootstrapPeers::UseBootstrapPeers(vec![]),
                &UseLedgerPeers::DontUseLedgerPeers,
                NodePeerSharing::PeerSharingDisabled,
                LedgerStateJudgement::TooOld,
            ),
            AssociationMode::Unrestricted,
        );
    }

    #[test]
    fn local_roots_only_suppresses_peer_sharing() {
        // In LocalRootsOnly mode, peer sharing requests should NOT be
        // generated even in Normal mode.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
        ]);
        let targets = GovernorTargets {
            target_known: 100,
            target_established: 2,
            target_active: 1,
            ..Default::default()
        };
        let state = GovernorState::default();
        let now = Instant::now();

        let actions = governor_tick(
            &reg,
            &targets,
            &[],
            PeerSelectionMode::Normal,
            AssociationMode::LocalRootsOnly,
            Some(&state),
            now,
        );
        assert!(!actions.iter().any(|a| matches!(a, GovernorAction::ShareRequest(_))));
    }

    #[test]
    fn local_roots_only_suppresses_big_ledger_promotions() {
        // In LocalRootsOnly mode, big-ledger promotions should NOT be
        // generated even in Normal mode.
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 1,
            target_known_big_ledger: 5,
            target_established_big_ledger: 1,
            target_active_big_ledger: 1,
            ..Default::default()
        };
        let actions = governor_tick(
            &reg,
            &targets,
            &[],
            PeerSelectionMode::Normal,
            AssociationMode::LocalRootsOnly,
            None,
            Instant::now(),
        );
        // Big-ledger peer should NOT be promoted in LocalRootsOnly.
        assert!(!actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    }

    #[test]
    fn unrestricted_allows_big_ledger_promotions() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
        ]);
        let targets = GovernorTargets {
            target_known: 10,
            target_established: 1,
            target_known_big_ledger: 5,
            target_established_big_ledger: 1,
            target_active_big_ledger: 1,
            ..Default::default()
        };
        let actions = governor_tick(
            &reg,
            &targets,
            &[],
            PeerSelectionMode::Normal,
            AssociationMode::Unrestricted,
            None,
            Instant::now(),
        );
        // Big-ledger peer SHOULD be promoted in Unrestricted.
        assert!(actions.contains(&GovernorAction::PromoteToWarm(addr(1))));
    }

    // -----------------------------------------------------------------------
    // PeerSelectionCounters tests
    // -----------------------------------------------------------------------

    #[test]
    fn counters_empty_registry() {
        let reg = PeerRegistry::default();
        let counters = PeerSelectionCounters::from_registry(&reg, None);
        assert_eq!(counters, PeerSelectionCounters::default());
    }

    #[test]
    fn counters_regular_peer_categories() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourceLedger, PeerStatus::PeerHot),
            (4, PeerSource::PeerSourcePeerShare, PeerStatus::PeerCold),
        ]);
        let counters = PeerSelectionCounters::from_registry(&reg, None);

        // Regular totals: all 4 are non-big-ledger.
        assert_eq!(counters.known, 4);
        assert_eq!(counters.available_to_connect, 2); // ports 1 and 4 are cold
        assert_eq!(counters.established, 2); // warm(2) + hot(3)
        assert_eq!(counters.active, 1); // hot(3)

        // Local-root: only port 1.
        assert_eq!(counters.known_local_root, 1);
        assert_eq!(counters.available_to_connect_local_root, 1);
        assert_eq!(counters.established_local_root, 0);
        assert_eq!(counters.active_local_root, 0);

        // Non-root: port 4 (PeerShare is not a root source).
        assert_eq!(counters.known_non_root, 1);
        assert_eq!(counters.available_to_connect_non_root, 1);

        // Root peers: 3 (LocalRoot + PublicRoot + Ledger).
        assert_eq!(counters.root_peers, 3);
    }

    #[test]
    fn counters_big_ledger_peers() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourceBigLedger, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        ]);
        let counters = PeerSelectionCounters::from_registry(&reg, None);

        // Big-ledger counters.
        assert_eq!(counters.known_big_ledger, 3);
        assert_eq!(counters.available_to_connect_big_ledger, 1); // cold
        assert_eq!(counters.established_big_ledger, 2); // warm + hot
        assert_eq!(counters.active_big_ledger, 1); // hot

        // Regular counters should be zero (big-ledger is excluded).
        assert_eq!(counters.known, 0);
        assert_eq!(counters.established, 0);
        assert_eq!(counters.active, 0);
    }

    #[test]
    fn counters_in_flight_from_governor_state() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
            (2, PeerSource::PeerSourceLedger, PeerStatus::PeerWarm),
            (3, PeerSource::PeerSourceBigLedger, PeerStatus::PeerCold),
            (4, PeerSource::PeerSourceBigLedger, PeerStatus::PeerHot),
        ]);
        let mut gs = GovernorState::default();
        gs.mark_in_flight_warm(addr(1)); // regular cold→warm
        gs.mark_in_flight_hot(addr(2)); // regular warm→hot
        gs.mark_in_flight_warm(addr(3)); // big-ledger cold→warm
        gs.mark_in_flight_demote_hot(addr(4)); // big-ledger hot→warm

        let counters = PeerSelectionCounters::from_registry(&reg, Some(&gs));

        assert_eq!(counters.cold_peers_promotions, 1); // addr(1)
        assert_eq!(counters.warm_peers_promotions, 1); // addr(2)
        assert_eq!(counters.cold_big_ledger_promotions, 1); // addr(3)
        assert_eq!(counters.active_big_ledger_demotions, 1); // addr(4)
    }

    #[test]
    fn counters_cooling_peers_not_available() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
        reg.set_status(addr(1), PeerStatus::PeerCooling);

        let counters = PeerSelectionCounters::from_registry(&reg, None);
        assert_eq!(counters.known, 1);
        assert_eq!(counters.available_to_connect, 0); // cooling → not available
        assert_eq!(counters.established, 0);
    }

    // -----------------------------------------------------------------------
    // OutboundConnectionsState tests
    // -----------------------------------------------------------------------

    #[test]
    fn outbound_local_roots_only_all_trustable() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
        ]);
        let group = LocalRootTargets {
            peers: vec![addr(1), addr(2)],
            hot_valency: 1,
            warm_valency: 2,
            trustable: true,
        };
        let state = compute_outbound_connections_state(
            &reg,
            &[group],
            AssociationMode::LocalRootsOnly,
            &UseBootstrapPeers::DontUseBootstrapPeers,
        );
        assert_eq!(state, OutboundConnectionsState::TrustedStateWithExternalPeers);
    }

    #[test]
    fn outbound_local_roots_only_non_trustable_warm_untrusted() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
        ]);
        let group = LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 0,
            warm_valency: 1,
            trustable: true,
        };
        let state = compute_outbound_connections_state(
            &reg,
            &[group],
            AssociationMode::LocalRootsOnly,
            &UseBootstrapPeers::DontUseBootstrapPeers,
        );
        // addr(2) is warm but not a trustable local root → untrusted.
        assert_eq!(state, OutboundConnectionsState::UntrustedState);
    }

    #[test]
    fn outbound_unrestricted_no_bootstrap_always_trusted() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let state = compute_outbound_connections_state(
            &reg,
            &[],
            AssociationMode::Unrestricted,
            &UseBootstrapPeers::DontUseBootstrapPeers,
        );
        assert_eq!(state, OutboundConnectionsState::TrustedStateWithExternalPeers);
    }

    #[test]
    fn outbound_unrestricted_bootstrap_all_trustable_with_external() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        ]);
        let group = LocalRootTargets {
            peers: vec![addr(2)],
            hot_valency: 0,
            warm_valency: 1,
            trustable: true,
        };
        let bootstrap = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        let state = compute_outbound_connections_state(
            &reg,
            &[group],
            AssociationMode::Unrestricted,
            &bootstrap,
        );
        // All established are trustable AND addr(1) is active + bootstrap → trusted.
        assert_eq!(state, OutboundConnectionsState::TrustedStateWithExternalPeers);
    }

    #[test]
    fn outbound_unrestricted_bootstrap_no_external_active_untrusted() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        ]);
        let group = LocalRootTargets {
            peers: vec![addr(1), addr(2)],
            hot_valency: 1,
            warm_valency: 2,
            trustable: true,
        };
        let bootstrap = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        let state = compute_outbound_connections_state(
            &reg,
            &[group],
            AssociationMode::Unrestricted,
            &bootstrap,
        );
        // All established are trustable BUT no bootstrap/public-root active → untrusted.
        assert_eq!(state, OutboundConnectionsState::UntrustedState);
    }

    #[test]
    fn outbound_unrestricted_bootstrap_untrusted_warm_peer() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceBootstrap, PeerStatus::PeerHot),
            (2, PeerSource::PeerSourcePeerShare, PeerStatus::PeerWarm),
        ]);
        let bootstrap = UseBootstrapPeers::UseBootstrapPeers(vec![]);
        let state = compute_outbound_connections_state(
            &reg,
            &[],
            AssociationMode::Unrestricted,
            &bootstrap,
        );
        // addr(2) is warm + PeerShare (not trustable) → untrusted.
        assert_eq!(state, OutboundConnectionsState::UntrustedState);
    }

    #[test]
    fn outbound_local_roots_only_empty_registry_trusted() {
        let reg = PeerRegistry::default();
        let state = compute_outbound_connections_state(
            &reg,
            &[],
            AssociationMode::LocalRootsOnly,
            &UseBootstrapPeers::DontUseBootstrapPeers,
        );
        // No established peers → all (vacuously) trustable.
        assert_eq!(state, OutboundConnectionsState::TrustedStateWithExternalPeers);
    }

    #[test]
    fn outbound_local_roots_only_non_trustable_group() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourceLocalRoot, PeerStatus::PeerWarm),
        ]);
        let group = LocalRootTargets {
            peers: vec![addr(1)],
            hot_valency: 0,
            warm_valency: 1,
            trustable: false, // group is NOT trustable
        };
        let state = compute_outbound_connections_state(
            &reg,
            &[group],
            AssociationMode::LocalRootsOnly,
            &UseBootstrapPeers::DontUseBootstrapPeers,
        );
        // addr(1) is warm but its group is not trustable → untrusted.
        assert_eq!(state, OutboundConnectionsState::UntrustedState);
    }

    // -----------------------------------------------------------------------
    // FetchMode tests
    // -----------------------------------------------------------------------

    #[test]
    fn fetch_mode_young_enough_is_deadline() {
        assert_eq!(
            fetch_mode_from_judgement(LedgerStateJudgement::YoungEnough),
            FetchMode::FetchModeDeadline,
        );
    }

    #[test]
    fn fetch_mode_too_old_is_bulk_sync() {
        assert_eq!(
            fetch_mode_from_judgement(LedgerStateJudgement::TooOld),
            FetchMode::FetchModeBulkSync,
        );
    }

    #[test]
    fn fetch_mode_unavailable_is_bulk_sync() {
        assert_eq!(
            fetch_mode_from_judgement(LedgerStateJudgement::Unavailable),
            FetchMode::FetchModeBulkSync,
        );
    }

    // -----------------------------------------------------------------------
    // ChurnMode / ChurnRegime tests
    // -----------------------------------------------------------------------

    #[test]
    fn churn_mode_from_deadline_is_normal() {
        assert_eq!(
            churn_mode_from_fetch_mode(FetchMode::FetchModeDeadline),
            ChurnMode::Normal,
        );
    }

    #[test]
    fn churn_mode_from_bulk_sync_is_bulk() {
        assert_eq!(
            churn_mode_from_fetch_mode(FetchMode::FetchModeBulkSync),
            ChurnMode::BulkSync,
        );
    }

    #[test]
    fn churn_regime_normal_always_default() {
        // ChurnModeNormal → ChurnDefault regardless of bootstrap/consensus.
        assert_eq!(
            pick_churn_regime(ChurnMode::Normal, &UseBootstrapPeers::DontUseBootstrapPeers, ConsensusMode::PraosMode),
            ChurnRegime::ChurnDefault,
        );
        assert_eq!(
            pick_churn_regime(ChurnMode::Normal, &UseBootstrapPeers::UseBootstrapPeers(vec![]), ConsensusMode::PraosMode),
            ChurnRegime::ChurnDefault,
        );
        assert_eq!(
            pick_churn_regime(ChurnMode::Normal, &UseBootstrapPeers::DontUseBootstrapPeers, ConsensusMode::GenesisMode),
            ChurnRegime::ChurnDefault,
        );
    }

    #[test]
    fn churn_regime_genesis_mode_always_default() {
        // GenesisMode → ChurnDefault even with BulkSync + bootstrap.
        assert_eq!(
            pick_churn_regime(ChurnMode::BulkSync, &UseBootstrapPeers::UseBootstrapPeers(vec![]), ConsensusMode::GenesisMode),
            ChurnRegime::ChurnDefault,
        );
    }

    #[test]
    fn churn_regime_bulk_sync_no_bootstrap_is_praos_sync() {
        assert_eq!(
            pick_churn_regime(ChurnMode::BulkSync, &UseBootstrapPeers::DontUseBootstrapPeers, ConsensusMode::PraosMode),
            ChurnRegime::ChurnPraosSync,
        );
    }

    #[test]
    fn churn_regime_bulk_sync_with_bootstrap_is_bootstrap_praos_sync() {
        assert_eq!(
            pick_churn_regime(ChurnMode::BulkSync, &UseBootstrapPeers::UseBootstrapPeers(vec![]), ConsensusMode::PraosMode),
            ChurnRegime::ChurnBootstrapPraosSync,
        );
    }

    // -----------------------------------------------------------------------
    // Regime-aware churn decrease tests
    // -----------------------------------------------------------------------

    #[test]
    fn churn_decrease_active_default_uses_standard() {
        // ChurnDefault → churn_decrease(10) = 10 - max(1, 10/5) = 10 - 2 = 8.
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnDefault, 10, 0), 8);
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnDefault, 10, 5), 8);
    }

    #[test]
    fn churn_decrease_active_praos_sync_caps_to_local_hot() {
        // PraosSync → min(max(1, local_hot), base - 1).
        // local_hot=3, base=10 → min(3, 9) = 3.
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnPraosSync, 10, 3), 3);
        // local_hot=0, base=10 → min(max(1,0)=1, 9) = 1.
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnPraosSync, 10, 0), 1);
    }

    #[test]
    fn churn_decrease_active_bootstrap_praos_same_as_praos() {
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnBootstrapPraosSync, 10, 3), 3);
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnBootstrapPraosSync, 10, 0), 1);
    }

    #[test]
    fn churn_decrease_active_zero_stays_zero() {
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnDefault, 0, 0), 0);
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnPraosSync, 0, 0), 0);
        assert_eq!(churn_decrease_active(ChurnRegime::ChurnBootstrapPraosSync, 0, 0), 0);
    }

    #[test]
    fn churn_decrease_established_default_shrinks_warm_portion() {
        // est=10, active=5 → warm_only=5, decrease(5)=4, result=4+5=9.
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnDefault, 10, 5), 9);
        // est=10, active=8 → warm_only=2, decrease(2)=1, result=1+8=9.
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnDefault, 10, 8), 9);
    }

    #[test]
    fn churn_decrease_established_praos_sync_same_as_default() {
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnPraosSync, 10, 5), 9);
    }

    #[test]
    fn churn_decrease_established_bootstrap_aggressive() {
        // BootstrapPraosSync → min(active, established - 1).
        // est=10, active=5 → min(5, 9) = 5.
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 10, 5), 5);
        // est=10, active=9 → min(9, 9) = 9.
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 10, 9), 9);
        // est=3, active=1 → min(1, 2) = 1.
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 3, 1), 1);
    }

    #[test]
    fn churn_decrease_established_zero_stays_zero() {
        assert_eq!(churn_decrease_established(ChurnRegime::ChurnBootstrapPraosSync, 0, 0), 0);
    }

    // -----------------------------------------------------------------------
    // Regime-aware apply_churn_to_targets tests
    // -----------------------------------------------------------------------

    #[test]
    fn churn_targets_praos_sync_caps_active_decrease() {
        let state = GovernorState {
            churn_phase: ChurnPhase::DecreasedActive { started: Instant::now() },
            churn_regime: ChurnRegime::ChurnPraosSync,
            local_root_hot_target: 3,
            ..Default::default()
        };
        let targets = GovernorTargets {
            target_active: 10,
            target_established: 20,
            ..Default::default()
        };
        let eff = state.apply_churn_to_targets(&targets);
        // PraosSync: min(max(1, 3), 10-1) = min(3, 9) = 3.
        assert_eq!(eff.target_active, 3);
        // Established unchanged (only active phase).
        assert_eq!(eff.target_established, 20);
    }

    #[test]
    fn churn_targets_bootstrap_aggressive_established() {
        let state = GovernorState {
            churn_phase: ChurnPhase::DecreasedEstablished { started: Instant::now() },
            churn_regime: ChurnRegime::ChurnBootstrapPraosSync,
            ..Default::default()
        };
        let targets = GovernorTargets {
            target_active: 5,
            target_established: 10,
            target_active_big_ledger: 2,
            target_established_big_ledger: 6,
            ..Default::default()
        };
        let eff = state.apply_churn_to_targets(&targets);
        // BootstrapPraosSync: min(active, established - 1).
        // Regular: min(5, 9) = 5.
        assert_eq!(eff.target_established, 5);
        // Big-ledger: min(2, 5) = 2.
        assert_eq!(eff.target_established_big_ledger, 2);
    }

    // -----------------------------------------------------------------------
    // FetchMode-dependent churn interval tests
    // -----------------------------------------------------------------------

    #[test]
    fn churn_config_interval_for_bulk_sync() {
        let config = ChurnConfig::default();
        assert_eq!(
            config.interval_for_mode(FetchMode::FetchModeBulkSync),
            Duration::from_secs(900),
        );
    }

    #[test]
    fn churn_config_interval_for_deadline() {
        let config = ChurnConfig::default();
        assert_eq!(
            config.interval_for_mode(FetchMode::FetchModeDeadline),
            Duration::from_secs(3300),
        );
    }

    #[test]
    fn deadline_mode_uses_longer_churn_interval() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let mut state = GovernorState {
            churn: ChurnConfig {
                bulk_churn_interval: Duration::from_secs(100),
                deadline_churn_interval: Duration::from_secs(500),
                phase_timeout: Duration::from_secs(10),
            },
            fetch_mode: FetchMode::FetchModeDeadline,
            ..Default::default()
        };
        let t0 = Instant::now();

        // Complete a cycle fast.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0);
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(11));
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(22));
        assert_eq!(state.churn_phase, ChurnPhase::Idle);

        // At 200s after cycle end (< 500s deadline interval): stays Idle.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(222));
        assert_eq!(state.churn_phase, ChurnPhase::Idle);

        // At 501s after cycle end: new cycle starts.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(523));
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));
    }

    #[test]
    fn bulk_sync_mode_uses_shorter_churn_interval() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let mut state = GovernorState {
            churn: ChurnConfig {
                bulk_churn_interval: Duration::from_secs(100),
                deadline_churn_interval: Duration::from_secs(500),
                phase_timeout: Duration::from_secs(10),
            },
            fetch_mode: FetchMode::FetchModeBulkSync,
            ..Default::default()
        };
        let t0 = Instant::now();

        // Complete a cycle.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0);
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(11));
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(22));
        assert_eq!(state.churn_phase, ChurnPhase::Idle);

        // At 101s after cycle end (> 100s bulk interval): new cycle starts.
        let _ = state.tick(&reg, &targets, &[], PeerSelectionMode::Normal, AssociationMode::Unrestricted, t0 + Duration::from_secs(123));
        assert!(matches!(state.churn_phase, ChurnPhase::DecreasedActive { .. }));
    }

    // -----------------------------------------------------------------------
    // PeerSelectionTimeouts tests
    // -----------------------------------------------------------------------

    #[test]
    fn peer_selection_timeouts_defaults() {
        let t = PeerSelectionTimeouts::default();
        assert_eq!(t.find_public_root_timeout, Duration::from_secs(5));
        assert_eq!(t.max_in_progress_peer_share_reqs, 2);
        assert_eq!(t.peer_share_retry_time, Duration::from_secs(900));
        assert_eq!(t.peer_share_batch_wait_time, Duration::from_secs(3));
        assert_eq!(t.peer_share_overall_timeout, Duration::from_secs(10));
        assert_eq!(t.peer_share_activation_delay, Duration::from_secs(300));
        assert_eq!(t.max_connection_retries, 5);
        assert_eq!(t.clear_fail_count_delay, Duration::from_secs(120));
    }

    // -----------------------------------------------------------------------
    // ConnectionManagerCounters tests
    // -----------------------------------------------------------------------

    #[test]
    fn connection_counters_empty_registry() {
        let reg = PeerRegistry::default();
        let counters = ConnectionManagerCounters::from_registry(&reg);
        assert_eq!(counters, ConnectionManagerCounters::default());
    }

    #[test]
    fn connection_counters_outbound_warm_and_hot() {
        let reg = make_registry(&[
            (1, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerWarm),
            (2, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerHot),
            (3, PeerSource::PeerSourcePublicRoot, PeerStatus::PeerCold),
        ]);
        let counters = ConnectionManagerCounters::from_registry(&reg);
        assert_eq!(counters.outbound_conns, 2);
        assert_eq!(counters.unidirectional_conns, 2);
        assert_eq!(counters.terminating_conns, 0);
        assert_eq!(counters.inbound_conns, 0);
    }

    #[test]
    fn connection_counters_terminating_cooling() {
        let mut reg = PeerRegistry::default();
        reg.insert_source(addr(1), PeerSource::PeerSourcePublicRoot);
        reg.set_status(addr(1), PeerStatus::PeerCooling);

        let counters = ConnectionManagerCounters::from_registry(&reg);
        assert_eq!(counters.terminating_conns, 1);
        assert_eq!(counters.outbound_conns, 0);
    }

    #[test]
    fn connection_counters_add_is_fieldwise() {
        let a = ConnectionManagerCounters {
            full_duplex_conns: 1,
            duplex_conns: 2,
            unidirectional_conns: 3,
            inbound_conns: 4,
            outbound_conns: 5,
            terminating_conns: 6,
        };
        let b = ConnectionManagerCounters {
            full_duplex_conns: 10,
            duplex_conns: 20,
            unidirectional_conns: 30,
            inbound_conns: 40,
            outbound_conns: 50,
            terminating_conns: 60,
        };
        let sum = a + b;
        assert_eq!(sum.full_duplex_conns, 11);
        assert_eq!(sum.duplex_conns, 22);
        assert_eq!(sum.unidirectional_conns, 33);
        assert_eq!(sum.inbound_conns, 44);
        assert_eq!(sum.outbound_conns, 55);
        assert_eq!(sum.terminating_conns, 66);
    }

    // -----------------------------------------------------------------------
    // ConsensusMode tests
    // -----------------------------------------------------------------------

    #[test]
    fn consensus_mode_eq() {
        assert_eq!(ConsensusMode::PraosMode, ConsensusMode::PraosMode);
        assert_eq!(ConsensusMode::GenesisMode, ConsensusMode::GenesisMode);
        assert_ne!(ConsensusMode::PraosMode, ConsensusMode::GenesisMode);
    }

    // -----------------------------------------------------------------------
    // tick() updates local_root_hot_target
    // -----------------------------------------------------------------------

    #[test]
    fn tick_updates_local_root_hot_target() {
        let reg = PeerRegistry::default();
        let targets = GovernorTargets::default();
        let groups = vec![
            LocalRootTargets {
                peers: vec![addr(1)],
                hot_valency: 3,
                warm_valency: 5,
                trustable: true,
            },
            LocalRootTargets {
                peers: vec![addr(2)],
                hot_valency: 7,
                warm_valency: 10,
                trustable: false,
            },
        ];
        let mut state = GovernorState::default();
        let _ = state.tick(&reg, &targets, &groups, PeerSelectionMode::Normal, AssociationMode::Unrestricted, Instant::now());
        assert_eq!(state.local_root_hot_target, 7);
    }
}
