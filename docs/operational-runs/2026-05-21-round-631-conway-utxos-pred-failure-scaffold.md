---
title: "Round 631 ConwayUtxosPredFailure scaffold + wire UTXO tag 0 (A5 Phase-2.5)"
parent: Reference
---

# Round 631 ConwayUtxosPredFailure scaffold + wire UTXO tag 0 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayUtxosPredFailure` 2-variant scaffold (the UTXOS
Plutus-script-evaluation sub-rule) plus the supporting
`FailureDescription` and `TagMismatchDescription` helper types,
and wires `ConwayUtxoPredFailure::UtxosFailure` to the typed
enum. The `ValidationTagMismatch` variant is fully typed; only
`CollectErrors` (a Plutus collection-error list) keeps a raw
payload.

The Conway LEDGER → UTXOW → UTXO → UTXOS chain renders typed
end-to-end for the ValidationTagMismatch path.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxos.hs:83-95,140-151`
  (`data ConwayUtxosPredFailure era` 2-variant ADT, CBOR
  encoder tags 0/1).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Rules/Utxos.hs:296-345`
  (`FailureDescription` — single PlutusFailure variant at CBOR
  tag 1, upstream skips tag 0; `TagMismatchDescription` —
  PassedUnexpectedly tag 0 / FailedUnexpectedly tag 1).

## Changes

- Added `FailureDescription` struct (`PlutusFailure Text
  ByteString`). `from_decoder` walks the 3-element CBOR `Sum`
  envelope `[1, text, bytes]` (upstream deliberately skips
  tag 0 — a removed legacy constructor). Display:
  `PlutusFailure <quoted-text> <bytestring N bytes>` (the
  reconstruction blob is rendered as a hex marker rather than
  the full ByteString mnemonic-escape Show — cardano-submit-api
  doesn't carry that helper).
- Added `TagMismatchDescription` enum:
  - Tag 0 `PassedUnexpectedly` — no payload.
  - Tag 1 `FailedUnexpectedly(Vec<FailureDescription>)` —
    NonEmpty, rejected if empty at decode time.
- Added `ConwayUtxosPredFailure` 2-variant enum:
  - Tag 0 `ValidationTagMismatch { is_valid: bool, description:
    TagMismatchDescription }` — fully typed. `is_valid` decodes
    the CBOR bool; `description` decodes through
    `TagMismatchDescription`.
  - Tag 1 `CollectErrors(Vec<u8>)` — raw pending CollectError
    decoder.
- `from_cbor` enforces exact envelope length per variant
  (3-element for tag 0, 2-element for tag 1). Unknown tags
  reject.
- Display: tag 0 emits `ValidationTagMismatch (IsValid
  True|False) (<description>)`; tag 1 emits `<raw-cbor N bytes>`.
- Refactored `ConwayUtxoPredFailure::UtxosFailure(Vec<u8>)` →
  `UtxosFailure(ConwayUtxosPredFailure)`. UTXO tag 0 dispatcher
  routes through the typed decoder; Display routes the typed
  nested payload.

5 new focused unit tests:
- `_validation_tag_mismatch_passed_tag0` — IsValid + typed
  PassedUnexpectedly.
- `_validation_tag_mismatch_failed_tag0` — IsValid + typed
  FailedUnexpectedly with a nested FailureDescription
  (PlutusFailure text+bytes).
- `_collect_errors_raw_tag1` — raw routing confirmation.
- `_unknown_tag_rejects` — tag 99 rejection.
- `conway_utxo_pred_failure_utxos_typed_routing_tag0` —
  end-to-end UTXO → UTXOS chain.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (278 lib + 4
  doctests + 1 main, +5 new tests vs R630 baseline of 273)

## Remaining (A5 Phase-2.5+)

- Conway UTXOS `CollectErrors` typed payload — `NonEmpty
  (CollectError era)` (Plutus script collection errors).
- Conway UTXO raw variants (Value, ExUnits, ValidityInterval,
  DeltaCoin, NonEmptyMap, triple/pair encodings).
- Conway UTXOW raw variants (tags 10/11/12/13/15/18).
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
