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

/// Selects the VRF-output-to-nonce derivation for the current era.
///
/// TPraos (Shelley–Alonzo) and Praos (Babbage/Conway) use different
/// hashing schemes to convert a VRF output into a nonce contribution.
///
/// Reference: `hashVerifiedVRF` (TPraos), `vrfNonceValue` (Praos) in
/// `Ouroboros.Consensus.Protocol.Praos.VRF`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NonceDerivation {
    /// TPraos (Shelley–Alonzo): `Blake2b-256(output)`.
    ///
    /// Reference: `hashVerifiedVRF` in `Cardano.Ledger.BaseTypes`.
    TPraos,
    /// Praos (Babbage/Conway): `Blake2b-256(Blake2b-256("N" || output))`.
    ///
    /// The VRF output is first range-extended via `hashVRF SVRFNonce`
    /// (which prepends `"N"` and hashes), then the resulting 32-byte
    /// hash is hashed again to produce the nonce.
    ///
    /// Reference: `vrfNonceValue` in
    /// `Ouroboros.Consensus.Protocol.Praos.VRF`.
    Praos,
}

/// Converts a VRF output (raw bytes) to a `Nonce` using TPraos derivation.
///
/// This is `Blake2b-256(output)`, matching upstream `hashVerifiedVRF`.
///
/// For Praos-era (Babbage/Conway) blocks, use [`praos_vrf_output_to_nonce`]
/// instead.
///
/// Reference: `hashVerifiedVRF` in `Cardano.Ledger.BaseTypes`.
pub fn vrf_output_to_nonce(output: &[u8]) -> Nonce {
    let hash = hash_bytes_256(output);
    Nonce::Hash(hash.0)
}

/// Converts a VRF output (raw bytes) to a `Nonce` using Praos derivation.
///
/// This is `Blake2b-256(Blake2b-256("N" || output))`, matching upstream
/// `vrfNonceValue` from `Ouroboros.Consensus.Protocol.Praos.VRF`.
///
/// The double hash is intentional: the first hash (`"N" || output`) is
/// the VRF range-extension step, and the second hash converts the
/// crypto-dependent hash into a fixed `Blake2b_256` nonce.
///
/// Reference: `vrfNonceValue`, `hashVRF SVRFNonce` in
/// `Ouroboros.Consensus.Protocol.Praos.VRF`.
pub fn praos_vrf_output_to_nonce(output: &[u8]) -> Nonce {
    // Step 1: hashVRF SVRFNonce = Blake2b-256("N" || output)
    let mut prefixed = Vec::with_capacity(1 + output.len());
    prefixed.push(b'N');
    prefixed.extend_from_slice(output);
    let inner_hash = hash_bytes_256(&prefixed);
    // Step 2: hashWith id (hashToBytes inner_hash) = Blake2b-256(inner_hash_bytes)
    let outer_hash = hash_bytes_256(&inner_hash.0);
    Nonce::Hash(outer_hash.0)
}

