---
title: "Round 610 Wire typed UTXO sub-rule into UTXOW tag 4 (A5 Phase-2.5)"
parent: Reference
---

# Round 610 Wire typed UTXO sub-rule into UTXOW tag 4 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Bubbles the now-fully-typed `ShelleyUtxoPredFailure` (R609 close)
up into the parent `ShelleyUtxowPredFailure::UtxoFailure` variant
(UTXOW tag 4). After R610, **all 11 UTXOW variants carry typed
payloads**, and the UTXOW→UTXO predicate-failure chain renders
end-to-end through nested typed Display.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs:121,190-193`
  (UTXOW tag 4 `UtxoFailure (PredicateFailure (EraRule "UTXO" era))`).

## Changes

- Refactored `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)` →
  `UtxoFailure(ShelleyUtxoPredFailure)`.
- Updated `ShelleyUtxowPredFailure::Display` to route the typed
  inner payload through its `Display`:
  `UtxoFailure (<inner-utxo-shape>)`.
- Updated `ShelleyUtxowPredFailure::from_cbor` dispatcher: tag 4
  routes through `ShelleyUtxoPredFailure::from_cbor`. Tag 4 is now
  the only call site reading the typed inner UTXO predicate
  failure from the UTXOW envelope.
- Replaced R601's `_routes_tag4_to_raw_variant` test with two
  typed-end-to-end equivalents:
  - `_utxo_failure_routes_to_typed_utxo` (UTXOW tag 4 wrapping
    UTXO tag 3 `InputSetEmptyUTxO`, no-payload inner).
  - `_utxo_failure_nests_full_utxo_predicate` (UTXOW tag 4
    wrapping UTXO tag 4 `FeeTooSmallUTxO` with full Mismatch
    Display chain).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (200 lib + 4
  doctests + 1 main, +1 net new test vs R609 baseline of 199 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)` (LEDGER tag 0)
  — bubbles the full UTXOW→UTXO chain into the top-level LEDGER
  predicate-failure layer.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1 (mirror of
  the UTXOW work — DELEGS has its own sub-rule tree).
- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
