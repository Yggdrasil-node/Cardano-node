//! Runtime-side bridges between consensus state and the network crate's
//! peer-source traits.
//!
//! Mirrors upstream `Cardano.Node.Diffusion.Configuration` glue around
//! `Ouroboros.Network.Diffusion.LedgerPeers::LedgerPeers` and
//! `Ouroboros.Network.PeerSelection.LedgerPeers::PeerSnapshot` —
//! the runtime-side hooks that feed the diffusion layer's peer-source
//! traits with consensus and on-disk snapshot state.
//!
//! Five items move from `runtime.rs` here:
//!
//! - `ChainDbConsensusLedgerSource` — implements
//!   `ConsensusLedgerPeerSource` over a shared `ChainDb` so the network
//!   crate's `live_refresh_ledger_peer_registry` can pull consensus-fed
//!   `(latest_slot, judgement, ledger_snapshot)` inputs without
//!   depending on storage types.
//! - `derive_judgement_for_observe` — derives a `LedgerStateJudgement`
//!   from the recovered tip's wall-clock age, falling back to
//!   `YoungEnough` when genesis timing inputs are missing (test paths).
//! - `wall_clock_unix_secs` — `SystemTime::now()` ↔ Unix-epoch f64.
//! - `block_producer_ledger_state_judgement` — block-producer-loop
//!   variant that reads `RuntimeBlockProducerConfig.max_ledger_state_age_secs`.
//! - `FilePeerSnapshotSource` — implements `PeerSnapshotFileSource`
//!   over the configured `peerSnapshotFile` path so the diffusion
//!   layer's `live_refresh_ledger_peer_registry` can re-read the
//!   on-disk snapshot each tick.
//!
//! Extracted from `runtime.rs` in R271q (Phase γ §R271 seventeenth slice).

use std::path::Path;
use std::sync::{Arc, RwLock};

use serde_json::json;

use yggdrasil_consensus::EpochSchedule;
use yggdrasil_ledger::{LedgerState, SlotNo};
use yggdrasil_network::{
    ConsensusLedgerPeerInputs, ConsensusLedgerPeerSource, LedgerPeerSnapshot, LedgerStateJudgement,
    PeerSnapshotFileObservation, PeerSnapshotFileSource,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::config::load_peer_snapshot_file;
use crate::sync::{recover_ledger_state_chaindb, recover_ledger_state_chaindb_epoch_boundary};
use crate::tracer::{NodeTracer, trace_fields};

use super::block_producer_config::RuntimeBlockProducerConfig;
use super::peer_management::{ledger_peer_snapshot_from_ledger_state, point_slot};

pub(super) struct ChainDbConsensusLedgerSource<'a, I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    pub(super) chain_db: &'a Arc<RwLock<ChainDb<I, V, L>>>,
    pub(super) base_ledger_state: &'a LedgerState,
    pub(super) tracer: &'a NodeTracer,
    /// Seconds since the Unix epoch of `ShelleyGenesis.system_start`.
    /// `None` falls back to the legacy `YoungEnough` behaviour to keep
    /// no-genesis test paths working.
    pub(super) system_start_unix_secs: Option<f64>,
    /// Slot duration in seconds from `ShelleyGenesis.slot_length`.
    /// `None` falls back to the legacy `YoungEnough` behaviour.
    pub(super) slot_length_secs: Option<f64>,
    /// Maximum tolerated tip age in seconds before the judgement flips to
    /// `TooOld`. Upstream uses `stabilityWindow * slotLength` (≈
    /// `3 * k / f * slotLength`).
    pub(super) max_ledger_state_age_secs: f64,
    /// Era-aware epoch schedule for boundary-aware ChainDb recovery.
    pub(super) epoch_schedule: Option<EpochSchedule>,
}

impl<I, V, L> ConsensusLedgerPeerSource for ChainDbConsensusLedgerSource<'_, I, V, L>
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    fn observe(&mut self) -> ConsensusLedgerPeerInputs {
        let chain_db = self.chain_db.read().expect("chain db lock poisoned");
        let tip = chain_db.recovery().tip;
        let recovery_result = match self.epoch_schedule {
            Some(epoch_schedule) => recover_ledger_state_chaindb_epoch_boundary(
                &chain_db,
                self.base_ledger_state.clone(),
                epoch_schedule,
                None,
            ),
            None => recover_ledger_state_chaindb(&chain_db, self.base_ledger_state.clone()),
        };
        match recovery_result {
            Ok(recovery) => {
                let latest_slot = point_slot(&recovery.point).or_else(|| point_slot(&tip));
                let judgement = derive_judgement_for_observe(
                    latest_slot,
                    self.system_start_unix_secs,
                    self.slot_length_secs,
                    self.max_ledger_state_age_secs,
                );
                ConsensusLedgerPeerInputs {
                    latest_slot,
                    judgement,
                    ledger_snapshot: ledger_peer_snapshot_from_ledger_state(&recovery.ledger_state),
                }
            }
            Err(err) => {
                self.tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to recover ledger peers from chain db",
                    trace_fields([("error", json!(err.to_string()))]),
                );
                ConsensusLedgerPeerInputs {
                    latest_slot: point_slot(&tip),
                    judgement: LedgerStateJudgement::Unavailable,
                    ledger_snapshot: LedgerPeerSnapshot::default(),
                }
            }
        }
    }
}

