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
use crate::multiplexer::MiniProtocolNum;
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
    ///
    /// Lenient on the receive side: any `value >= 1` is treated as
    /// enabled, matching upstream's liberal-receiver semantics. The
    /// slice-61 preflight warns operators against transmitting values
    /// outside `{0, 1}`, so the transmit path is strict while the
    /// receive path is tolerant.
    pub fn from_wire(value: u8) -> Self {
        if value >= 1 {
            Self::PeerSharingEnabled
        } else {
            Self::PeerSharingDisabled
        }
    }

    /// Encode this value for handshake transmission.
    ///
    /// Strict inverse of `from_wire`: `PeerSharingDisabled → 0`,
    /// `PeerSharingEnabled → 1`. Upstream's `NodeToNodeVersionData`
    /// encoder always emits these two exact values, so our transmit
    /// side does the same (rather than mirroring `from_wire`'s lenient
    /// accept range).
    ///
    /// Reference: `Ouroboros.Network.PeerSharing` — `peerSharing` codec
    /// in `NodeToNodeVersionData`.
    pub fn to_wire(self) -> u8 {
        match self {
            Self::PeerSharingDisabled => 0,
            Self::PeerSharingEnabled => 1,
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

/// Backoff state for request-style discovery operations.
///
/// This models the signed-counter behavior used by upstream root and
/// big-ledger peer request loops:
///
/// - failure path: `counter = min(counter, 0) - 1`
/// - no-progress path: `counter = max(counter, 0) + 1`
/// - delay: `2 ^ min(abs(counter), 8)` seconds
/// - progress path: `counter = 0`, retry at supplied TTL
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestBackoffState {
    /// Signed backoff counter.
    ///
    /// Negative values indicate request failures; positive values indicate
    /// successful request responses that made no progress.
    pub counter: i32,
    /// Earliest time when another request should be attempted.
    pub next_retry: Option<Instant>,
    /// Whether a request is currently in-flight.
    pub in_progress: bool,
}

impl RequestBackoffState {
    /// Return true if a new request may be started at `now`.
    pub fn can_request(&self, now: Instant) -> bool {
        !self.in_progress && self.next_retry.is_none_or(|t| now >= t)
    }

    /// Mark request as dispatched.
    pub fn mark_request_started(&mut self) {
        self.in_progress = true;
    }

    /// Record request failure and schedule exponential retry.
    pub fn on_failure(&mut self, now: Instant) {
        self.counter = self.counter.min(0) - 1;
        let delay = Self::exponential_delay_secs(self.counter);
        self.next_retry = Some(now + Duration::from_secs(delay));
        self.in_progress = false;
    }

    /// Record request result.
    ///
    /// When `progress` is true, counter resets to zero and retry time is the
    /// provided `ttl` (optionally capped by `ttl_cap`).
    /// When `progress` is false, counter moves in the positive direction and
    /// retry uses exponential backoff.
    pub fn on_result(
        &mut self,
        now: Instant,
        progress: bool,
        ttl: Duration,
        ttl_cap: Option<Duration>,
    ) {
        let delay = if progress {
            self.counter = 0;
            match ttl_cap {
                Some(cap) => ttl.min(cap),
                None => ttl,
            }
        } else {
            self.counter = self.counter.max(0) + 1;
            Duration::from_secs(Self::exponential_delay_secs(self.counter))
        };
        self.next_retry = Some(now + delay);
        self.in_progress = false;
    }

    fn exponential_delay_secs(counter: i32) -> u64 {
        let exp = u32::try_from(counter.abs()).unwrap_or(u32::MAX).min(8);
        2u64.saturating_pow(exp)
    }
}

// ---------------------------------------------------------------------------
// Pick policy — randomized peer selection (upstream `PickPolicy`)
// ---------------------------------------------------------------------------

/// Minimal xorshift64 PRNG for deterministic peer shuffling.
///
/// Upstream uses `StdGen` from `System.Random` in Haskell; we use a
/// lightweight embedded PRNG to avoid adding a `rand` crate dependency.
/// The only requirement is uniform-enough output for peer selection —
/// cryptographic quality is not needed here.
#[derive(Clone, Debug)]
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    /// Create a new PRNG from a seed.  A zero seed is silently replaced
    /// with 1 to avoid the degenerate all-zeros state.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Generate the next pseudo-random u64.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Generate a pseudo-random u32 in `[0, bound)`.
    fn next_bounded(&mut self, bound: u32) -> u32 {
        (self.next_u64() % u64::from(bound)) as u32
    }

    /// Fisher-Yates partial shuffle: randomly permute the first `count`
    /// elements of `slice` and truncate to those elements.
    ///
    /// This is equivalent to upstream's `addRand` + sort-by-weight + take N:
    /// selecting `count` uniformly random elements without replacement.
    pub fn partial_shuffle<T>(&mut self, slice: &mut Vec<T>, count: usize) {
        let n = slice.len();
        let k = count.min(n);
        for i in 0..k {
            let j = i + self.next_bounded((n - i) as u32) as usize;
            slice.swap(i, j);
        }
        slice.truncate(k);
    }
}

/// Randomized peer selection policy.
///
/// Wraps an [`Xorshift64`] PRNG and provides the `pick` operation that
/// the upstream Haskell governor uses in all seven `PickPolicy` callbacks.
///
/// Upstream reference: `Ouroboros.Network.PeerSelection.Simple` —
/// `simplePeerSelectionPolicy` creates seven `PickPolicy` callbacks
/// that all call `addRand :: StdGen → Set peer → Map peer Word32`
/// to assign random weights, sort by weight, and take the first N
/// peers.  `hotDemotionPolicy` additionally adds `upstreamyness +
/// fetchyness` score atop the random weight.
///
/// Construct with [`PickPolicy::new`] for production or
/// [`PickPolicy::deterministic`] for reproducible tests.
#[derive(Clone, Debug)]
pub struct PickPolicy {
    rng: Xorshift64,
}

impl PickPolicy {
    /// Create a new randomized pick policy from a seed.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Xorshift64::new(seed),
        }
    }

    /// Create a deterministic pick policy suitable for tests.
    ///
    /// Identical to `new` but named for intent clarity; the seed `42`
    /// produces a fixed, reproducible selection sequence.
    pub fn deterministic(seed: u64) -> Self {
        Self::new(seed)
    }

    /// Select up to `count` peers randomly from `candidates`.
    ///
    /// Mirrors upstream `pickPeers :: StdGen → Int → Set peeraddr →
    /// (StdGen, Set peeraddr)`.  The candidates Vec is consumed; the
    /// returned Vec contains the randomly selected subset.
    pub fn pick(&mut self, count: usize, mut candidates: Vec<SocketAddr>) -> Vec<SocketAddr> {
        self.rng.partial_shuffle(&mut candidates, count);
        candidates
    }

    /// Return a randomized coin flip.
    ///
    /// Upstream: `random stdGen` boolean used by
    /// `Governor.KnownPeers.belowTarget` to choose inbound-vs-peer-share.
    pub fn coin_flip(&mut self) -> bool {
        (self.rng.next_u64() & 1) == 1
    }

    /// Select up to `count` peers from `candidates`, scoring each peer
    /// with an optional metric weight before random tiebreaking.
    ///
    /// Higher-scored peers are preferred (placed earlier).  Peers with
    /// equal scores are randomized among themselves.  This implements
    /// the upstream `hotDemotionPolicy` where `upstreamyness + fetchyness`
    /// is added to the random weight.
    pub fn pick_scored(
        &mut self,
        count: usize,
        candidates: Vec<SocketAddr>,
        scores: &PeerMetrics,
    ) -> Vec<SocketAddr> {
        // Assign (score, random_weight) per candidate, sort descending by
        // score then by random_weight (higher = preferred).
        let mut weighted: Vec<(SocketAddr, u64, u64)> = candidates
            .into_iter()
            .map(|addr| {
                let score = scores.combined_score(&addr);
                let rand_weight = self.rng.next_u64();
                (addr, score, rand_weight)
            })
            .collect();
        weighted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)));
        weighted.truncate(count);
        weighted.into_iter().map(|(addr, _, _)| addr).collect()
    }
}

