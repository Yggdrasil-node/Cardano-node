//! Epoch nonce evolution state machine.
//!
//! Tracks the evolving and candidate nonces per block and computes the
//! new epoch nonce at epoch boundaries.  This implements the combined
//! UPDN (Update Nonce) and TICKN rules from `cardano-protocol-tpraos`.
//!
//! ## Per-block update (UPDN rule)
//!
//! For each block with VRF nonce contribution `η`:
//! - `evolving_nonce  ← evolving_nonce ⭒ η`
//! - If the block's slot is **not** in the stability window (the last
//!   `stability_window` slots of the epoch): `candidate_nonce ← evolving_nonce'`
//! - Otherwise: `candidate_nonce` is **frozen** (unchanged).
//!
//! ## Epoch transition (TICKN rule)
//!
//! At the first block of a new epoch:
//! - `epoch_nonce ← candidate_nonce ⭒ prev_hash_nonce ⭒ extra_entropy`
//! - `prev_hash_nonce ← lab_nonce`
//!
//! The `lab_nonce` is updated per block to `from_header_hash(prev_hash)`.
//!
//! ## Nonce derivation from VRF output
//!
//! A VRF output (64 bytes) is converted to a `Nonce` by hashing it with
//! Blake2b-256, matching upstream `hashVerifiedVRF`.
//!
//! Reference: `Cardano.Protocol.TPraos.Rules.Updn` (UPDN rule),
//! `Cardano.Protocol.TPraos.Rules.Tickn` (TICKN rule),
//! `Cardano.Protocol.TPraos.API` (`tickChainDepState`, `updateChainDepState`).

use yggdrasil_crypto::hash_bytes_256;
use yggdrasil_ledger::{EpochNo, HeaderHash, Nonce, SlotNo};

use crate::epoch::{EpochSize, epoch_first_slot, slot_to_epoch};

/// Converts a VRF output (raw bytes) to a `Nonce` by hashing with Blake2b-256.
///
/// Reference: `hashVerifiedVRF` in `Cardano.Ledger.BaseTypes`.
pub fn vrf_output_to_nonce(output: &[u8]) -> Nonce {
    let hash = hash_bytes_256(output);
    Nonce::Hash(hash.0)
}

/// Configuration parameters governing nonce evolution and stability window.
///
/// These values are fixed per network and era (they come from the genesis
/// configuration).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NonceEvolutionConfig {
    /// Number of slots per epoch (mainnet: 432000).
    pub epoch_size: EpochSize,
    /// Stability window in slots: `3k/f` where `k` = `SecurityParam` and
    /// `f` = `ActiveSlotCoeff` (mainnet: 129600).
    pub stability_window: u64,
    /// Extra entropy injected at epoch transitions (protocol parameter;
    /// `NeutralNonce` from Babbage onward).
    pub extra_entropy: Nonce,
}

/// State machine tracking epoch nonce evolution.
///
/// This tracks the five nonce components described in `ChainDepState` and
/// `PrtclState` from the upstream Haskell implementation:
///
/// - `evolving_nonce` (η_v): XOR of all VRF nonce outputs seen so far in the
///   epoch; always updated per block.
/// - `candidate_nonce` (η_c): Same as `evolving_nonce` until the stability
///   window, then frozen.
/// - `epoch_nonce`: The nonce used for VRF verification in the current epoch.
/// - `prev_hash_nonce`: Hash of the last block from the previous epoch, used
///   in the next epoch transition.
/// - `lab_nonce`: "Last applied block" nonce — `prevHashToNonce` from the most
///   recently processed block.
///
/// Reference: `ChainDepState` = `PrtclState` + `TicknState` + `csLabNonce`
/// in `Cardano.Protocol.TPraos.API`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonceEvolutionState {
    /// Evolving nonce (η_v) — XOR'd with every block's VRF nonce output.
    pub evolving_nonce: Nonce,
    /// Candidate nonce (η_c) — tracks evolving nonce until stability window,
    /// then freezes.
    pub candidate_nonce: Nonce,
    /// Current epoch nonce used for VRF verification.
    pub epoch_nonce: Nonce,
    /// Hash of the last block from the previous epoch, XOR'd into the epoch
    /// nonce at the next epoch transition.
    pub prev_hash_nonce: Nonce,
    /// Nonce from the `prev_hash` field of the last applied block header.
    pub lab_nonce: Nonce,
    /// Current epoch number (used to detect epoch transitions).
    pub current_epoch: EpochNo,
}