/// Derives a [`LedgerStateJudgement`] for [`ChainDbConsensusLedgerSource::observe`].
///
/// Falls back to `YoungEnough` (the historical pre-slice behaviour) when
/// either of the genesis timing inputs is `None`, so tests and other
/// non-production paths that don't configure genesis aren't disturbed.
/// When both inputs are present, delegates to
/// [`yggdrasil_network::judge_ledger_state_age`] for the upstream-aligned
/// comparison.
pub(super) fn derive_judgement_for_observe(
    tip_slot: Option<u64>,
    system_start_unix_secs: Option<f64>,
    slot_length_secs: Option<f64>,
    max_age_secs: f64,
) -> LedgerStateJudgement {
    derive_judgement_at(
        tip_slot,
        system_start_unix_secs,
        slot_length_secs,
        max_age_secs,
        wall_clock_unix_secs(),
    )
}

/// Pure variant of [`derive_judgement_for_observe`] that takes an explicit
/// `now_unix_secs` for deterministic testing. The production helper above
/// is a thin wrapper that supplies the real wall-clock value.
pub(crate) fn derive_judgement_at(
    tip_slot: Option<u64>,
    system_start_unix_secs: Option<f64>,
    slot_length_secs: Option<f64>,
    max_age_secs: f64,
    now_unix_secs: f64,
) -> LedgerStateJudgement {
    if system_start_unix_secs.is_none() || slot_length_secs.is_none() {
        return LedgerStateJudgement::YoungEnough;
    }
    yggdrasil_network::judge_ledger_state_age(yggdrasil_network::LedgerStateAgeInputs {
        tip_slot,
        system_start_unix_secs,
        slot_length_secs,
        max_age_secs,
        now_unix_secs,
    })
}

pub(super) fn wall_clock_unix_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub(super) fn block_producer_ledger_state_judgement(
    tip_slot: Option<SlotNo>,
    config: &RuntimeBlockProducerConfig,
) -> LedgerStateJudgement {
    match config.max_ledger_state_age_secs {
        Some(max_age_secs) => derive_judgement_at(
            tip_slot.map(|slot| slot.0),
            config.system_start_unix_secs,
            Some(config.slot_length.as_secs_f64()),
            max_age_secs,
            wall_clock_unix_secs(),
        ),
        None => LedgerStateJudgement::YoungEnough,
    }
}

/// Live `peerSnapshotFile` source that re-reads the configured snapshot path
/// each tick.
pub(super) struct FilePeerSnapshotSource<'a> {
    pub(super) path: Option<&'a str>,
    pub(super) tracer: &'a NodeTracer,
}

impl PeerSnapshotFileSource for FilePeerSnapshotSource<'_> {
    fn observe(&mut self) -> PeerSnapshotFileObservation {
        let Some(path) = self.path else {
            return PeerSnapshotFileObservation::not_configured();
        };

        match load_peer_snapshot_file(Path::new(path)) {
            Ok(loaded_snapshot) => {
                PeerSnapshotFileObservation::loaded(loaded_snapshot.slot, loaded_snapshot.snapshot)
            }
            Err(err) => {
                self.tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "failed to refresh configured peer snapshot",
                    trace_fields([
                        ("snapshotPath", json!(path)),
                        ("error", json!(err.to_string())),
                    ]),
                );
                PeerSnapshotFileObservation::unavailable()
            }
        }
    }
}

pub(super) fn refresh_ledger_peer_sources_from_chain_db<I, V, L>(
    registry: &mut yggdrasil_network::PeerRegistry,
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    base_ledger_state: &LedgerState,
    topology: &yggdrasil_network::TopologyConfig,
    tracer: &NodeTracer,
    judgement_settings: super::ledger_judgement::LedgerJudgementSettings,
    epoch_schedule: Option<EpochSchedule>,
) -> yggdrasil_network::LiveLedgerPeerRefreshObservation
where
    I: ImmutableStore,
    V: VolatileStore,
    L: LedgerStore,
{
    if !topology.use_ledger_peers.enabled() {
        return yggdrasil_network::LiveLedgerPeerRefreshObservation {
            update: yggdrasil_network::LedgerPeerRegistryUpdate {
                decision: yggdrasil_network::LedgerPeerUseDecision::Disabled,
                changed: false,
            },
            latest_slot: None,
            judgement: LedgerStateJudgement::Unavailable,
            peer_snapshot_freshness: yggdrasil_network::PeerSnapshotFreshness::NotConfigured,
        };
    }

    let mut consensus_source = ChainDbConsensusLedgerSource {
        chain_db,
        base_ledger_state,
        tracer,
        system_start_unix_secs: judgement_settings.system_start_unix_secs,
        slot_length_secs: judgement_settings.slot_length_secs,
        max_ledger_state_age_secs: judgement_settings.max_ledger_state_age_secs,
        epoch_schedule,
    };
    let mut snapshot_source = FilePeerSnapshotSource {
        path: topology.peer_snapshot_file.as_deref(),
        tracer,
    };

    let observation = yggdrasil_network::live_refresh_ledger_peer_registry_observed(
        registry,
        topology.use_ledger_peers,
        &mut consensus_source,
        &mut snapshot_source,
    );

    if observation.update.changed {
        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "ledger peer registry refreshed",
            trace_fields([(
                "decision",
                json!(format!("{:?}", observation.update.decision)),
            )]),
        );
    }

    observation
}
