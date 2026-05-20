---
title: "Round 613 ShelleyDelplPredFailure scaffold + wire DELEGS DelplFailure (A5 Phase-2.5)"
parent: Reference
---

# Round 613 ShelleyDelplPredFailure scaffold + wire DELEGS DelplFailure (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ShelleyDelplPredFailure` 2-variant scaffold (the DELPL
sub-rule that DELEGS dispatches into) and wires
`ShelleyDelegsPredFailure::DelplFailure` to the typed enum. After
R613, the full LEDGER → DELEGS → DELPL chain renders typed
end-to-end through nested Display.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Delpl.hs:61-65,137-173`
  (`data ShelleyDelplPredFailure era` 2-variant ADT:
  `PoolFailure (PredicateFailure (EraRule "POOL" era))` |
  `DelegFailure (PredicateFailure (EraRule "DELEG" era))`, CBOR
  envelope `[tag, payload]` with tag 0/1).

## Changes

- Added `ShelleyDelplPredFailure` 2-variant enum:
  - Tag 0 `PoolFailure(Vec<u8>)` — pending POOL sub-rule decoder.
  - Tag 1 `DelegFailure(Vec<u8>)` — pending DELEG sub-rule
    decoder.
- Helpers: `tag()`, `constructor()`, `from_cbor` walks the
  2-element envelope and captures the raw inner payload. Unknown
  tags reject explicitly.
- Display matches the scaffold pattern: `<Constructor> <raw-cbor
  N bytes>`.
- Refactored `ShelleyDelegsPredFailure::DelplFailure(Vec<u8>)` →
  `DelplFailure(ShelleyDelplPredFailure)`. DELEGS `from_cbor`
  decodes through `ShelleyDelplPredFailure::from_cbor`. Display
  routes the typed nested payload: `DelplFailure (<inner-shape>)`.

Test surface updates:
- R612's tests now construct `DelplFailure` with the typed
  `PoolFailure(vec![])` payload.
- Updated `_from_cbor_decodes_tag1` to assert the full nested
  Display chain through the new typed sub-rule.
- New `_display_routes_typed_delegs` pins the LEDGER → DELEGS →
  DELPL typed Display chain.
- New `_pool_failure_decodes_tag0` end-to-end DELPL envelope walk.
- New `_deleg_failure_decodes_tag1` end-to-end DELPL envelope walk.
- New `_unknown_tag_rejects` (tag 88).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (206 lib + 4
  doctests + 1 main, +3 net new tests vs R612 baseline of 203 —
  added 4, replaced 1)

## Remaining (A5 Phase-2.5+)

- `ShelleyPoolPredFailure` decoder for DELPL tag 0.
- `ShelleyDelegPredFailure` decoder for DELPL tag 1.
- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
