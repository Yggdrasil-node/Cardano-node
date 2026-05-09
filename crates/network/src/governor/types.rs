//! Governor types — targets, peer-sharing flag, association mode, and
//! sensitive-mode predicates.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.PeerSelection.Governor.Types` (`PeerSelectionTargets`,
//!   `AssociationMode`)
//! - `Ouroboros.Network.PeerSelection.PeerSharing` (`PeerSharing`)
//! - `Cardano.Network.PeerSelection.Bootstrap` (`requiresBootstrapPeers`)
//!
//! Extracted from `governor.rs` in R270a as the first slice of the
//! per-domain governor split mirroring upstream `Ouroboros.Network.PeerSelection.*`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side synthesis combining
//! `Ouroboros.Network.PeerSelection.Governor.Types`
//! (PeerSelectionTargets, AssociationMode),
//! `Ouroboros.Network.PeerSelection.PeerSharing` (PeerSharing flag),
//! and `Cardano.Network.PeerSelection.Bootstrap` (requiresBootstrapPeers
//! predicate). The three upstream files cover related but separate
//! config / mode / predicate concerns; Yggdrasil unifies them in
//! one types module that the rest of the governor cluster
//! consumes.

use std::net::SocketAddr;

use crate::ledger_peers_provider::LedgerStateJudgement;
use crate::peer_selection::LocalRootConfig;
use crate::root_peers::{UseBootstrapPeers, UseLedgerPeers};

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