impl NonceEvolutionState {
    /// Creates a new state from an initial epoch nonce.
    ///
    /// Reference: `initialChainDepState` in `Cardano.Protocol.TPraos.API`.
    pub fn new(initial_nonce: Nonce) -> Self {
        Self {
            evolving_nonce: initial_nonce,
            candidate_nonce: initial_nonce,
            epoch_nonce: initial_nonce,
            prev_hash_nonce: Nonce::Neutral,
            lab_nonce: Nonce::Neutral,
            current_epoch: EpochNo(0),
        }
    }

    /// Creates a new state starting from a known epoch with a given epoch nonce.
    ///
    /// Useful for resuming from a snapshot or starting from a non-genesis state.
    pub fn from_epoch(epoch: EpochNo, epoch_nonce: Nonce) -> Self {
        Self {
            evolving_nonce: epoch_nonce,
            candidate_nonce: epoch_nonce,
            epoch_nonce,
            prev_hash_nonce: Nonce::Neutral,
            lab_nonce: Nonce::Neutral,
            current_epoch: epoch,
        }
    }

    /// Processes a block, updating nonce state.
    ///
    /// This implements the UPDN rule (per-block nonce update) and the TICKN
    /// rule (epoch transition) in a single call.
    ///
    /// ## Parameters
    ///
    /// - `slot`: The block's slot number.
    /// - `vrf_nonce_output`: The VRF nonce contribution from this block.
    ///   For TPraos (Shelley–Alonzo) this is the `nonce_vrf` output; for
    ///   Praos (Babbage/Conway) this is the single `vrf_result` output.
    ///   Pass the raw VRF output bytes (64 bytes typically); they will be
    ///   hashed to produce a 32-byte nonce.
    /// - `prev_hash`: The `prev_hash` from the block header (`None` for
    ///   genesis successor blocks).
    /// - `config`: Network and era parameters.
    pub fn apply_block(
        &mut self,
        slot: SlotNo,
        vrf_nonce_output: &[u8],
        prev_hash: Option<HeaderHash>,
        config: &NonceEvolutionConfig,
    ) {
        let block_epoch = slot_to_epoch(slot, config.epoch_size);

        // Detect epoch transition and apply TICKN rule.
        if block_epoch > self.current_epoch {
            self.tick_epoch_transition(config);
            self.current_epoch = block_epoch;
        }

        // Derive nonce from VRF output: η = Blake2b-256(vrf_output)
        let eta = vrf_output_to_nonce(vrf_nonce_output);

        // UPDN rule:
        // evolving_nonce' = evolving_nonce ⭒ η
        self.evolving_nonce = self.evolving_nonce.combine(eta);

        // candidate_nonce update: freeze in stability window.
        let next_epoch = EpochNo(block_epoch.0 + 1);
        let first_slot_next = epoch_first_slot(next_epoch, config.epoch_size);
        let in_stability_window = slot.0 + config.stability_window >= first_slot_next.0;

        if !in_stability_window {
            // Not in stability window — update candidate to match evolving.
            self.candidate_nonce = self.evolving_nonce;
        }
        // else: candidate nonce stays frozen.

        // Update lab_nonce (prevHashToNonce of this block).
        self.lab_nonce = match prev_hash {
            Some(h) => Nonce::from_header_hash(h),
            None => Nonce::Neutral,
        };
    }

    /// Applies the TICKN epoch transition rule.
    ///
    /// This is called when we detect that we've entered a new epoch.
    ///
    /// ```text
    /// epoch_nonce' = candidate_nonce ⭒ prev_hash_nonce ⭒ extra_entropy
    /// prev_hash_nonce' = lab_nonce
    /// ```
    ///
    /// Reference: `tickTransition` in `Cardano.Protocol.TPraos.Rules.Tickn`.
    fn tick_epoch_transition(&mut self, config: &NonceEvolutionConfig) {
        // New epoch nonce = candidate ⭒ prev_hash ⭒ extra_entropy
        self.epoch_nonce = self
            .candidate_nonce
            .combine(self.prev_hash_nonce)
            .combine(config.extra_entropy);

        // The lab_nonce (hash of last applied block) becomes the new
        // prev_hash_nonce for the next epoch transition.
        self.prev_hash_nonce = self.lab_nonce;
    }
}