/// Peer performance metrics for scoring hot peers during demotion.
///
/// Tracks two independent metrics per peer:
///
/// * **Upstreamyness** — how often a peer was the first to present a
///   new header via ChainSync (header tip timeliness).
/// * **Fetchyness** — how often a peer was the first to deliver a
///   requested block via BlockFetch (data delivery timeliness).
///
/// Both are maintained as bounded-window counters: each slot where the
/// peer "won" increments the score, and scores are periodically decayed
/// by the runtime (not by the governor itself).
///
/// Upstream reference: `Ouroboros.Network.PeerSelection.PeerMetric` —
/// `SlotMetric` with `PeerMetricsConfiguration.maxEntriesToTrack`
/// representing a bounded-size priority queue keyed by `SlotNo`.
///
/// The governor only reads these metrics via [`PeerMetrics::combined_score`]
/// when scoring hot peers for demotion (`hotDemotionPolicy`).  Runtime
/// code updates the metrics when ChainSync/BlockFetch observations arrive.
#[derive(Clone, Debug, Default)]
pub struct PeerMetrics {
    /// Per-peer upstreamyness score (header tip timeliness).
    pub upstreamyness: BTreeMap<SocketAddr, u64>,
    /// Per-peer fetchyness score (block delivery timeliness).
    pub fetchyness: BTreeMap<SocketAddr, u64>,
    /// Per-peer ChainSync header density (Slice GD-Governor).
    ///
    /// `density ∈ [0.0, ~1.0]` is the per-peer chain-quality signal
    /// derived from the consensus-side `DensityWindow` sliding window.
    /// Updated by the runtime each governor tick from the
    /// `node/src/sync.rs::DensityRegistry`.  Consumed by
    /// [`PeerMetrics::combined_score`] and [`PeerMetrics::is_low_density`]
    /// to bias hot demotion away from healthy chain-quality peers and
    /// toward laggards.
    ///
    /// Reference: `Ouroboros.Consensus.Genesis.Governor` density
    /// signal in `IntersectMBO/ouroboros-consensus`.
    pub density: BTreeMap<SocketAddr, f64>,
}

/// Density-quality bonus added to a peer's combined score when its
/// observed chain density meets or exceeds
/// [`LOW_DENSITY_THRESHOLD`].  Small enough not to override the
/// upstreamyness+fetchyness signal; large enough to act as a
/// tie-breaker between two equally-scored peers.
pub const HIGH_DENSITY_BONUS: u64 = 5;

/// Density floor below which a peer is considered low-quality for
/// demotion biasing.  Mirrors upstream
/// `genesisHotDemotionLowDensityThreshold` heuristic and the
/// consensus-side `DEFAULT_LOW_DENSITY_THRESHOLD = 0.6`.
pub const LOW_DENSITY_THRESHOLD: f64 = 0.6;

impl PeerMetrics {
    /// Return the combined score for a peer (`upstreamyness + fetchyness +
    /// density bonus`).
    ///
    /// This matches the upstream `hotDemotionPolicy` which adds
    /// `upstreamyness + fetchyness` to the random weight when scoring
    /// hot peers for demotion.  When per-peer density is available
    /// (Slice GD-Governor), peers with `density >= LOW_DENSITY_THRESHOLD`
    /// receive an additive [`HIGH_DENSITY_BONUS`] so high-quality
    /// chain peers stay hot through close score ties.  Higher score
    /// means the peer is more productive and should be kept hot longer.
    pub fn combined_score(&self, addr: &SocketAddr) -> u64 {
        let base = self.upstreamyness.get(addr).copied().unwrap_or(0)
            + self.fetchyness.get(addr).copied().unwrap_or(0);
        let density_bonus = if self.density_for(addr) >= LOW_DENSITY_THRESHOLD {
            HIGH_DENSITY_BONUS
        } else {
            0
        };
        base + density_bonus
    }

    /// Read the per-peer density score, defaulting to `0.0` if no
    /// observation is recorded.  Equivalent to `density.get(addr)
    /// .copied().unwrap_or(0.0)` and exposed as a method so the
    /// governor's tests and the runtime path use the same accessor.
    pub fn density_for(&self, addr: &SocketAddr) -> f64 {
        self.density.get(addr).copied().unwrap_or(0.0)
    }

    /// Returns `true` when the peer's recorded density is below
    /// [`LOW_DENSITY_THRESHOLD`].  Unknown peers (no density entry yet)
    /// are NOT treated as low-density — that returns `false` — so a
    /// freshly-promoted peer gets a chance to deliver a few headers
    /// before becoming a demotion candidate.
    pub fn is_low_density(&self, addr: &SocketAddr) -> bool {
        match self.density.get(addr) {
            Some(d) => *d < LOW_DENSITY_THRESHOLD,
            None => false,
        }
    }

    /// Record an upstreamyness observation: the peer was first to
    /// present a header at the given slot.
    pub fn record_upstreamyness(&mut self, addr: SocketAddr, _slot: u64) {
        *self.upstreamyness.entry(addr).or_insert(0) += 1;
    }

    /// Record a fetchyness observation: the peer was first to deliver
    /// a block at the given slot.
    pub fn record_fetchyness(&mut self, addr: SocketAddr, _slot: u64) {
        *self.fetchyness.entry(addr).or_insert(0) += 1;
    }

