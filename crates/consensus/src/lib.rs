//! Consensus-facing types for chain selection, epoch math, and Praos helpers.

/// Chain selection helpers.
pub mod chain_selection;
/// Volatile chain state tracking with rollback depth enforcement.
pub mod chain_state;
/// Epoch and slot modeling helpers.
pub mod epoch;
mod error;
/// Block header types and KES-based header signature verification.
pub mod header;
/// Epoch nonce evolution state machine (UPDN + TICKN rules).
pub mod nonce;
/// Operational certificate (OpCert) types and verification.
pub mod opcert;
/// Praos-specific threshold and leader-election helpers.
pub mod praos;

/// Chain candidate type and selection helper.
pub use chain_selection::{ChainCandidate, select_preferred};
/// Chain state tracking with rollback depth enforcement.
pub use chain_state::{ChainEntry, ChainState, SecurityParam};
/// Epoch size and slot-to-epoch helpers.
pub use epoch::{EpochSize, epoch_first_slot, is_new_epoch, slot_to_epoch};
/// Consensus-facing error type.
pub use error::ConsensusError;
/// Block header types and verification entry point.
pub use header::{Header, HeaderBody, verify_header, verify_opcert_only};
/// Epoch nonce evolution state machine and helpers.
pub use nonce::{NonceEvolutionConfig, NonceEvolutionState, vrf_output_to_nonce};
/// Operational certificate type and helpers.
pub use opcert::{OpCert, check_kes_period, kes_period_of_slot};
/// Active slot coefficient wrapper, threshold, and leader check helpers.
pub use praos::{
    ActiveSlotCoeff, check_is_leader, check_leader_value, leadership_threshold, verify_leader_proof,
    vrf_input,
};
