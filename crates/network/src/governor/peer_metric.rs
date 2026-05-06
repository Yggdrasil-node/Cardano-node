//! Peer metrics + scheduling — failure backoff, peer-pick policy, and
//! hot-peer egress scheduling.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.PeerSelection.PeerMetric` (`PeerMetric`,
//!   `HeaderMetricsTracer`, `BlockFetchMetricsTracer`)
//! - `Ouroboros.Network.PeerSelection.LedgerPeers.Utils` (peer-pick
//!   randomized policy)
//! - `Ouroboros.Network.PeerSelection.Governor.RootPeers` (failure
//!   backoff bookkeeping)
//!
//! `Xorshift64` and `PickPolicy` together implement the deterministic
//! randomized peer-selection used by the governor; `PeerFailureRecord` +
//! `RequestBackoffState` track per-peer failure history with exponential
//! backoff; `PeerMetrics` and `HotPeerScheduling` track latency and
//! per-protocol egress weights for hot peers.
//!
//! Extracted from `governor.rs` in R270c.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::multiplexer::MiniProtocolNum;
use crate::peer_registry::{PeerRegistry, PeerSource, PeerStatus};

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
            && !super::is_big_ledger(entry)
        {
            out.insert(*addr);
        }
    }
    out
}