    /// Set the per-peer density score (Slice GD-Governor).  Called by
    /// the runtime each governor tick after reading the consensus-side
    /// `DensityRegistry`.
    pub fn set_density(&mut self, addr: SocketAddr, density: f64) {
        self.density.insert(addr, density);
    }

    /// Remove metrics for a peer that has been forgotten.
    pub fn remove_peer(&mut self, addr: &SocketAddr) {
        self.upstreamyness.remove(addr);
        self.fetchyness.remove(addr);
        self.density.remove(addr);
    }
}

/// Per-mini-protocol scheduling weights for hot peers, plus a derived view
/// onto the currently-hot remote-peer set.
///
/// Mirrors the upstream `Ouroboros.Network.PeerSelection.Governor.HotPeers`
/// module which assigns each mini-protocol a relative weight used by the
/// connection-manager scheduler to decide how to allocate in-flight slots
/// across hot peers.  The weight defaults follow upstream
/// `defaultMiniProtocolParameters`:
///
/// | Protocol            | Weight |
/// |---------------------|-------:|
/// | BlockFetch          | 10     |
/// | ChainSync           | 3      |
/// | TxSubmission        | 2      |
/// | KeepAlive           | 1      |
/// | PeerSharing         | 1      |
///
/// Weights are advisory metadata exposed via `set_hot_protocol_weight`
/// for runtime configuration.  The `hot_peers_remote()` free function
/// derives the current remote-hot set directly from a [`PeerRegistry`]
/// snapshot so callers (e.g. the multi-peer BlockFetch dispatcher in
/// `node/src/sync.rs`) can route work across the active peers without
/// duplicating registry traversal.
///
/// Reference: `Ouroboros.Network.PeerSelection.Governor.HotPeers` in
/// `IntersectMBO/ouroboros-network`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotPeerScheduling {
    weights: BTreeMap<MiniProtocolNum, u8>,
}

impl HotPeerScheduling {
    /// Default upstream weights — see the [type-level table].
    ///
    /// [type-level table]: HotPeerScheduling
    pub fn new() -> Self {
        let mut weights = BTreeMap::new();
        weights.insert(MiniProtocolNum::CHAIN_SYNC, 3);
        weights.insert(MiniProtocolNum::BLOCK_FETCH, 10);
        weights.insert(MiniProtocolNum::TX_SUBMISSION, 2);
        weights.insert(MiniProtocolNum::KEEP_ALIVE, 1);
        weights.insert(MiniProtocolNum::PEER_SHARING, 1);
        Self { weights }
    }

    /// Sets the scheduling weight for a single mini-protocol.  Last write
    /// wins; setting weight 0 effectively disables the protocol from the
    /// scheduler's allocation share.
    pub fn set_hot_protocol_weight(&mut self, proto: MiniProtocolNum, weight: u8) {
        self.weights.insert(proto, weight);
    }

    /// Returns the current weight for `proto`, or `0` if the protocol has
    /// no configured weight.  Mirrors upstream `defaultMiniProtocolParameters`
    /// which treats absent entries as zero-weight.
    pub fn hot_protocol_weight(&self, proto: MiniProtocolNum) -> u8 {
        self.weights.get(&proto).copied().unwrap_or(0)
    }

    /// Read-only view of the full weight table.
    pub fn weights(&self) -> &BTreeMap<MiniProtocolNum, u8> {
        &self.weights
    }
}

