#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Consensus-facing types for chain selection, epoch math, and Praos helpers.

/// Chain selection helpers.
pub mod chain_selection;
/// Volatile chain state tracking with rollback depth enforcement.
pub mod chain_state;
/// Diffusion pipelining support (DPvDV): tentative header announcement
/// before body validation completes.
pub mod diffusion_pipelining;
/// Epoch and slot modeling helpers.
pub mod epoch;
mod error;
/// Genesis-density tracking (sliding-window header density per peer).
///
/// Used by the network governor as a chain-quality signal for hot-peer
/// demotion decisions.  Mirrors upstream
/// `Ouroboros.Consensus.Genesis.Governor` density math.
pub mod genesis_density;
/// Block header types and KES-based header signature verification.
pub mod header;
/// Blocks-from-the-future detection (ChainSync InFutureCheck).
pub mod in_future;
/// Epoch nonce evolution state machine (UPDN + TICKN rules).
pub mod nonce;
/// Operational certificate (OpCert) types and verification.
pub mod opcert;
/// Praos-specific threshold and leader-election helpers.
pub mod praos;

/// Chain candidate type and selection helper.
pub use chain_selection::{ChainCandidate, VrfTiebreakerFlavor, select_preferred};
/// Chain state tracking with rollback depth enforcement.
pub use chain_state::{ChainEntry, ChainState, SecurityParam};
/// Diffusion pipelining types and criterion.
pub use diffusion_pipelining::{
    DiffusionPipeliningSupport, HotIdentity, PeerPipeliningState, PipeliningEvent, TentativeHeader,
    TentativeHeaderState, TentativeHeaderView, TentativeState,
};
/// Epoch size and slot-to-epoch helpers.
pub use epoch::{EpochSchedule, EpochSize, epoch_first_slot, is_new_epoch, slot_to_epoch};
/// Consensus-facing error type.
pub use error::ConsensusError;
/// Genesis density window plus default-window and default-low-density-threshold constants.
pub use genesis_density::{DEFAULT_LOW_DENSITY_THRESHOLD, DEFAULT_SLOT_WINDOW, DensityWindow};
/// Block header types and verification entry point.
pub use header::{
    Header, HeaderBody, check_header_protocol_version, verify_header,
    verify_header_with_signed_bytes, verify_opcert_only,
};
/// Blocks-from-the-future detection types.
pub use in_future::{ClockSkew, FutureSlotJudgement, judge_header_slot};
/// Epoch nonce evolution state machine and helpers.
pub use nonce::{
    NonceDerivation, NonceEvolutionConfig, NonceEvolutionState, derive_vrf_nonce,
    praos_vrf_output_to_nonce, vrf_output_to_nonce,
};
/// Operational certificate type and helpers.
pub use opcert::{OcertCounters, OpCert, check_kes_period, kes_period_of_slot};
/// Active slot coefficient wrapper, threshold, and leader check helpers.
pub use praos::{
    ActiveSlotCoeff, VrfMode, VrfUsage, check_is_leader, check_leader_value, leadership_threshold,
    praos_vrf_input, tpraos_vrf_seed, verify_leader_proof, verify_nonce_proof, vrf_input,
};
