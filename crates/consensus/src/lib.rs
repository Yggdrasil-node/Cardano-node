//! Consensus-facing types for chain selection, epoch math, and Praos helpers.

/// Chain selection helpers.
pub mod chain_selection;
/// Epoch and slot modeling helpers.
pub mod epoch;
mod error;
/// Block header types and KES-based header signature verification.
pub mod header;
/// Operational certificate (OpCert) types and verification.
pub mod opcert;
/// Praos-specific threshold and leader-election helpers.
pub mod praos;

/// Chain candidate type and selection helper.
pub use chain_selection::{ChainCandidate, select_preferred};
/// Epoch size and slot-to-epoch helpers.
pub use epoch::{EpochSize, epoch_first_slot, is_new_epoch, slot_to_epoch};
/// Consensus-facing error type.
pub use error::ConsensusError;
/// Block header types and verification entry point.
pub use header::{Header, HeaderBody, verify_header, verify_opcert_only};
/// Operational certificate type and helpers.
pub use opcert::{OpCert, check_kes_period, kes_period_of_slot};
/// Active slot coefficient wrapper, threshold, and leader check helpers.
pub use praos::{
    ActiveSlotCoeff, check_is_leader, check_leader_value, leadership_threshold, verify_leader_proof,
    vrf_input,
};