impl Default for HotPeerScheduling {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the set of currently-hot remote (non-local-root) peers from a
/// registry snapshot.
///
/// Excludes local-root peers (which are always kept hot under their own
/// valency invariant) and big-ledger peers (whose hot-set is tracked
/// separately).  Used by:
///
/// - [`evaluate_hot_promotions`] to know who is already hot before
///   computing promotions.
/// - The runtime's multi-peer BlockFetch dispatcher in `node/src/sync.rs`
///   to spread fetches across all hot peers.
///
/// Mirrors `hotPeers` derivation in upstream
/// `Ouroboros.Network.PeerSelection.Governor.HotPeers`.
pub fn hot_peers_remote(registry: &PeerRegistry) -> BTreeSet<SocketAddr> {
    let mut out = BTreeSet::new();
    for (addr, entry) in registry.iter() {
        if entry.status == PeerStatus::PeerHot
            && !entry.sources.contains(&PeerSource::PeerSourceLocalRoot)
            && !is_big_ledger(entry)
        {
            out.insert(*addr);
        }
    }
    out
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
    /// Randomized peer selection policy used by all evaluation functions
    /// to select subsets of candidate peers.
    ///
    /// Upstream: the seven `PickPolicy` callbacks in
    /// `simplePeerSelectionPolicy` all use `StdGen` from `System.Random`.
    /// Here, each evaluation function calls `pick.pick()` or
    /// `pick.pick_scored()` instead of deterministic `.take(N)`.
    pub pick: PickPolicy,
    /// Performance metrics used for scoring hot peers during demotion.
    ///
    /// Updated by runtime when ChainSync/BlockFetch observations
    /// arrive; consumed by `evaluate_hot_to_warm_demotions()` via
    /// `pick_scored()`.
    ///
    /// Upstream: `PeerMetric` in
    /// `Ouroboros.Network.PeerSelection.PeerMetric`.
    pub metrics: PeerMetrics,
    /// Upstream-style backoff state for public-root peer discovery requests.
    ///
    /// Mirrors `publicRootBackoffs` + `publicRootRetryTime` +
    /// `inProgressPublicRootsReq` from `Governor.RootPeers`.
    pub public_root_backoff: RequestBackoffState,
    /// Upstream-style backoff state for big-ledger peer discovery requests.
    ///
    /// Mirrors `bigLedgerPeerBackoffs` + `bigLedgerPeerRetryTime` +
    /// `inProgressBigLedgerPeersReq` from `Governor.BigLedgerPeers`.
    pub big_ledger_peer_backoff: RequestBackoffState,
    /// Set of currently available inbound peers eligible for known-peer
    /// discovery.
    ///
    /// Upstream: `inboundPeers` input passed into
    /// `Governor.KnownPeers.belowTarget`.
    pub inbound_peers: BTreeMap<SocketAddr, NodePeerSharing>,
    /// Earliest time when inbound peer discovery is allowed again.
    ///
    /// Upstream: `inboundPeersRetryTime`.
    pub inbound_peers_retry_time: Option<Instant>,
    /// Minimum delay between inbound-discovery picks.
    ///
    /// Upstream: `Policies.inboundPeersRetryDelay` (60s).
    pub inbound_peers_retry_delay: Duration,
    /// Maximum inbound peers adopted in a single discovery round.
    ///
    /// Upstream: `Policies.maxInboundPeers` (10).
    pub max_inbound_peers: usize,
    /// Feature gate for upstream-style public-root and big-ledger request
    /// actions.
    ///
    /// Defaults to `false` to preserve current behavior until runtime wiring
    /// explicitly enables it.
    pub enable_root_big_ledger_requests: bool,
    /// Per-mini-protocol scheduling weights for the hot-peer set.
    ///
    /// Defaults to upstream `defaultMiniProtocolParameters`.  Consumed by
    /// the connection-manager scheduler for in-flight allocation across
    /// hot peers; see [`HotPeerScheduling`] for the weight table and
    /// [`hot_peers_remote`] for the registry-derived set view.
    ///
    /// Upstream: `Ouroboros.Network.PeerSelection.Governor.HotPeers`.
    pub hot_scheduling: HotPeerScheduling,
    /// R222 — Phase D.2 first slice.  Per-peer **lifetime** statistics
    /// keyed by `SocketAddr` that survive across reconnects, providing
    /// stable observability about peer churn.
    ///
    /// Distinct from the existing session-keyed state (`failures`,
    /// `in_flight_*`, peer-registry status) which resets per
    /// reconnect.  When a peer disconnects and reconnects, the
    /// session-keyed counters reset to mirror the live session, but
    /// `lifetime_stats` accumulates monotonically — letting an
    /// operator distinguish "this peer has had 5 reconnects in the
    /// last hour" (churn) from "we just connected to this peer for
    /// the first time" (initial bootstrap).
    ///
    /// Upstream parallel: the long-lived
    /// `KnownPeers.knownPeerInfo` map keyed by `PeerAddr` from
    /// `Ouroboros.Network.PeerSelection.State.KnownPeers`, which
    /// also persists across hot/warm/cold cycle transitions.
    pub lifetime_stats: BTreeMap<SocketAddr, PeerLifetimeStats>,
}

/// R222 — Phase D.2 lifetime peer-statistics record.  Persists
/// across reconnects (unlike session-keyed governor state), keyed
/// by `SocketAddr` (peer identity), and accumulates monotonically.
///
/// Upstream parallel: per-peer `KnownPeerInfo` carried in
/// `Ouroboros.Network.PeerSelection.State.KnownPeers`'s
/// `knownPeerInfo` map.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PeerLifetimeStats {
    /// Total number of successful sessions established with this
    /// peer (handshake completed + at least one mini-protocol
    /// message exchanged).  Monotonic; never resets.
    pub sessions: u32,
    /// Cumulative bytes received from this peer across all
    /// sessions (header + body bytes per BlockFetch + ChainSync +
    /// TxSubmission2 + KeepAlive + PeerSharing).  Updated on a
    /// best-effort basis from mini-protocol metrics.
    pub bytes_in: u64,
    /// Cumulative bytes sent to this peer across all sessions.
    pub bytes_out: u64,
    /// Cumulative number of successful handshake completions
    /// (NtN handshake "AcceptVersion" reached) with this peer.
    /// May exceed `sessions` if a session disconnects after
    /// handshake but before any mini-protocol traffic.
    pub successful_handshakes: u32,
    /// Cumulative number of session failures (handshake failure,
    /// mux abort, mini-protocol error).  Distinct from the
    /// session-keyed `failures` map which decays.
    pub failures_total: u32,
    /// Wall-clock instant of the first observation of this peer
    /// (typically the first handshake attempt).
    pub first_seen: Option<Instant>,
    /// Wall-clock instant of the most recent observation of this
    /// peer (last status update / handshake / message).
    pub last_seen: Option<Instant>,
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
            pick: PickPolicy::new(0xCAFE_BABE_DEAD_BEEF),
            metrics: PeerMetrics::default(),
            public_root_backoff: RequestBackoffState::default(),
            big_ledger_peer_backoff: RequestBackoffState::default(),
            inbound_peers: BTreeMap::new(),
            inbound_peers_retry_time: None,
            inbound_peers_retry_delay: Duration::from_secs(60),
            max_inbound_peers: 10,
            enable_root_big_ledger_requests: false,
            hot_scheduling: HotPeerScheduling::new(),
            lifetime_stats: BTreeMap::new(),
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

    /// R222 — Record the start of a new session with `peer`.  Bumps
    /// the lifetime `sessions` counter and the
    /// `successful_handshakes` counter; updates `first_seen` /
    /// `last_seen`.  Call this from the runtime when the NtN
    /// handshake completes and the per-peer mux is wired through
    /// (i.e., the peer transitions from `PeerCold/PeerCooling` to
    /// `PeerWarm` or higher).
    ///
    /// Idempotent in the sense that repeated calls accumulate
    /// monotonically; callers should ensure the state machine
    /// only fires the event once per actual session.
    pub fn record_lifetime_session_started(&mut self, peer: SocketAddr) {
        let entry = self.lifetime_stats.entry(peer).or_default();
        let now = Instant::now();
        entry.sessions = entry.sessions.saturating_add(1);
        entry.successful_handshakes = entry.successful_handshakes.saturating_add(1);
        if entry.first_seen.is_none() {
            entry.first_seen = Some(now);
        }
        entry.last_seen = Some(now);
    }

    /// R222 — Record a session-level failure for `peer`.  Bumps
    /// the lifetime `failures_total` counter and updates
    /// `last_seen`.  Distinct from
    /// [`Self::record_failure`] which manipulates the
    /// session-keyed `failures` map (used for backoff
    /// computation); the lifetime counter is observability-only.
    pub fn record_lifetime_session_failure(&mut self, peer: SocketAddr) {
        let entry = self.lifetime_stats.entry(peer).or_default();
        let now = Instant::now();
        entry.failures_total = entry.failures_total.saturating_add(1);
        if entry.first_seen.is_none() {
            entry.first_seen = Some(now);
        }
        entry.last_seen = Some(now);
    }

    /// R222 — Accumulate `bytes_in` / `bytes_out` for `peer`.
    /// Best-effort: callers feed in the per-message byte counts
    /// from mini-protocol drivers (BlockFetch served bytes,
    /// ChainSync header/tip bytes, TxSubmission2 reply bytes,
    /// etc.).  No-op if the peer has no lifetime entry yet
    /// (require [`Self::record_lifetime_session_started`] to be
    /// called first).  Updates `last_seen` on every call.
    pub fn record_lifetime_traffic(&mut self, peer: SocketAddr, bytes_in: u64, bytes_out: u64) {
        if let Some(entry) = self.lifetime_stats.get_mut(&peer) {
            entry.bytes_in = entry.bytes_in.saturating_add(bytes_in);
            entry.bytes_out = entry.bytes_out.saturating_add(bytes_out);
            entry.last_seen = Some(Instant::now());
        }
    }

