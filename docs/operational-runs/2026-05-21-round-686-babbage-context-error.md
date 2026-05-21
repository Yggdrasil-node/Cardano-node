---
title: "Round 686 Typed ContextError BabbageContextError (A5 Phase-2.5)"
parent: Reference
---

# Round 686 Typed ContextError BabbageContextError (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `TxOutSource` and `BabbageContextError` types and types
`ContextError` tag 8 (`BabbageContextError`) — the last raw
`ContextError` variant. **All 8 `ContextError` variants now
carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxInfo.hs:248-296`
  (`data BabbageContextError era` — CBOR `Sum` tags 0-7
  (tag 3 unused): ByronTxOutInContext, TranslationLogicMissing-
  Input, RedeemerPointerPointsToNothing, InlineDatumsNot-
  Supported, ReferenceScriptsNotSupported,
  ReferenceInputsNotSupported, TimeTranslationPastHorizon).
- `data TxOutSource = TxOutFromInput TxIn | TxOutFromOutput
  TxIx` — CBOR `Sum` `[0, TxIn]` / `[1, TxIx]`.

## Changes

- Added `TxOutSource` enum (TxOutFromInput / TxOutFromOutput).
- Added `BabbageContextError` 7-variant enum — every variant
  carries a typed payload (`TxOutSource`, `TxIn`,
  `ConwayPlutusPurposeIx`, `Vec<TxIn>`, `String`).
- Refactored `ContextError::BabbageContextError(Vec<u8>)` →
  `BabbageContextError(BabbageContextError)`. Removed the
  now-unused `capture_raw` closure from `ContextError::from_decoder`
  — all 8 variants typed.

1 new focused unit test:
- `context_error_decodes_babbage_context_error` — a tag-8
  `ContextError` whose inner `BabbageContextError` is
  `TranslationLogicMissingInput` with a typed `TxIn`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (353 lib + 4
  doctests + 1 main, +1 new test vs R685 baseline of 352)

## Remaining (A5 Phase-2.5+)

- Deeper Shelley DELEGS → DELPL → DELEG/POOL payload decoders
  for the pre-Conway eras.
