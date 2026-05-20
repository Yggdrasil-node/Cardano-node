---
title: "Round 594 cardano-submit-api typed rejection scaffold (A5 Phase-2)"
parent: Reference
---

# Round 594 cardano-submit-api typed rejection scaffold (A5 Phase-2)

Date: 2026-05-20

## Scope

Begins Category A5 (cardano-submit-api structured rejection enum)
Phase 2. Adds the type-level scaffold for the era-tagged
`TxValidationErrorInCardanoMode` rejection enum on top of R345's
Phase-1 raw-bytes carrier. Per-variant CBOR decoders that expand
the era payload into typed predicate-failure sums are deferred to
Phase-2.5+ rounds.

## Upstream references

- `.reference-haskell-cardano-node/cardano-submit-api/src/Cardano/TxSubmit/Types.hs:95-110`
  (`TxCmdTxSubmitValidationError !TxValidationErrorInCardanoMode`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/API/Mempool.hs:157`
  (`newtype ApplyTxError ShelleyEra = ShelleyApplyTxError (NonEmpty
  (ShelleyLedgerPredFailure ShelleyEra))`).
- Same `ApplyTxError <era>` newtype-with-NonEmpty-PredicateFailure
  shape repeats for Allegra/Mary/Alonzo/Babbage/Conway/Dijkstra in
  their respective `Cardano.Ledger.<Era>` modules.

## Changes

- `crates/tools/cardano-submit-api/src/types.rs`:
  - Added `TxValidationEra` enum (Shelley/Allegra/Mary/Alonzo/
    Babbage/Conway variants) with `apply_tx_error_constructor()`
    helper exposing the upstream `<Era>ApplyTxError` constructor
    name for Show rendering.
  - Added `EraApplyTxError` payload struct: raw CBOR + rendered
    text + Display impl. Mirrors upstream's `(NonEmpty
    (PredicateFailure ...))` collapsed into raw bytes for now;
    per-variant typed expansion lands in Phase-2.5+.
  - Added `TxValidationErrorInCardanoMode` 6-variant era-tagged
    enum, each variant wrapping an `EraApplyTxError`. Helpers:
    `from_raw(era, payload)`, `era()`, `payload()`. Display impl
    emits upstream `<Era>ApplyTxError (<rendered>)`.
  - Added `TxSubmitValidationError::into_typed(era)` constructor
    routing existing rejections through the new typed view
    without breaking JSON serialization or existing callers.
- `docs/COMPLETION_ROADMAP.md` A5 section updated to record the
  Phase-2 scaffold and the Phase-2.5+ remaining work.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (150 lib + 4
  doctests, +5 new tests vs R588 baseline of 145)

5 focused unit tests:
- `tx_validation_era_constructor_names` pins all 6 upstream
  constructor names.
- `tx_validation_error_in_cardano_mode_from_raw_preserves_era_and_payload`
  round-trip.
- `tx_validation_error_in_cardano_mode_display_wraps_in_constructor`
  pins the `<Era>ApplyTxError (<payload>)` Display shape.
- `tx_submit_validation_error_into_typed_round_trips` verifies the
  scaffolding integrates with the existing `TxSubmitValidationError`.

## Remaining (A5 Phase-2.5+)

- Port `ShelleyLedgerPredFailure` Conway-variant sum (~40 variants:
  UtxoFailure, DelegsFailure, etc.) and per-variant CBOR decoders.
- Extend `EraApplyTxError` to carry a typed `Vec<PredicateFailure>`
  alongside the raw CBOR fallback. Decoder errors must keep the raw
  bytes accessible.
- Implement upstream `Show (ApplyTxError <era>)` byte-equivalent
  rendering through the typed predicate-failure tree, replacing
  the cached `rendered` string.
- Repeat for Allegra/Mary/Alonzo/Babbage. Conway predicate-failure
  type is the largest (governance variants on top of the Babbage
  set).