    /// R224 — Phase D.2 third slice: overwrite the lifetime
    /// `bytes_in` total for `peer` from an external cumulative
    /// source (e.g. `BlockFetchInstrumentation::peer_state(peer)
    /// .bytes_delivered`, which already accumulates monotonically
    /// across reconnects).  Use this instead of
    /// [`Self::record_lifetime_traffic`] when the source is
    /// already cumulative; mixing the two would double-count.
    /// Creates the lifetime entry if absent (allowing the runtime
    /// to refresh totals before the first explicit
    /// `record_lifetime_session_started`).  Updates `last_seen`.
    pub fn set_lifetime_bytes_in(&mut self, peer: SocketAddr, total: u64) {
        let entry = self.lifetime_stats.entry(peer).or_default();
        let now = Instant::now();
        entry.bytes_in = total;
        if entry.first_seen.is_none() {
            entry.first_seen = Some(now);
        }
        entry.last_seen = Some(now);
    }

    /// R237 — overwrite the lifetime `bytes_out` total for `peer`
    /// from an external cumulative server-egress source.  Mirrors
    /// [`Self::set_lifetime_bytes_in`] for the responder side:
    /// the source already accumulates monotonically per peer, so the
    /// runtime must overwrite rather than add to avoid double-counting.
    pub fn set_lifetime_bytes_out(&mut self, peer: SocketAddr, total: u64) {
        let entry = self.lifetime_stats.entry(peer).or_default();
        let now = Instant::now();
        entry.bytes_out = total;
        if entry.first_seen.is_none() {
            entry.first_seen = Some(now);
        }
        entry.last_seen = Some(now);
    }

    /// R222 — Read-only accessor for a peer's lifetime stats, or
    /// `None` if the peer has never connected.
    pub fn lifetime_stats_for(&self, peer: &SocketAddr) -> Option<&PeerLifetimeStats> {
        self.lifetime_stats.get(peer)
    }

