---
title: "Round 654 Conway UTXOS CollectErrors typed scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 654 Conway UTXOS CollectErrors typed scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXOS tag 1 (`CollectErrors`) by adding the
`Language` enum and the `CollectError` 4-variant scaffold.
**After R654, both Conway UTXOS variants carry typed payloads —
the Conway UTXOS sub-rule is fully typed.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Plutus/Context.hs:344-375`
  (`data CollectError era = NoRedeemer (PlutusPurpose AsItem
  era) | NoWitness ScriptHash | NoCostModel Language |
  BadTranslation (ContextError era)`; CBOR `Sum` tags 0-3).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/Language.hs:224-285`
  (`data Language = PlutusV1 | PlutusV2 | PlutusV3 | PlutusV4`;
  `encCBOR = encodeEnum` → Word8 0-3).

## Changes

- Added `Language` enum (PlutusV1-V4) — decodes the Word8 enum,
  Display matches stock-derived Show.
- Added `CollectError` 4-variant enum:
  - Tag 0 `NoRedeemer(Vec<u8>)` — raw pending PlutusPurpose
    AsItem decoder.
  - Tag 1 `NoWitness(ScriptHash)` — typed.
  - Tag 2 `NoCostModel(Language)` — typed.
  - Tag 3 `BadTranslation(Vec<u8>)` — raw pending the
    era-specific ContextError decoder.
  - `from_decoder` reads the 2-element `[tag, payload]` envelope;
    raw variants capture the payload by byte range via the
    ledger decoder's recursive `skip()`.
- Added `NonEmptyCollectError` carrier — `Vec<CollectError>`,
  empty arrays reject at decode time.
- Refactored `ConwayUtxosPredFailure::CollectErrors(Vec<u8>)` →
  `CollectErrors(NonEmptyCollectError)`. `from_cbor` decodes the
  2-element envelope `[1, NonEmpty CollectError]`.

2 new tests + 1 replaced:
- Replaced R631's `_collect_errors_raw_tag1` with the typed
  `_collect_errors_typed_tag1` (NoWitness + NoCostModel
  round-trip).
- New `_collect_errors_rejects_empty` — NonEmpty empty-array
  rejection.

## Conway predicate-failure tree status

Every Conway sub-rule is now structurally typed:
- LEDGER 9/9, UTXOW 17/19, UTXO 23/23, UTXOS **2/2** (closed by
  R654), CERT chain DELEG/POOL/GOVCERT complete, GOV 19/19.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (320 lib + 4
  doctests + 1 main, +1 net new test vs R653 baseline of 319 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW tag 10 (MissingRedeemers — `NonEmpty
  (PlutusPurpose AsItem, ScriptHash)`).
- `CollectError` raw variants (tag 0 NoRedeemer, tag 3
  BadTranslation).
- GovAction per-variant typed payloads.
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
