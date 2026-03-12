//! Consensus-facing types for chain selection, epoch math, and Praos helpers.

/// Chain selection helpers.
pub mod chain_selection;
/// Epoch and slot modeling helpers.
pub mod epoch;
mod error;
/// Praos-specific threshold and leader-election helpers.
pub mod praos;

/// Chain candidate type and selection helper.
pub use chain_selection::{ChainCandidate, select_preferred};
/// Epoch size and slot-to-epoch helpers.
pub use epoch::{EpochSize, epoch_first_slot, is_new_epoch, slot_to_epoch};
/// Consensus-facing error type.
pub use error::ConsensusError;
/// Active slot coefficient wrapper, threshold, and leader check helpers.
pub use praos::{
    ActiveSlotCoeff, check_is_leader, check_leader_value, leadership_threshold, verify_leader_proof,
    vrf_input,
};