    /// Record a connection failure for `peer`.
    pub fn record_failure(&mut self, peer: SocketAddr) {
        let now = Instant::now();
        let decayed = self
            .failures
            .get(&peer)
            .map(|record| self.decayed_failure_count(record, now))
            .unwrap_or(0);

        let record = self
            .failures
            .entry(peer)
            .or_insert_with(|| PeerFailureRecord {
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
    pub fn filter_backed_off(
        &self,
        actions: Vec<GovernorAction>,
        now: Instant,
    ) -> Vec<GovernorAction> {
        actions
            .into_iter()
            .filter(|a| match a {
                GovernorAction::PromoteToWarm(addr) => {
                    !self.is_backing_off(addr, now) && !self.in_flight_warm.contains(addr)
                }
                GovernorAction::PromoteToHot(addr) => {
                    !self.is_backing_off(addr, now) && !self.in_flight_hot.contains(addr)
                }
                GovernorAction::DemoteToWarm(addr) => !self.in_flight_demote_hot.contains(addr),
                GovernorAction::DemoteToCold(addr) => !self.in_flight_demote_warm.contains(addr),
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
        self.in_progress_peer_share_reqs = self.in_progress_peer_share_reqs.saturating_add(1);
    }

    /// Record that one or more peer-sharing responses arrived.
    ///
    /// Upstream: decrements `inProgressPeerShareReqs` by the number of
    /// completed requests.
    pub fn clear_peer_share_completed(&mut self, count: u32) {
        self.in_progress_peer_share_reqs = self.in_progress_peer_share_reqs.saturating_sub(count);
    }

    /// Mark that a public-root discovery request was dispatched.
    pub fn mark_public_root_request_started(&mut self) {
        self.public_root_backoff.mark_request_started();
    }

    /// Record public-root discovery request completion.
    ///
    /// Upstream successful progress uses `min 60 ttl`.
    pub fn complete_public_root_request(&mut self, now: Instant, progress: bool, ttl: Duration) {
        self.public_root_backoff
            .on_result(now, progress, ttl, Some(Duration::from_secs(60)));
    }

    /// Record public-root request failure.
    pub fn fail_public_root_request(&mut self, now: Instant) {
        self.public_root_backoff.on_failure(now);
    }

    /// Mark that a big-ledger peer discovery request was dispatched.
    pub fn mark_big_ledger_request_started(&mut self) {
        self.big_ledger_peer_backoff.mark_request_started();
    }

    /// Record big-ledger discovery request completion.
    ///
    /// Upstream successful progress uses unmodified TTL.
    pub fn complete_big_ledger_request(&mut self, now: Instant, progress: bool, ttl: Duration) {
        self.big_ledger_peer_backoff
            .on_result(now, progress, ttl, None);
    }

    /// Record big-ledger request failure.
    pub fn fail_big_ledger_request(&mut self, now: Instant) {
        self.big_ledger_peer_backoff.on_failure(now);
    }

    /// Replace the currently available inbound peers used for known-peer
    /// discovery.
    pub fn set_inbound_peers(
        &mut self,
        inbound: impl IntoIterator<Item = (SocketAddr, NodePeerSharing)>,
    ) {
        self.inbound_peers = inbound.into_iter().collect();
    }

    /// Record that inbound peer discovery was used.
    pub fn mark_inbound_peer_pick(&mut self, now: Instant) {
        self.inbound_peers_retry_time = Some(now + self.inbound_peers_retry_delay);
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
        // Clone metrics snapshot to avoid a borrow conflict: we need
        // `&GovernorState` (immutable) for failure/in-flight checks and
        // `&mut PickPolicy` (mutable) for randomized selection.
        let metrics_snapshot = self.metrics.clone();
        let mut pick = self.pick.clone();
        let actions = governor_tick(
            registry,
            &effective_targets,
            local_root_groups,
            mode,
            association,
            Some(self),
            &mut pick,
            &metrics_snapshot,
            now,
        );
        self.pick = pick;
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
    /// Request a refresh of public root peers.
    ///
    /// Upstream: request branch in `Governor.RootPeers.belowTarget`.
    RequestPublicRoots,
    /// Request a refresh of big-ledger peers.
    ///
    /// Upstream: request branch in `Governor.BigLedgerPeers.belowTarget`.
    RequestBigLedgerPeers,
    /// Add an unknown inbound peer to the known peer set.
    ///
    /// Upstream: inbound branch in `Governor.KnownPeers.belowTarget`.
    AdoptInboundPeer(SocketAddr),
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
    pick: &mut PickPolicy,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    if counts.established >= targets.target_established {
        return Vec::new();
    }
    let needed = targets.target_established - counts.established;

    // Collect cold peers in four buckets (local-root non-tepid, local-root
    // tepid, other non-tepid, other tepid) so non-tepid peers are promoted
    // first within each source tier.  Each bucket is individually
    // randomized via `pick` to avoid deterministic SocketAddr ordering.
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

    // Randomize within each priority tier, then chain.
    let local_fresh = pick.pick(local_fresh.len(), local_fresh);
    let local_tepid = pick.pick(local_tepid.len(), local_tepid);
    let other_fresh = pick.pick(other_fresh.len(), other_fresh);
    let other_tepid = pick.pick(other_tepid.len(), other_tepid);

    local_fresh
        .into_iter()
        .chain(local_tepid)
        .chain(other_fresh)
        .chain(other_tepid)
        .take(needed)
        .map(GovernorAction::PromoteToWarm)
        .collect()
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
    pick: &mut PickPolicy,
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

    let local_fresh = pick.pick(local_fresh.len(), local_fresh);
    let local_tepid = pick.pick(local_tepid.len(), local_tepid);
    let other_fresh = pick.pick(other_fresh.len(), other_fresh);
    let other_tepid = pick.pick(other_tepid.len(), other_tepid);

    local_fresh
        .into_iter()
        .chain(local_tepid)
        .chain(other_fresh)
        .chain(other_tepid)
        .take(needed)
        .map(GovernorAction::PromoteToHot)
        .collect()
}

/// Multi-peer hot-promotion entry point that mirrors upstream
/// `Ouroboros.Network.PeerSelection.Governor.HotPeers.evaluatePromotions`.
///
/// Currently a thin facade around [`evaluate_warm_to_hot_promotions`] —
/// existing call sites that already produce N promotions per tick continue
/// to do so unchanged.  The dedicated entry point exists so that:
///
/// 1. The runtime BlockFetch dispatcher (Slice E) can locate the canonical
///    hot-promotion call site.
/// 2. Future weight-aware refinements can be added under this name without
///    touching the internal helper.
/// 3. Upstream module structure stays mirrored: `HotPeers.evaluatePromotions`
///    delegates to `Governor.PromoteWarmToHot.evaluate` in Haskell, exactly
///    as this function delegates to `evaluate_warm_to_hot_promotions` here.
///
/// The `_scheduling` parameter is currently unused (weights affect the
/// connection-manager scheduler, not promotion candidacy) but is part of
/// the API to preserve the contract that scheduling is consulted at every
/// promotion decision.  Removing it would break upstream parity at the
/// callable-API layer.
pub fn evaluate_hot_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
    _scheduling: &HotPeerScheduling,
) -> Vec<GovernorAction> {
    evaluate_warm_to_hot_promotions(registry, targets, pick)
}

/// Evaluate which hot peers should be demoted to warm because we have
/// more active peers than the target.
///
/// Prefers demoting non-local-root peers first.  Within the non-local
/// tier, peers are scored by `PeerMetrics` (upstreamyness + fetchyness)
/// so that more productive peers are kept hot.  This matches the
/// upstream `hotDemotionPolicy` which adds metric scores to random
/// weights.
pub fn evaluate_hot_to_warm_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
    metrics: &PeerMetrics,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    if counts.active <= targets.target_active {
        return Vec::new();
    }
    let excess = counts.active - targets.target_active;

    // Collect hot peers, preferring to demote non-local-root first.
    // Non-local peers are scored so productive peers survive.
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

    // Score-aware selection: lower-scored non-local peers are
    // demoted first.  `pick_scored` puts highest-scored first,
    // so we take from the end (lowest-scored) by asking for all
    // then reversing.
    let mut non_local_scored = pick.pick_scored(non_local_hot.len(), non_local_hot, metrics);
    non_local_scored.reverse(); // lowest-scored first → demote first
    let local_hot = pick.pick(local_hot.len(), local_hot);

    non_local_scored
        .into_iter()
        .chain(local_hot)
        .take(excess)
        .map(GovernorAction::DemoteToWarm)
        .collect()
}

/// Evaluate which warm peers should be demoted to cold because we have
/// more established peers than the target.
///
/// Prefers demoting non-local-root peers first.
pub fn evaluate_warm_to_cold_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
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

    let non_local_warm = pick.pick(non_local_warm.len(), non_local_warm);
    let local_warm = pick.pick(local_warm.len(), local_warm);

    non_local_warm
        .into_iter()
        .chain(local_warm)
        .take(excess)
        .map(GovernorAction::DemoteToCold)
        .collect()
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
    pick: &mut PickPolicy,
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
            let chosen = pick.pick(needed, cold_peers);
            for addr in chosen {
                actions.push(GovernorAction::PromoteToWarm(addr));
            }
        }

        // Promote warm→hot until we meet hot_valency.
        if hot_count < group.hot_valency {
            let needed = (group.hot_valency - hot_count) as usize;
            let chosen = pick.pick(needed, warm_peers);
            for addr in chosen {
                actions.push(GovernorAction::PromoteToHot(addr));
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
    pick: &mut PickPolicy,
) -> Vec<GovernorAction> {
    let warm_or_hot = registry
        .iter()
        .filter(|(_, e)| {
            is_big_ledger(e) && matches!(e.status, PeerStatus::PeerWarm | PeerStatus::PeerHot)
        })
        .count();

    let target = targets.target_established_big_ledger;
    if warm_or_hot >= target {
        return Vec::new();
    }
    let needed = target - warm_or_hot;

    let candidates: Vec<SocketAddr> = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerCold)
        .map(|(addr, _)| *addr)
        .collect();
    pick.pick(needed, candidates)
        .into_iter()
        .map(GovernorAction::PromoteToWarm)
        .collect()
}

/// Evaluate which warm big-ledger peers should be promoted to hot to meet
/// the `target_active_big_ledger` target.
pub fn evaluate_warm_to_hot_big_ledger_promotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
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

    let candidates: Vec<SocketAddr> = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerWarm)
        .map(|(addr, _)| *addr)
        .collect();
    pick.pick(needed, candidates)
        .into_iter()
        .map(GovernorAction::PromoteToHot)
        .collect()
}

/// Evaluate which hot big-ledger peers should be demoted to warm when
/// we exceed `target_active_big_ledger`.
pub fn evaluate_hot_to_warm_big_ledger_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
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

    let candidates: Vec<SocketAddr> = registry
        .iter()
        .filter(|(_, e)| {
            is_big_ledger(e)
                && e.status == PeerStatus::PeerHot
                && !e.sources.contains(&PeerSource::PeerSourceLocalRoot)
        })
        .map(|(addr, _)| *addr)
        .collect();
    pick.pick(excess, candidates)
        .into_iter()
        .map(GovernorAction::DemoteToWarm)
        .collect()
}

