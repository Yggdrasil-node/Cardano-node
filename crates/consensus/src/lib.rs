//! Consensus-facing types for chain selection, epoch math, and Praos helpers.

/// Chain selection helpers.
pub mod chain_selection;
/// Epoch and slot modeling helpers.
pub mod epoch;
mod error;
/// Praos-specific threshold helpers.
pub mod praos;

/// Chain candidate type and selection helper.
pub use chain_selection::{ChainCandidate, select_preferred};
/// Epoch and slot wrappers plus conversion helper.
pub use epoch::{Epoch, Slot, slot_to_epoch};
/// Consensus-facing error type.
pub use error::ConsensusError;
/// Active slot coefficient wrapper and threshold helper.
pub use praos::{ActiveSlotCoeff, leadership_threshold};
