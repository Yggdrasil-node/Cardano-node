//! Churn governor — periodic decrease/restore cycle for peer targets.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.PeerSelection.Churn` (`peerChurnGovernor`,
//!   `ChurnMode`, `ChurnRegime`)
//! - `Ouroboros.Network.BlockFetch.ConsensusInterface` (`FetchMode`)
//! - `Cardano.Node.Diffusion.mkReadFetchMode` (mode derivation)
//!
//! The two-phase churn cycle decreases active and established targets in
//! turn (`ChurnPhase`), demoting peers downward through the connection
//! lifecycle (hot to warm to cold), then restores targets to allow fresh
//! promotions. The decrease formula is `max(0, v - max(1, v/5))` —
//! "remove 20% or at least one peer per cycle".
//!
//! Extracted from `governor.rs` in R270b.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side churn-cycle policy
//! configuration + mode classification. Surfaces three upstream
//! concepts in one focused module: `ChurnMode` / `ChurnRegime`
//! enums (originally from `Ouroboros.Network.PeerSelection.Churn`),
//! the `decrease` / `decreaseWithMin` policy helpers (also from
//! `Churn.hs`), and `FetchMode` derivation (from
//! `Ouroboros.Network.BlockFetch.ConsensusInterface` +
//! `Cardano.Node.Diffusion.mkReadFetchMode`). The actual
//! `peerChurnGovernor` driver loop lives in
//! `crates/network/src/governor.rs` (the strict mirror of
//! `Ouroboros.Network.PeerSelection.Governor`); this file is the
//! configuration / policy half it consumes.

use std::time::{Duration, Instant};

use crate::ledger_peers_provider::LedgerStateJudgement;
use crate::root_peers::UseBootstrapPeers;

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