/// Evaluate which warm big-ledger peers should be demoted to cold when
/// we exceed `target_established_big_ledger`.
pub fn evaluate_warm_to_cold_big_ledger_demotions(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
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

    let candidates: Vec<SocketAddr> = registry
        .iter()
        .filter(|(_, e)| is_big_ledger(e) && e.status == PeerStatus::PeerWarm)
        .map(|(addr, _)| *addr)
        .collect();
    pick.pick(excess, candidates)
        .into_iter()
        .map(GovernorAction::DemoteToCold)
        .collect()
}

/// Evaluate whether a public-root refresh request should be issued.
///
/// Upstream analogue: request branch in `Governor.RootPeers.belowTarget`.
pub fn evaluate_request_public_roots(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    state: &GovernorState,
    now: Instant,
) -> Vec<GovernorAction> {
    if !state.enable_root_big_ledger_requests {
        return Vec::new();
    }
    let root_count = registry
        .iter()
        .filter(|(_, e)| !is_big_ledger(e) && e.is_root_peer())
        .count();
    if root_count >= targets.target_root {
        return Vec::new();
    }
    if !state.public_root_backoff.can_request(now) {
        return Vec::new();
    }
    vec![GovernorAction::RequestPublicRoots]
}

/// Evaluate whether a big-ledger peer refresh request should be issued.
///
/// Upstream analogue: request branch in `Governor.BigLedgerPeers.belowTarget`.
pub fn evaluate_request_big_ledger_peers(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    state: &GovernorState,
    now: Instant,
) -> Vec<GovernorAction> {
    if !state.enable_root_big_ledger_requests {
        return Vec::new();
    }
    let known_big_ledger = registry.iter().filter(|(_, e)| is_big_ledger(e)).count();
    if known_big_ledger >= targets.target_known_big_ledger {
        return Vec::new();
    }
    if !state.big_ledger_peer_backoff.can_request(now) {
        return Vec::new();
    }
    vec![GovernorAction::RequestBigLedgerPeers]
}

// ---------------------------------------------------------------------------
// Forget cold peers — known-peer set management
// ---------------------------------------------------------------------------

/// Evaluate which cold, non-local-root, non-big-ledger peers should be
/// forgotten (removed from the known set) when the known count exceeds
/// `target_known`.
///
/// This policy also enforces the one-sided root-peer floor from
/// `target_root`: root peers are only forgotten when the current regular
/// root count is above that floor.
///
/// Upstream equivalent:
/// `Ouroboros.Network.PeerSelection.Governor.KnownPeers.belowTarget` —
/// the governor forgets cold peers it no longer needs sources for.
pub fn evaluate_forget_cold_peers(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    pick: &mut PickPolicy,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    let target = targets.target_known;
    if counts.known <= target {
        return Vec::new();
    }
    let excess = counts.known - target;
    let regular_root_count = registry
        .iter()
        .filter(|(_, e)| !is_big_ledger(e) && e.is_root_peer())
        .count();

    // Only forget cold, ephemeral peers (peer-share or public-root that
    // are no longer essential). Local-root, Bootstrap, Ledger, and
    // BigLedger peers are never forgotten.
    let forgettable_sources = [
        PeerSource::PeerSourcePeerShare,
        PeerSource::PeerSourcePublicRoot,
    ];

    let mut non_root_candidates = Vec::new();
    let mut root_candidates = Vec::new();

    for (addr, entry) in registry.iter() {
        if is_big_ledger(entry)
            || entry.status != PeerStatus::PeerCold
            || !entry
                .sources
                .iter()
                .all(|s| forgettable_sources.contains(s))
        {
            continue;
        }

        if entry.is_root_peer() {
            root_candidates.push(*addr);
        } else {
            non_root_candidates.push(*addr);
        }
    }

    // Prefer forgetting non-root ephemeral peers first.
    let mut selected = pick.pick(excess, non_root_candidates);
    let remaining = excess.saturating_sub(selected.len());

    if remaining > 0 {
        // Enforce the one-sided root floor (`target_root`): never forget
        // root peers below this threshold.
        let root_forget_budget = regular_root_count.saturating_sub(targets.target_root);
        let root_take = remaining.min(root_forget_budget);
        selected.extend(pick.pick(root_take, root_candidates));
    }

    selected
        .into_iter()
        .map(GovernorAction::ForgetPeer)
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
    pick: &mut PickPolicy,
) -> Vec<GovernorAction> {
    // Check budget.
    if state.in_progress_peer_share_reqs >= state.max_in_progress_peer_share_reqs {
        return Vec::new();
    }
    let budget =
        (state.max_in_progress_peer_share_reqs - state.in_progress_peer_share_reqs) as usize;

    // Check whether known-peer set is below target.
    let counts = regular_peer_counts(registry);
    if counts.known >= targets.target_known {
        return Vec::new();
    }

    // Pick warm/hot peers that can serve PeerSharing requests.
    // Exclude local-root and bootstrap sources — they are configured
    // rather than discovered and are not expected to participate in
    // gossip-based peer sharing.
    let candidates: Vec<SocketAddr> = registry
        .iter()
        .filter(|(_, entry)| {
            matches!(entry.status, PeerStatus::PeerWarm | PeerStatus::PeerHot)
                && !entry.sources.contains(&PeerSource::PeerSourceLocalRoot)
                && !entry.sources.contains(&PeerSource::PeerSourceBootstrap)
                && !is_big_ledger(entry)
        })
        .map(|(addr, _)| *addr)
        .collect();
    pick.pick(budget, candidates)
        .into_iter()
        .map(GovernorAction::ShareRequest)
        .collect()
}