/// Derives a nonce from a VRF output using the given era-specific derivation.
pub fn derive_vrf_nonce(output: &[u8], derivation: NonceDerivation) -> Nonce {
    match derivation {
        NonceDerivation::TPraos => vrf_output_to_nonce(output),
        NonceDerivation::Praos => praos_vrf_output_to_nonce(output),
    }
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
    ///   hashed to produce a 32-byte nonce using the era-appropriate
    ///   derivation.
    /// - `prev_hash`: The `prev_hash` from the block header (`None` for
    ///   genesis successor blocks).
    /// - `config`: Network and era parameters.
    /// - `derivation`: Era-specific VRF-to-nonce hashing scheme.
    pub fn apply_block(
        &mut self,
        slot: SlotNo,
        vrf_nonce_output: &[u8],
        prev_hash: Option<HeaderHash>,
        config: &NonceEvolutionConfig,
        derivation: NonceDerivation,
    ) {
        let block_epoch = slot_to_epoch(slot, config.epoch_size);

        // Detect epoch transition and apply TICKN rule.
        if block_epoch > self.current_epoch {
            self.tick_epoch_transition(config);
            self.current_epoch = block_epoch;
        }

        // Derive nonce from VRF output using era-appropriate derivation.
        let eta = derive_vrf_nonce(vrf_nonce_output, derivation);

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

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::HeaderHash;

    fn cfg() -> NonceEvolutionConfig {
        NonceEvolutionConfig {
            epoch_size: EpochSize(100),
            stability_window: 30, // last 30 slots of each epoch
            extra_entropy: Nonce::Neutral,
        }
    }

    // ── vrf_output_to_nonce ──────────────────────────────────────────

    #[test]
    fn vrf_output_to_nonce_is_deterministic() {
        let output = [0xAB; 64];
        let n1 = vrf_output_to_nonce(&output);
        let n2 = vrf_output_to_nonce(&output);
        assert_eq!(n1, n2);
    }

    #[test]
    fn vrf_output_to_nonce_is_hash_variant() {
        let n = vrf_output_to_nonce(&[0x00; 64]);
        assert!(matches!(n, Nonce::Hash(_)));
    }

    #[test]
    fn vrf_output_to_nonce_different_inputs_differ() {
        let n1 = vrf_output_to_nonce(&[0x00; 64]);
        let n2 = vrf_output_to_nonce(&[0xFF; 64]);
        assert_ne!(n1, n2);
    }

    // ── NonceEvolutionState::new ─────────────────────────────────────

    #[test]
    fn new_state_initial_values() {
        let nonce = Nonce::Hash([0xAA; 32]);
        let state = NonceEvolutionState::new(nonce);
        assert_eq!(state.evolving_nonce, nonce);
        assert_eq!(state.candidate_nonce, nonce);
        assert_eq!(state.epoch_nonce, nonce);
        assert_eq!(state.prev_hash_nonce, Nonce::Neutral);
        assert_eq!(state.lab_nonce, Nonce::Neutral);
        assert_eq!(state.current_epoch, EpochNo(0));
    }

    #[test]
    fn from_epoch_state() {
        let nonce = Nonce::Hash([0xBB; 32]);
        let state = NonceEvolutionState::from_epoch(EpochNo(5), nonce);
        assert_eq!(state.current_epoch, EpochNo(5));
        assert_eq!(state.epoch_nonce, nonce);
        assert_eq!(state.evolving_nonce, nonce);
        assert_eq!(state.candidate_nonce, nonce);
    }

    // ── apply_block (within epoch) ───────────────────────────────────

    #[test]
    fn apply_block_updates_evolving_nonce() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        let vrf_out = [0x42; 64];
        state.apply_block(SlotNo(5), &vrf_out, None, &c, d);
        // evolving_nonce should have changed from Neutral
        assert_ne!(state.evolving_nonce, Nonce::Neutral);
    }

    #[test]
    fn apply_block_before_stability_window_updates_candidate() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        // slot 5 is well before the stability window (slots 70-99 of epoch 0)
        state.apply_block(SlotNo(5), &[0x42; 64], None, &c, d);
        // candidate should track evolving
        assert_eq!(state.candidate_nonce, state.evolving_nonce);
    }

    #[test]
    fn apply_block_in_stability_window_freezes_candidate() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        // First, apply a block outside stability window
        state.apply_block(SlotNo(5), &[0x42; 64], None, &c, d);
        let candidate_before = state.candidate_nonce;
        // Slot 75 is in the stability window (75 + 30 >= 100)
        state.apply_block(SlotNo(75), &[0xFF; 64], None, &c, d);
        // candidate should NOT have changed
        assert_eq!(state.candidate_nonce, candidate_before);
        // But evolving should have changed
        assert_ne!(state.evolving_nonce, candidate_before);
    }

    #[test]
    fn apply_block_updates_lab_nonce() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        let prev_hash = HeaderHash([0xDD; 32]);
        state.apply_block(SlotNo(5), &[0x42; 64], Some(prev_hash), &c, d);
        assert_eq!(state.lab_nonce, Nonce::from_header_hash(prev_hash));
    }

    #[test]
    fn apply_block_none_prev_hash_sets_neutral_lab() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        state.apply_block(SlotNo(5), &[0x42; 64], None, &c, d);
        assert_eq!(state.lab_nonce, Nonce::Neutral);
    }

    // ── epoch transition (TICKN) ─────────────────────────────────────

    #[test]
    fn epoch_transition_updates_epoch_nonce() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Hash([0xAA; 32]));
        // Apply a block in epoch 0
        let prev_hash = HeaderHash([0xDD; 32]);
        state.apply_block(SlotNo(10), &[0x42; 64], Some(prev_hash), &c, d);
        let epoch_nonce_before = state.epoch_nonce;
        // Apply a block in epoch 1 (triggers TICKN transition)
        state.apply_block(
            SlotNo(105),
            &[0xFF; 64],
            Some(HeaderHash([0xEE; 32])),
            &c,
            d,
        );
        assert_eq!(state.current_epoch, EpochNo(1));
        // epoch_nonce should have changed
        assert_ne!(state.epoch_nonce, epoch_nonce_before);
    }

    #[test]
    fn epoch_transition_prev_hash_nonce_becomes_lab() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Hash([0xAA; 32]));
        let prev_hash = HeaderHash([0xDD; 32]);
        state.apply_block(SlotNo(10), &[0x42; 64], Some(prev_hash), &c, d);
        let lab_before_transition = state.lab_nonce;
        // Transition to epoch 1
        state.apply_block(SlotNo(100), &[0xFF; 64], None, &c, d);
        // prev_hash_nonce should be what lab_nonce was before the transition
        assert_eq!(state.prev_hash_nonce, lab_before_transition);
    }

    #[test]
    fn epoch_transition_continues_evolving_nonce() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        // Apply block in epoch 0 outside stability window
        state.apply_block(SlotNo(10), &[0x01; 64], None, &c, d);
        // Transition to epoch 1
        state.apply_block(SlotNo(100), &[0x02; 64], None, &c, d);
        // The epoch_nonce is now candidate ⭒ prev_hash_nonce ⭒ extra_entropy.
        // The evolving nonce continues accumulation in the new epoch,
        // matching upstream Praos `tickChainDepState` which does NOT
        // reset evolving/candidate to epoch_nonce.
        assert_eq!(state.current_epoch, EpochNo(1));
    }

    // ── extra entropy ────────────────────────────────────────────────

    #[test]
    fn extra_entropy_affects_epoch_nonce() {
        let mut c1 = cfg();
        c1.extra_entropy = Nonce::Neutral;
        let mut c2 = cfg();
        c2.extra_entropy = Nonce::Hash([0xFF; 32]);

        let mut s1 = NonceEvolutionState::new(Nonce::Hash([0xAA; 32]));
        let mut s2 = NonceEvolutionState::new(Nonce::Hash([0xAA; 32]));

        let d = NonceDerivation::TPraos;
        // Same block in epoch 0
        s1.apply_block(SlotNo(10), &[0x42; 64], None, &c1, d);
        s2.apply_block(SlotNo(10), &[0x42; 64], None, &c2, d);

        // Transition to epoch 1
        s1.apply_block(SlotNo(100), &[0x01; 64], None, &c1, d);
        s2.apply_block(SlotNo(100), &[0x01; 64], None, &c2, d);

        // epoch_nonce should differ due to extra_entropy
        assert_ne!(s1.epoch_nonce, s2.epoch_nonce);
    }

    // ── stability window boundary ────────────────────────────────────

    #[test]
    fn stability_window_boundary_slot() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        // stability_window=30, epoch_size=100
        // Epoch 0 range: slots 0..99
        // Stability window starts at slot 70 (100 - 30 = 70)
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        // Slot 69: just outside stability window
        state.apply_block(SlotNo(69), &[0x01; 64], None, &c, d);
        let candidate_after_69 = state.candidate_nonce;
        // candidate should have been updated
        assert_eq!(candidate_after_69, state.evolving_nonce);

        // Slot 70: first slot IN stability window (70 + 30 >= 100)
        state.apply_block(SlotNo(70), &[0x02; 64], None, &c, d);
        // candidate should be FROZEN
        assert_eq!(state.candidate_nonce, candidate_after_69);
        assert_ne!(state.evolving_nonce, candidate_after_69);
    }

    // ── multi-epoch ──────────────────────────────────────────────────

    #[test]
    fn multi_epoch_transitions() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Hash([0xAA; 32]));
        // One block per epoch for 5 epochs
        for epoch in 0..5u64 {
            let slot = epoch * 100 + 10;
            state.apply_block(
                SlotNo(slot),
                &[epoch as u8; 64],
                Some(HeaderHash([epoch as u8; 32])),
                &c,
                d,
            );
        }
        assert_eq!(state.current_epoch, EpochNo(4));
        // Each transition should have updated the epoch_nonce
        // Just verify the state is consistent / doesn't panic
        assert!(matches!(state.epoch_nonce, Nonce::Hash(_)));
    }

    #[test]
    fn multiple_blocks_within_epoch() {
        let c = cfg();
        let d = NonceDerivation::TPraos;
        let mut state = NonceEvolutionState::new(Nonce::Neutral);
        // Many blocks in epoch 0
        for slot in 0..20u64 {
            state.apply_block(SlotNo(slot), &[slot as u8; 64], None, &c, d);
        }
        assert_eq!(state.current_epoch, EpochNo(0));
        // Evolving should accumulate all contributions
        assert!(matches!(state.evolving_nonce, Nonce::Hash(_)));
    }

    // ── Praos VRF nonce derivation (Babbage/Conway) ──────────────────

    #[test]
    fn praos_vrf_output_to_nonce_differs_from_tpraos() {
        let output = [0x42; 64];
        let tpraos_nonce = vrf_output_to_nonce(&output);
        let praos_nonce = praos_vrf_output_to_nonce(&output);
        // The two derivations must produce different nonces for the same input,
        // because Praos prepends "N" and double-hashes.
        assert_ne!(tpraos_nonce, praos_nonce);
    }

    #[test]
    fn praos_vrf_output_to_nonce_is_deterministic() {
        let output = [0xAB; 64];
        let n1 = praos_vrf_output_to_nonce(&output);
        let n2 = praos_vrf_output_to_nonce(&output);
        assert_eq!(n1, n2);
    }

    #[test]
    fn praos_vrf_output_to_nonce_is_hash_variant() {
        let n = praos_vrf_output_to_nonce(&[0x00; 64]);
        assert!(matches!(n, Nonce::Hash(_)));
    }

    #[test]
    fn praos_vrf_output_to_nonce_different_inputs_differ() {
        let n1 = praos_vrf_output_to_nonce(&[0x00; 64]);
        let n2 = praos_vrf_output_to_nonce(&[0xFF; 64]);
        assert_ne!(n1, n2);
    }

    #[test]
    fn derive_vrf_nonce_dispatches_correctly() {
        let output = [0x42; 64];
        assert_eq!(
            derive_vrf_nonce(&output, NonceDerivation::TPraos),
            vrf_output_to_nonce(&output)
        );
        assert_eq!(
            derive_vrf_nonce(&output, NonceDerivation::Praos),
            praos_vrf_output_to_nonce(&output)
        );
    }

    #[test]
    fn apply_block_praos_derivation_differs() {
        let c = cfg();
        let mut s1 = NonceEvolutionState::new(Nonce::Neutral);
        let mut s2 = NonceEvolutionState::new(Nonce::Neutral);
        let vrf_out = [0x42; 64];
        s1.apply_block(SlotNo(5), &vrf_out, None, &c, NonceDerivation::TPraos);
        s2.apply_block(SlotNo(5), &vrf_out, None, &c, NonceDerivation::Praos);
        // evolving nonces should differ because derivation functions differ
        assert_ne!(s1.evolving_nonce, s2.evolving_nonce);
    }
}
