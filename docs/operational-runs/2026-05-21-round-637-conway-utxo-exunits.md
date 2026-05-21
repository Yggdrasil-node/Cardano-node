---
title: "Round 637 Typed Conway UTXO ExUnitsTooBigUTxO variant (A5 Phase-2.5)"
parent: Reference
---

# Round 637 Typed Conway UTXO ExUnitsTooBigUTxO variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXO tag 14 (`ExUnitsTooBigUTxO`) by adding the
`ExUnits` type. After R637, **19 of 23 Conway UTXO variants
carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/ExUnits.hs:79-103,192-193`
  (`data ExUnits' a = ExUnits' { exUnitsMem' :: a, exUnitsSteps'
  :: a }`, `newtype ExUnits = WrapExUnits {unWrapExUnits ::
  ExUnits' Natural}`, CBOR `Rec ExUnits !> To m !> To s`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:118-119,327`
  (`ExUnitsTooBigUTxO (Mismatch RelLTEQ ExUnits)`; encoder
  `Sum (ExUnitsTooBigUTxO . unswapMismatch) 14 !> ToGroup
  (swapMismatch mm)`).

## Changes

- Added `ExUnits { mem: u64, steps: u64 }` — decodes the
  canonical 2-element record array `[mem, steps]`. Display
  matches the stock-derived Show on the newtype + inner record:
  `WrapExUnits {unWrapExUnits = ExUnits' {exUnitsMem' = <n>,
  exUnitsSteps' = <m>}}`.
- Refactored `ConwayUtxoPredFailure::ExUnitsTooBigUTxO(Vec<u8>)`
  → `ExUnitsTooBigUTxO(Mismatch<ExUnits>)`. `from_cbor` decodes
  the 3-element ToGroup-flattened envelope `[14, expected
  ExUnits, supplied ExUnits]` (swapMismatch → expected-first
  wire ordering). Display routes through the generic
  `Mismatch<ExUnits>` Display.

1 new focused unit test:
- `_ex_units_too_big_tag14` — Mismatch<ExUnits> round-trip with
  the swapMismatch expected-first ordering, asserting the full
  nested Display.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (291 lib + 4
  doctests + 1 main, +1 new test vs R636 baseline of 290)

## Remaining (A5 Phase-2.5+)

- Conway UTXO raw variants: tag 6 (Value Mismatch), 13
  (NonEmptyMap TxIn TxOut), 15 (Value), 21 (NonEmpty (TxOut,
  Coin) pair).
- Conway UTXOW raw variants (tags 10/13/18).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
