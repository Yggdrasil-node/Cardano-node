//! PPUP (Protocol Parameter Update Proposal) helpers.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Shelley.Rules.Ppup`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ppup.hs)
//! plus the protocol-version successor predicate from
//! [`Cardano.Ledger.Shelley.PParams`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/shelley/impl/src/Cardano/Ledger/Shelley/PParams.hs)'s
//! `pvCanFollow`. Also covers the d-overlay-slot helpers used by the
//! pre-Praos blocks-made counting rule.
//!
//! Carried in this submodule so the `LedgerState` apply paths (in
//! `state.rs`) and the per-rule submodules (e.g. `epoch_boundary.rs`'s
//! `apply_ppup_at_epoch_boundary`) can call into a single named place
//! for the upstream PPUP gating predicates.
//!
//! Extracted from `state.rs` in R269 fifteenth slice as part of the strict
//! 1:1 filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269o-state-ppup-extraction.md`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Aggregates per-era `Ppup` rule helpers
//! (`Cardano.Ledger.Allegra/Alonzo/Babbage.Rules.Ppup`) plus the
//! `pvCanFollow` protocol-version successor predicate from
//! `Cardano.Ledger.Shelley.PParams`, plus the d-overlay-slot helpers
//! for the pre-Praos blocks-made counting rule. Yggdrasil keeps a
//! single named place for the upstream PPUP gating predicates so
//! `LedgerState` apply paths and `epoch_boundary.rs` call into one
//! module.

use crate::types::UnitInterval;

/// Slot-based context for full upstream PPUP epoch validation.
///
/// When provided to [`super::LedgerState::validate_ppup_proposal`], enables the
/// exact `getTheSlotOfNoReturn` check from upstream `Ppup.hs`:
///
/// * `too_late = first_slot(current_epoch + 1) - stability_window`
/// * If `slot < too_late`: target must equal `current_epoch` (VoteForThisEpoch).
/// * If `slot >= too_late`: target must equal `current_epoch + 1` (VoteForNextEpoch).
///
/// Reference: `Cardano.Ledger.Slot.getTheSlotOfNoReturn`.
#[derive(Clone, Debug)]
pub struct PpupSlotContext {
    /// Current slot of the transaction or block being applied.
    pub slot: u64,
    /// First slot of the next epoch — pre-resolved era-aware (mirrors
    /// upstream `epochInfoFirst (currentEpoch + 1)`). Caller must
    /// compute via [`super::LedgerState::epoch_first_slot`] so any chain
    /// with a Byron prefix uses the correct boundary, not the
    /// `(current + 1) * epoch_size` fixed-length math anchored at
    /// slot 0 (R263/R264 bug class).
    pub first_slot_next_epoch: u64,
    /// Stability window in slots (upstream `stabilityWindow`, typically `3k/f`).
    pub stability_window: u64,
}

/// Upstream `pvCanFollow` — check whether a proposed protocol version is a
/// legal successor to the current one.
///
/// Rules (from `Cardano.Ledger.Shelley.PParams`):
/// * `(succVersion curMajor, 0) == (Just newMajor, newMinor)` — major+1 with minor=0, OR
/// * `(curMajor, curMinor + 1) == (newMajor, newMinor)` — same major with minor+1.
pub fn pv_can_follow(cur_major: u64, cur_minor: u64, new_major: u64, new_minor: u64) -> bool {
    // Increment major by 1 and set minor to 0.
    let major_bump = new_major == cur_major + 1 && new_minor == 0;
    // Keep major, increment minor by 1.
    let minor_bump = new_major == cur_major && new_minor == cur_minor + 1;
    major_bump || minor_bump
}

pub fn overlay_step(offset_from_epoch_start: u64, d: UnitInterval) -> u128 {
    let denominator = d.denominator as u128;
    if denominator == 0 {
        return 0;
    }
    (offset_from_epoch_start as u128)
        .saturating_mul(d.numerator as u128)
        .div_ceil(denominator)
}

pub fn is_overlay_slot_for_blocks_made(first_slot: u64, d: UnitInterval, slot: u64) -> bool {
    if d.numerator == 0 || d.denominator == 0 || slot < first_slot {
        return false;
    }

    let offset = slot - first_slot;
    overlay_step(offset, d) < overlay_step(offset.saturating_add(1), d)
}
