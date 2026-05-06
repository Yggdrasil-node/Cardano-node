//! Per-era `LedgerState` apply implementations.
//!
//! Mirrors upstream Haskell's per-era `Cardano.Ledger.<Era>.LedgerState`
//! split: each era owns its block-application rule (transitions UTxO,
//! certificates, withdrawals, governance, etc.) and lives in its own
//! file at `state/eras/<era>.rs`. The orchestrator at `state.rs::apply_block`
//! dispatches by `Era` tag to the per-era apply method.
//!
//! Each era's file declares `impl LedgerState { pub(in crate::state) fn apply_<era>_block(...) }`
//! — the `pub(in crate::state)` visibility is the minimum that lets the
//! dispatcher in `state.rs::apply_block_validated` (a grandparent module
//! of `state::eras::<era>`) call across module boundaries while keeping
//! the method crate-internal. `pub(super)` would only expose to
//! `state::eras` (the parent).
//!
//! R269q (this round) extracts Byron only as a validation slice for the
//! per-era split pattern. Subsequent rounds R269r–R269w extract Shelley,
//! Allegra, Mary, Alonzo, Babbage, and Conway respectively.

pub(super) mod allegra;
pub(super) mod alonzo;
pub(super) mod babbage;
pub(super) mod byron;
pub(super) mod conway;
pub(super) mod mary;
pub(super) mod shelley;
