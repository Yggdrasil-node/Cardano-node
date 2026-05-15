//! Runtime governor configuration — derived from node configuration.
//!
//! Mirrors the configuration-overlay layer that upstream
//! `Cardano.Node.Run.checkPointsAndApplyChunkOptions` builds before
//! handing off to `peerSelectionGovernor`. Holds tick interval,
//! keepalive cadence, target peer counts, and shared handles for
//! cross-task coordination (block-fetch instrumentation, fetch-worker
//! pool, chainsync-worker pool, density registry, ledger-judgement
//! settings).
//!
//! Extracted from `runtime.rs` in R271a as the first slice of the
//! per-domain runtime split.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side configuration-overlay
//! layer aggregating tick interval, keep-alive cadence, target
//! peer counts, and cross-task shared handles. Mirrors the
//! configuration prep that upstream
//! `Cardano.Node.Run.checkPointsAndApplyChunkOptions` does before
//! handing off to `Ouroboros.Network.PeerSelection.Governor`;
//! Haskell wires those parameters inline, Yggdrasil isolates them
//! in a struct.

use std::time::Duration;

use yggdrasil_consensus::EpochSchedule;
use yggdrasil_network::{ConsensusMode, GovernorTargets, NodePeerSharing};

use super::{LedgerJudgementSettings, SharedFetchWorkerPool};

/// Runtime governor configuration derived from node configuration.
#[derive(Clone, Debug)]
pub struct RuntimeGovernorConfig {
    /// Period between governor evaluation ticks.
    pub tick_interval: Duration,
    /// KeepAlive cadence for established warm peers.
    pub keepalive_interval: Option<Duration>,
    /// Node-level peer-sharing willingness for governor association mode.
    pub peer_sharing: NodePeerSharing,
    /// Consensus mode used to derive governor churn regime.
    pub consensus_mode: ConsensusMode,
    /// Target peer counts maintained by the governor.
    pub targets: GovernorTargets,
    /// Optional shared `BlockFetchInstrumentation` handle. When set, the
    /// governor tick propagates the per-tick `fetch_mode_from_judgement`
    /// signal into the pool's per-peer concurrency cap, mirroring upstream
    /// `Ouroboros.Network.BlockFetch.ConsensusInterface.mkReadFetchMode`
    /// where `LedgerStateJudgement` drives `bfcMaxConcurrency{BulkSync,
    /// Deadline}`. When `None`, the pool stays in whatever mode it was
    /// constructed with — same behavior as before this slice.
    pub block_fetch_pool: Option<yggdrasil_network::BlockFetchInstrumentation>,
    /// Genesis-derived inputs feeding the live `LedgerStateJudgement`
    /// computation in `ChainDbConsensusLedgerSource`. Mirrors upstream
    /// `mkLedgerStateJudgement` from
    /// `Cardano.Node.Diffusion.Configuration` — the judgement flips from
    /// `YoungEnough` to `TooOld` when `now - tipSlotTime` exceeds
    /// `max_ledger_state_age_secs`. Defaults to the conservative
    /// fallback (no genesis timing → always `YoungEnough`) so existing
    /// test paths keep working.
    pub ledger_judgement_settings: LedgerJudgementSettings,
    /// Era-aware epoch schedule used when recovering the ledger snapshot
    /// that feeds live ledger-peer discovery. Preview/preprod/mainnet can
    /// cross hard-fork boundaries during replay; using the non-boundary
    /// path can reject otherwise valid PPUP timing during runtime resume.
    pub epoch_schedule: Option<EpochSchedule>,
    /// Optional shared per-peer ChainSync header-density registry
    /// (Slice GD-Final).  When set, the governor loop reads density
    /// values from the registry into `PeerMetrics::density` before
    /// each tick so `combined_score` can apply the density-aware
    /// hot-demotion bonus.  Wire to the same `DensityRegistry`
    /// instance the sync service uses
    /// (`VerifiedSyncServiceConfig::density_registry`) so writes from
    /// the sync hook land where the governor reads.
    pub density_registry: Option<yggdrasil_node_sync::DensityRegistry>,
    /// Operator-configured upper bound on concurrent BlockFetch
    /// peers.  When `> 1`, the governor migrates each warm peer's
    /// `BlockFetchClient` into a per-peer
    /// [`yggdrasil_node_sync::blockfetch_worker::FetchWorkerHandle`] at promote
    /// time, populating the shared
    /// [`SharedFetchWorkerPool`] held by `OutboundPeerManager`.
    /// The sync loop's multi-peer dispatch branch then activates.
    /// Default `1` keeps the pool empty and the legacy single-peer
    /// path active.
    ///
    /// Mirrors upstream `bfcMaxConcurrencyDeadline = 1` /
    /// `bfcMaxConcurrencyBulkSync = 2`.
    pub max_concurrent_block_fetch_peers: u8,
    /// Optional shared `FetchWorkerPool` cloned from runtime startup
    /// (see [`new_shared_fetch_worker_pool`]).  When `Some`, the
    /// governor's `OutboundPeerManager` uses this pool so the sync
    /// loop's `VerifiedSyncServiceConfig::shared_fetch_worker_pool`
    /// observes the registrations made here.  When `None`, the
    /// governor creates its own private pool — useful for tests
    /// that don't need cross-task sharing.
    pub shared_fetch_worker_pool: Option<SharedFetchWorkerPool>,
    /// Optional shared `ChainSyncWorkerPool` cloned from runtime
    /// startup (see
    /// [`yggdrasil_node_sync::chainsync_worker::new_shared_chainsync_worker_pool`]).
    /// When `Some`, the governor exports the live registered-worker
    /// count to `/metrics` each tick.  When `None`, the
    /// `chainsync_workers_registered` gauge stays at 0.
    pub shared_chainsync_worker_pool:
        Option<yggdrasil_node_sync::chainsync_worker::SharedChainSyncWorkerPool>,
}

