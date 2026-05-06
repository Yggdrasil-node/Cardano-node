//! Block-producer runtime configuration + shared state.
//!
//! Mirrors upstream:
//! - `Ouroboros.Consensus.Node.Forking.forkBlockForging` runtime config
//!   parameters (slot length, system start, max ledger age, active slot
//!   coefficient, KES expiry warning thresholds, max block body size,
//!   protocol version)
//! - `Ouroboros.Consensus.Node.Forking.SharedKernelState` per-slot live
//!   inputs (epoch nonce + per-pool relative stake sigma)
//!
//! `SharedBlockProducerState` is updated by the sync pipeline after each
//! batch applies nonce evolution and stake-snapshot rotation, so the
//! concurrent block producer loop reads live values without polling the
//! sync side. `RuntimeBlockProducerConfig` is built once at startup
//! from the node configuration and stays immutable across the run.
//!
//! Extracted from `runtime.rs` in R271b.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use yggdrasil_consensus::NonceEvolutionState;
use yggdrasil_consensus::praos::ActiveSlotCoeff;
use yggdrasil_ledger::Nonce;

/// Shared block-producer state updated by the sync pipeline so the producer
/// loop reads live epoch nonce and stake sigma values across block forging.
///
/// Reference: upstream `forkBlockForging` in `NodeKernel.hs` re-reads the
/// ledger view's epoch nonce and per-pool relative stake each slot.
#[derive(Clone, Debug, Default)]
pub struct SharedBlockProducerState {
    /// Current epoch nonce available to the block producer.
    pub epoch_nonce: Option<Nonce>,
    /// Current delegated stake sigma (numerator / denominator) available to the block producer.
    pub sigma: Option<(u64, u64)>,
}

/// Update the shared block-producer state with the latest epoch nonce from
/// the nonce evolution state machine.
///
/// Called after each sync batch applies nonce evolution, so the concurrent
/// block producer loop observes the live nonce without polling the sync
/// pipeline.
///
/// Reference: upstream `forkBlockForging` reads `currentSlot`'s ledger view
/// epoch nonce on every slot tick.
pub fn update_bp_state_nonce(
    bp_state: &Option<Arc<RwLock<SharedBlockProducerState>>>,
    nonce_state: Option<&NonceEvolutionState>,
) {
    if let (Some(bp), Some(ns)) = (bp_state.as_ref(), nonce_state) {
        if let Ok(mut st) = bp.write() {
            st.epoch_nonce = Some(ns.epoch_nonce);
        }
    }
}

/// Update the shared block-producer state with the pool's relative stake
/// from the active (set) stake snapshot.
///
/// `pool_key_hash` is the Blake2b-224 hash of the block producer's cold
/// verification key (`issuer_vkey`).
///
/// The `set` snapshot is the one active for leader election in the current
/// epoch (upstream: `esNesPd . nesEs`).
///
/// Reference: upstream `forkBlockForging` reads `IndividualPoolStake` from
/// the epoch's stake distribution on every slot tick.
pub fn update_bp_state_sigma(
    bp_state: &Option<Arc<RwLock<SharedBlockProducerState>>>,
    stake_snapshots: Option<&yggdrasil_ledger::StakeSnapshots>,
    pool_key_hash: &[u8; 28],
) {
    if let (Some(bp), Some(snapshots)) = (bp_state.as_ref(), stake_snapshots) {
        let dist = snapshots.set.pool_stake_distribution();
        let sigma = dist.relative_stake(pool_key_hash);
        if let Ok(mut st) = bp.write() {
            st.sigma = Some(sigma);
        }
    }
}

/// Runtime block-producer configuration derived from node configuration.
#[derive(Clone, Debug)]
pub struct RuntimeBlockProducerConfig {
    /// Slot duration used by the local slot clock.
    pub slot_length: Duration,
    /// Seconds since Unix epoch of `ShelleyGenesis.system_start`.
    ///
    /// Block production must use absolute network slots derived from this
    /// value; relative process-start clocks are valid only in unit tests.
    pub system_start_unix_secs: Option<f64>,
    /// Maximum tolerated age of the current ledger tip before forging is
    /// suppressed.
    pub max_ledger_state_age_secs: Option<f64>,
    /// Active slot coefficient `f` used for Praos leader checks.
    pub active_slot_coeff: ActiveSlotCoeff,
    /// Relative stake numerator for the forging key (sigma numerator).
    pub sigma_num: u64,
    /// Relative stake denominator for the forging key (sigma denominator).
    pub sigma_den: u64,
    /// Epoch nonce used for leader checks.
    pub epoch_nonce: Nonce,
    /// Maximum aggregate block-body size in bytes.
    pub max_block_body_size: u32,
    /// Protocol version inserted into forged headers.
    pub protocol_version: (u64, u64),
}
