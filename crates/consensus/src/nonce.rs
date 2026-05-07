//! Epoch nonce evolution state machine + VRF-output-to-nonce derivation.
//!
//! Tracks the evolving and candidate nonces per block and computes the
//! new epoch nonce at epoch boundaries. Implements the combined
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

pub mod derivation;
pub mod evolution;

pub use derivation::{
    NonceDerivation, derive_vrf_nonce, praos_vrf_output_to_nonce, vrf_output_to_nonce,
};
pub use evolution::{NonceEvolutionConfig, NonceEvolutionState};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::EpochSize;
    use yggdrasil_ledger::{EpochNo, HeaderHash, Nonce, SlotNo};

    fn cfg() -> NonceEvolutionConfig {
        NonceEvolutionConfig {
            epoch_size: EpochSize(100),
            stability_window: 30, // last 30 slots of each epoch
            extra_entropy: Nonce::Neutral,
            byron_shelley_transition: None,
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

    /// R263 regression pin: with `byron_shelley_transition = Some((86400, 4))`
    /// (preprod), a block at slot 432000 (offset 345600 from Shelley
    /// boundary) MUST be classified as Shelley epoch 4 — NOT EpochNo(1)
    /// from fixed-length math. The bug this guards: pre-R263 `apply_block`
    /// used `slot_to_epoch(slot, epoch_size)` which for preprod would
    /// fire `tick_epoch_transition` at slot 432000 (= 1 * epoch_size),
    /// rotating the active `epoch_nonce` to a wrong value and producing
    /// `InvalidVrfProof` on the next active-overlay block.
    ///
    /// Reference: `docs/operational-runs/2026-05-06-round-263-r253-fix-byron-aware-nonce.md`.
    #[test]
    fn preprod_byron_shelley_transition_no_spurious_epoch_tick_at_slot_432000() {
        let c = NonceEvolutionConfig {
            epoch_size: EpochSize(432_000),
            stability_window: 129_600,
            extra_entropy: Nonce::Neutral,
            byron_shelley_transition: Some((86_400, 4)), // preprod
        };
        // slot 432000 → preprod Shelley epoch 4 (offset 345600,
        // post=345600/432000=0, epoch=4+0=4).
        assert_eq!(c.slot_to_epoch(SlotNo(432_000)), EpochNo(4));
        // slot 86400 (Byron→Shelley boundary) → Shelley epoch 4.
        assert_eq!(c.slot_to_epoch(SlotNo(86_400)), EpochNo(4));
        // slot 518400 (= 86400 + 432000) → epoch 5.
        assert_eq!(c.slot_to_epoch(SlotNo(518_400)), EpochNo(5));
        // Pre-Shelley slot 0 → conceptual Byron epoch 0 (no tick).
        assert_eq!(c.slot_to_epoch(SlotNo(0)), EpochNo(0));

        // epoch_first_slot inverse for the same boundaries.
        assert_eq!(c.epoch_first_slot(EpochNo(4)), SlotNo(86_400));
        assert_eq!(c.epoch_first_slot(EpochNo(5)), SlotNo(518_400));

        // End-to-end: apply blocks across the prior failure window
        // and confirm `tick_epoch_transition` does NOT fire at slot
        // 432000. The `current_epoch` should remain 4 after the slot
        // 432000 block, and `epoch_nonce` should be unchanged from
        // the value carried into Shelley.
        let mut state = NonceEvolutionState::new(Nonce::Hash([0x16; 32]));
        state.current_epoch = EpochNo(4); // post Byron→Shelley boundary
        let eta_before = state.epoch_nonce;
        // Apply a block at slot 86_420 (first active overlay slot in
        // Shelley) to seed evolving/candidate.
        state.apply_block(
            SlotNo(86_420),
            &[0x42; 64],
            Some(HeaderHash([0xCC; 32])),
            &c,
            NonceDerivation::TPraos,
        );
        // Active epoch_nonce must NOT have rotated.
        assert_eq!(state.epoch_nonce, eta_before);
        assert_eq!(state.current_epoch, EpochNo(4));

        // Apply a block at slot 432_000 (the prior failure point).
        // Pre-R263 this fired `tick_epoch_transition`; post-R263 it
        // must remain mid-epoch-4.
        state.apply_block(
            SlotNo(432_000),
            &[0x42; 64],
            Some(HeaderHash([0xDD; 32])),
            &c,
            NonceDerivation::TPraos,
        );
        assert_eq!(
            state.current_epoch,
            EpochNo(4),
            "preprod slot 432000 must stay in Shelley epoch 4 — \
             a tick fired here would rotate epoch_nonce and break VRF",
        );
        assert_eq!(
            state.epoch_nonce, eta_before,
            "preprod active epoch_nonce must NOT rotate at slot 432000 \
             (this is the R249/R262 bug class)",
        );

        // Apply a block at slot 518_400 (the actual Shelley epoch 4→5
        // boundary). NOW `tick_epoch_transition` should fire.
        state.apply_block(
            SlotNo(518_400),
            &[0x42; 64],
            Some(HeaderHash([0xEE; 32])),
            &c,
            NonceDerivation::TPraos,
        );
        assert_eq!(state.current_epoch, EpochNo(5));
        // At the actual epoch boundary, epoch_nonce SHOULD differ
        // (rotation fired with candidate ⭒ prev_hash ⭒ extra_entropy).
        assert_ne!(state.epoch_nonce, eta_before);
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