impl RuntimeGovernorConfig {
    /// Construct a runtime governor config from the explicit interval and targets.
    pub fn new(
        tick_interval: Duration,
        keepalive_interval: Option<Duration>,
        peer_sharing: NodePeerSharing,
        consensus_mode: ConsensusMode,
        targets: GovernorTargets,
    ) -> Self {
        Self {
            tick_interval,
            keepalive_interval,
            peer_sharing,
            consensus_mode,
            targets,
            block_fetch_pool: None,
            ledger_judgement_settings: LedgerJudgementSettings::default(),
            epoch_schedule: None,
            density_registry: None,
            max_concurrent_block_fetch_peers: 1,
            shared_fetch_worker_pool: None,
            shared_chainsync_worker_pool: None,
        }
    }

    /// Attach a shared `ChainSyncWorkerPool` so the governor's
    /// metrics tick exports the registered-worker count.  Wire to
    /// the same instance the sync service uses via
    /// `VerifiedSyncServiceConfig::shared_chainsync_worker_pool`.
    pub fn with_shared_chainsync_worker_pool(
        mut self,
        pool: Option<yggdrasil_node_sync::chainsync_worker::SharedChainSyncWorkerPool>,
    ) -> Self {
        self.shared_chainsync_worker_pool = pool;
        self
    }

    /// Set the operator-configured `max_concurrent_block_fetch_peers`
    /// knob.  Values `> 1` activate the upstream-faithful multi-peer
    /// BlockFetch path: the governor migrates each warm peer's
    /// `BlockFetchClient` into a worker at promote time.
    pub fn with_max_concurrent_block_fetch_peers(mut self, knob: u8) -> Self {
        self.max_concurrent_block_fetch_peers = knob;
        self
    }

    /// Attach a shared `FetchWorkerPool` so the governor's
    /// `OutboundPeerManager` writes to the same pool the sync
    /// loop reads from via
    /// `VerifiedSyncServiceConfig::shared_fetch_worker_pool`.
    pub fn with_shared_fetch_worker_pool(mut self, pool: Option<SharedFetchWorkerPool>) -> Self {
        self.shared_fetch_worker_pool = pool;
        self
    }

    /// Attach a shared per-peer ChainSync density registry so the
    /// governor's hot-demotion scoring receives the live chain-quality
    /// signal.  Pass the same `DensityRegistry` instance as the sync
    /// service is using (`VerifiedSyncServiceConfig::density_registry`)
    /// so writes from the sync hook land where the governor reads.
    pub fn with_density_registry(
        mut self,
        registry: Option<yggdrasil_node_sync::DensityRegistry>,
    ) -> Self {
        self.density_registry = registry;
        self
    }

    /// Attach a shared `BlockFetchInstrumentation` handle so the governor
    /// tick propagates `fetch_mode_from_judgement(...)` into the pool's
    /// per-peer concurrency cap. Pass `None` (the default) to keep the
    /// pool's mode pinned at construction time.
    pub fn with_block_fetch_pool(
        mut self,
        pool: Option<yggdrasil_network::BlockFetchInstrumentation>,
    ) -> Self {
        self.block_fetch_pool = pool;
        self
    }

    /// Attach genesis-derived [`LedgerJudgementSettings`] so the
    /// `ChainDbConsensusLedgerSource` returned by the governor's ledger-peer
    /// refresh can compute a real wall-clock-based `LedgerStateJudgement`
    /// instead of the legacy hardcoded `YoungEnough`. Pass the default
    /// value (the constructor sets it) to keep the legacy fallback
    /// behavior — useful for tests that don't configure genesis.
    pub fn with_ledger_judgement_settings(mut self, settings: LedgerJudgementSettings) -> Self {
        self.ledger_judgement_settings = settings;
        self
    }

    /// Attach the era-aware epoch schedule used by ChainDb-backed
    /// ledger-peer recovery. Production callers should pass
    /// `NodeConfigFile::epoch_schedule()` so governor refreshes replay
    /// storage with the same epoch-boundary semantics as verified sync.
    pub fn with_epoch_schedule(mut self, epoch_schedule: Option<EpochSchedule>) -> Self {
        self.epoch_schedule = epoch_schedule;
        self
    }
}
