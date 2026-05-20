---
title: "Round 611 Wire typed UTXOW into LEDGER tag 0 (A5 Phase-2.5)"
parent: Reference
---

# Round 611 Wire typed UTXOW into LEDGER tag 0 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Bubbles the now-fully-typed `ShelleyUtxowPredFailure` (R610 close)
up into the parent `ShelleyLedgerPredFailure::UtxowFailure`
variant (LEDGER tag 0). After R611, **3 of 4
`ShelleyLedgerPredFailure` variants carry typed payloads**; only
`DelegsFailure` (LEDGER tag 1) remains raw, awaiting a
`ShelleyDelegsPredFailure` sub-rule decoder.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs:127,221`
  (LEDGER tag 0 `UtxowFailure (PredicateFailure (EraRule "UTXOW"
  era))`).

## Changes

- Refactored `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)` →
  `UtxowFailure(ShelleyUtxowPredFailure)`.
- Updated `ShelleyLedgerPredFailure::Display` to route the typed
  inner UTXOW payload through its `Display`:
  `UtxowFailure (<inner-utxow-shape>)`. The raw-cbor arm now
  exclusively serves `DelegsFailure` (LEDGER tag 1).
- Updated R595's tag-dispatch + constructor-names tests to
  construct UtxowFailure with a typed payload (`InvalidMetadata`,
  the simplest no-payload UTXOW variant).
- Replaced R595's `_display_marks_raw_cbor` test with two
  successors:
  - `_display_routes_typed_utxow` pins the nested typed Display.
  - `_display_marks_delegs_raw_cbor` keeps coverage for the
    remaining raw `DelegsFailure` variant pending its sub-rule
    decoder.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (201 lib + 4
  doctests + 1 main, +1 net new test vs R610 baseline of 200 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- `ShelleyDelegsPredFailure` sub-rule scaffold + decoder for
  LEDGER tag 1 (mirror of the UTXOW work — DELEGS has its own
  sub-rule tree branching into DELPL/POOL/DELEG).
- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
