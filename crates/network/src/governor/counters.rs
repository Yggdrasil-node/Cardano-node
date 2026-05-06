//! Governor view-layer types — counters, outbound-connections state,
//! per-protocol timeouts, and connection-manager counters.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.PeerSelection.Governor.Types` (`PeerSelectionView` /
//!   `PeerSelectionCounters` pattern synonym, `OutboundConnectionsState`,
//!   `PeerSelectionTimeouts`)
//! - `Ouroboros.Network.ConnectionManager.Types` (`ConnectionManagerCounters`)
//!
//! These are the observability + orchestration-timing layer of the
//! governor — pure data types derived from [`super::GovernorState`] and
//! the peer registry, used by the runtime for metrics export, oncall
//! dashboards, and the inbound governor's connection-manager view.
//!
//! Extracted from `governor.rs` in R270e — the final slice of the
//! per-domain governor split. After R270e, `governor.rs` is a thin
//! orchestration shell (just imports + `pub mod ... ; pub use ...`
//! re-export blocks + the `mod tests;` declaration).

use std::time::Duration;

use crate::peer_registry::{PeerRegistry, PeerSource, PeerStatus};
use crate::root_peers::UseBootstrapPeers;

use super::state::{GovernorState, is_big_ledger, is_trustable_peer, trustable_local_root_set};
use super::types::{AssociationMode, LocalRootTargets};

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