/// Evaluate known-peer discovery when below `target_known`.
///
/// This mirrors upstream `Governor.KnownPeers.belowTarget`: flip a coin to
/// either adopt unknown inbound peers or issue peer-share requests. The
/// inbound branch is only eligible when no peer-share requests are currently
/// in progress and the inbound retry timer has elapsed.
pub fn evaluate_known_peer_discovery(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    state: &GovernorState,
    pick: &mut PickPolicy,
    now: Instant,
) -> Vec<GovernorAction> {
    let counts = regular_peer_counts(registry);
    if counts.known >= targets.target_known {
        return Vec::new();
    }

    let available_for_peer_share: BTreeSet<SocketAddr> = registry
        .iter()
        .filter(|(_, entry)| {
            matches!(entry.status, PeerStatus::PeerWarm | PeerStatus::PeerHot)
                && !entry.sources.contains(&PeerSource::PeerSourceLocalRoot)
                && !entry.sources.contains(&PeerSource::PeerSourceBootstrap)
                && !is_big_ledger(entry)
        })
        .map(|(addr, _)| *addr)
        .collect();

    let use_inbound_peers = pick.coin_flip();
    let inbound_retry_elapsed = state
        .inbound_peers_retry_time
        .is_none_or(|deadline| now >= deadline);
    let inbound_available: Vec<SocketAddr> = state
        .inbound_peers
        .keys()
        .copied()
        .filter(|peer| registry.get(peer).is_none())
        .collect();

    if state.in_progress_peer_share_reqs == 0
        && inbound_retry_elapsed
        && (use_inbound_peers || available_for_peer_share.is_empty())
        && !inbound_available.is_empty()
    {
        let objective = targets.target_known - counts.known;
        let limit = state.max_inbound_peers.min(objective);
        return pick
            .pick(limit, inbound_available)
            .into_iter()
            .map(GovernorAction::AdoptInboundPeer)
            .collect();
    }

    evaluate_peer_share_requests(registry, targets, state, pick)
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
    entry.sources.contains(&PeerSource::PeerSourceBootstrap) || trustable_locals.contains(addr)
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
    registry.iter().all(|(addr, entry)| match entry.status {
        PeerStatus::PeerCold | PeerStatus::PeerCooling => true,
        PeerStatus::PeerWarm | PeerStatus::PeerHot => {
            is_trustable_peer(addr, entry, &trustable_locals)
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
            GovernorAction::PromoteToWarm(addr) | GovernorAction::PromoteToHot(addr) => registry
                .get(addr)
                .is_some_and(|entry| is_trustable_peer(addr, entry, &trustable_locals)),
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
#[allow(clippy::too_many_arguments)]
pub fn governor_tick(
    registry: &PeerRegistry,
    targets: &GovernorTargets,
    local_root_groups: &[LocalRootTargets],
    mode: PeerSelectionMode,
    association: AssociationMode,
    state: Option<&GovernorState>,
    pick: &mut PickPolicy,
    metrics: &PeerMetrics,
    now: Instant,
) -> Vec<GovernorAction> {
    let mut actions = Vec::new();

    match mode {
        PeerSelectionMode::Sensitive => {
            // In sensitive mode:
            // 1. Demote all non-trustable hot peers to warm.
            actions.extend(evaluate_sensitive_hot_demotions(
                registry,
                local_root_groups,
            ));
            // 2. Demote all non-trustable warm peers to cold.
            actions.extend(evaluate_sensitive_warm_demotions(
                registry,
                local_root_groups,
            ));
            // 3. Enforce local root valency (trustable groups only).
            actions.extend(enforce_local_root_valency(
                registry,
                local_root_groups,
                pick,
            ));
            // 4. Normal promotion targets, filtered to trustable peers only.
            let mut promotions = Vec::new();
            promotions.extend(evaluate_cold_to_warm_promotions(registry, targets, pick));
            promotions.extend(evaluate_warm_to_hot_promotions(registry, targets, pick));
            actions.extend(filter_sensitive_promotions(
                promotions,
                registry,
                local_root_groups,
            ));
            // 5. Big-ledger promotions are suppressed in sensitive mode —
            //    big-ledger peers are not trustable by definition.
            // 6. Forget excess cold peers.
            actions.extend(evaluate_forget_cold_peers(registry, targets, pick));
            // 7. Forget cold peers that have exceeded max connection retries.
            if let Some(gs) = state {
                actions.extend(evaluate_forget_failed_peers(registry, gs, now));
            }
        }
        PeerSelectionMode::Normal => {
            // 1. Local root valency takes priority.
            actions.extend(enforce_local_root_valency(
                registry,
                local_root_groups,
                pick,
            ));
            // 2. Global promotion targets.  Hot promotions go through the
            //    upstream-style `evaluate_hot_promotions` entry point so
            //    runtime callers and the canonical tick path use the same
            //    function name.  Sensitive mode keeps the direct
            //    `evaluate_warm_to_hot_promotions` call because the
            //    `filter_sensitive_promotions` post-step expects the legacy
            //    flat output and intentionally bypasses scheduling weights
            //    for trustable-only promotion.
            actions.extend(evaluate_cold_to_warm_promotions(registry, targets, pick));
            let default_scheduling = HotPeerScheduling::new();
            let hot_sched = state
                .map(|s| &s.hot_scheduling)
                .unwrap_or(&default_scheduling);
            actions.extend(evaluate_hot_promotions(registry, targets, pick, hot_sched));
            // 3. Big-ledger peer promotions (suppressed in LocalRootsOnly).
            if association == AssociationMode::Unrestricted {
                actions.extend(evaluate_cold_to_warm_big_ledger_promotions(
                    registry, targets, pick,
                ));
                actions.extend(evaluate_warm_to_hot_big_ledger_promotions(
                    registry, targets, pick,
                ));
            }
            // 4. Global demotion targets.
            actions.extend(evaluate_hot_to_warm_demotions(
                registry, targets, pick, metrics,
            ));
            actions.extend(evaluate_warm_to_cold_demotions(registry, targets, pick));
            // 5. Big-ledger peer demotions.
            actions.extend(evaluate_hot_to_warm_big_ledger_demotions(
                registry, targets, pick,
            ));
            actions.extend(evaluate_warm_to_cold_big_ledger_demotions(
                registry, targets, pick,
            ));
            // 6. Forget excess cold peers.
            actions.extend(evaluate_forget_cold_peers(registry, targets, pick));
            // 7. Forget cold peers that have exceeded max connection retries.
            if let Some(gs) = state {
                actions.extend(evaluate_forget_failed_peers(registry, gs, now));
            }
            // 8. Peer sharing requests — suppressed in LocalRootsOnly mode
            //    since BP/hidden-relay nodes should not participate in peer
            //    sharing discovery.
            if association == AssociationMode::Unrestricted {
                if let Some(gs) = state {
                    actions.extend(evaluate_request_public_roots(registry, targets, gs, now));
                    actions.extend(evaluate_request_big_ledger_peers(
                        registry, targets, gs, now,
                    ));
                    actions.extend(evaluate_known_peer_discovery(
                        registry, targets, gs, pick, now,
                    ));
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
            let all_trusted = registry.iter().all(|(addr, entry)| match entry.status {
                PeerStatus::PeerCold | PeerStatus::PeerCooling => true,
                PeerStatus::PeerWarm | PeerStatus::PeerHot => trustable_locals.contains(addr),
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
pub fn churn_decrease_established(regime: ChurnRegime, established: usize, active: usize) -> usize {
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
    /// Minimal delay between adopting inbound peers into known peers.
    ///
    /// Upstream: `inboundPeersRetryDelay` = 60 s.
    pub inbound_peers_retry_delay: Duration,
    /// Maximum inbound peers adopted in a single discovery round.
    ///
    /// Upstream: `maxInboundPeers` = 10.
    pub max_inbound_peers: usize,
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
            inbound_peers_retry_delay: Duration::from_secs(60),
            max_inbound_peers: 10,
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
mod tests;
