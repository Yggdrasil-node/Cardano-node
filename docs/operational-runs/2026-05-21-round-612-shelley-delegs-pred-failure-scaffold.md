---
title: "Round 612 ShelleyDelegsPredFailure scaffold + wire LEDGER tag 1 (A5 Phase-2.5)"
parent: Reference
---

# Round 612 ShelleyDelegsPredFailure scaffold + wire LEDGER tag 1 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Opens the `ShelleyDelegsPredFailure` newtype scaffold (DELEGS
sub-rule under `ShelleyLedgerPredFailure::DelegsFailure`, LEDGER
tag 1) and wires the parent variant to the typed enum. After
R612, **all 4 LEDGER variants carry typed payloads** — the LEDGER
root is fully wired. The nested DELPL sub-rule decoder
(`ShelleyDelplPredFailure` dispatching into POOL/DELEG) lands in
a follow-on round.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Delegs.hs:83-86,147-167`
  (`newtype ShelleyDelegsPredFailure era = DelplFailure
  (PredicateFailure (EraRule "DELPL" era))` with single-tag CBOR
  envelope `[1, DELPL-failure]`).

## Changes

- Added `ShelleyDelegsPredFailure` enum with the single
  `DelplFailure(Vec<u8>)` variant. Per upstream, the DELEGS layer
  is a newtype around the DELPL sub-rule.
- Helpers: `tag()` returns Word8 1, `constructor()` returns
  `"DelplFailure"`, `from_cbor` walks the 2-element envelope
  reading `[1, raw-DELPL-bytes]`. Unknown tags reject explicitly.
- Display matches the scaffold pattern: `DelplFailure <raw-cbor N
  bytes>`.
- Refactored
  `ShelleyLedgerPredFailure::DelegsFailure(Vec<u8>)` →
  `DelegsFailure(ShelleyDelegsPredFailure)`. Updated LEDGER
  Display to route the typed nested payload:
  `DelegsFailure (<inner-delegs-shape>)`.

Test surface updates:
- R595's `_tag_dispatch` and `_constructor_names` tests now
  construct `DelegsFailure` with the typed scaffold payload.
- Replaced the R611 `_display_marks_delegs_raw_cbor` test with
  `_display_routes_typed_delegs` (typed nested Display chain).
- New `_from_cbor_decodes_tag1` end-to-end DELEGS envelope walk.
- New `_unknown_tag_rejects` (rejects tag 99).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (203 lib + 4
  doctests + 1 main, +2 net new tests vs R611 baseline of 201 —
  added 3, replaced 1)

## Remaining (A5 Phase-2.5+)

- `ShelleyDelplPredFailure` decoder (DELPL sub-rule dispatches
  into POOL/DELEG sub-rules — multi-variant tree).
- `ShelleyPoolPredFailure` decoder.
- `ShelleyDelegPredFailure` decoder.
- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
