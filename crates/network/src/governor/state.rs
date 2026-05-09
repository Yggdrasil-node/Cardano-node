//! Governor state + decision functions.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.PeerSelection.Governor.PeerSelectionState` (`PeerSelectionState`)
//! - `Ouroboros.Network.PeerSelection.Governor.Monitor` (decision evaluator family)
//! - `Ouroboros.Network.PeerSelection.Governor.PeerSelectionActions` (action emitter)
//!
//! Holds the mutable [`GovernorState`] carried across governor ticks plus
//! the family of `evaluate_*` decision functions and the [`governor_tick`]
//! orchestrator. Each `evaluate_*` mirrors a single peer-selection
//! transition rule from upstream's `peerSelectionGovernor` body.
//!
//! Extracted from `governor.rs` in R270d as the fourth slice of the
//! per-domain governor split — this is the bulk of the governor and the
//! largest single peel-off in the R270 arc.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis combining
//! upstream `Ouroboros.Network.PeerSelection.Governor.PeerSelectionState.hs`
//! (PeerSelectionState data), `Ouroboros.Network.PeerSelection.Governor.Monitor.hs`
//! (decision-evaluator family), and
//! `Ouroboros.Network.PeerSelection.Governor.PeerSelectionActions.hs`
//! (action emitter). Yggdrasil's `governor_tick` orchestrator and
//! the family of `evaluate_*` decision functions live in one module
//! for cohesion; upstream splits state vs decision-eval vs action-
//! emit across three files.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::peer_registry::{PeerRegistry, PeerRegistryEntry, PeerSource, PeerStatus};

use super::churn::{
    ChurnConfig, ChurnPhase, ChurnRegime, FetchMode, churn_decrease_active,
    churn_decrease_established,
};
use super::peer_metric::{
    HotPeerScheduling, PeerFailureRecord, PeerMetrics, PickPolicy, RequestBackoffState,
};
use super::types::{
    AssociationMode, GovernorTargets, LocalRootTargets, NodePeerSharing, PeerSelectionMode,
};

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
pub fn is_big_ledger(entry: &PeerRegistryEntry) -> bool {
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
pub fn trustable_local_root_set(groups: &[LocalRootTargets]) -> BTreeSet<SocketAddr> {
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
pub fn is_trustable_peer(
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
