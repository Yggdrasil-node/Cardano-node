---
title: "Round 609 NonEmptyTxOut scaffold for UTXO tags 6/10 (A5 Phase-2.5)"
parent: Reference
---

# Round 609 NonEmptyTxOut scaffold for UTXO tags 6/10 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `RawTxOut` wrapper + `NonEmptyTxOut` carrier with a
CBOR datum-walker (`skip_single_datum`), and wires the last two
raw-payload UTXO variants:
- Tag 6 `OutputTooSmallUTxO(NonEmptyTxOut)`.
- Tag 10 `OutputBootAddrAttrsTooBig(NonEmptyTxOut)`.

**After R609, all 11 `ShelleyUtxoPredFailure` variants carry
typed payloads at the outer-shape level.** Inner per-TxOut typed
Show parse and full typed `Addr` parse-tree (Shelley vs Bootstrap
split) remain pending.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs:184-188,236,240`
  (tag 6 `OutputTooSmallUTxO (NonEmpty (TxOut era))`, tag 10
  `OutputBootAddrAttrsTooBig (NonEmpty (TxOut era))`).

## Changes

- Added `RawTxOut(Vec<u8>)` wrapper holding raw on-wire bytes for
  each TxOut. `from_decoder` walks a single CBOR datum (regardless
  of era-specific shape) and captures the byte range. Display
  emits `TxOut <hex N bytes: <hex>>` (interim format — full
  Shelley/Babbage parse-tree port lands in a follow-on round).
- Added `skip_single_datum` helper that walks the next CBOR datum
  for major types 0/2/4/5/6 (unsigned, bytes, array, map, tag) so
  era-specific TxOut envelopes — 2-array (Shelley/Allegra/Mary),
  3-array (Alonzo), tagged map (Babbage+) — are all consumed
  correctly. Recurses through arrays/maps.
- Added `NonEmptyTxOut` struct (`Vec<RawTxOut>` preserving
  insertion order). `from_cbor` and `from_decoder` entry points,
  non-empty invariant enforced. Display matches upstream `Show
  (NonEmpty a)`: `<head> :| [<tail>...]`.
- Refactored variant payloads:
  - `OutputTooSmallUTxO(Vec<u8>)` →
    `OutputTooSmallUTxO(NonEmptyTxOut)`.
  - `OutputBootAddrAttrsTooBig(Vec<u8>)` →
    `OutputBootAddrAttrsTooBig(NonEmptyTxOut)`.
- Updated `from_cbor` dispatcher: tags 6/10 share the typed
  branch via `NonEmptyTxOut::from_decoder`. Display routes both
  through their typed Display.
- Replaced R608's tag-6-raw-routing test with a typed
  `_output_too_small_decodes_tag6` end-to-end test, plus a
  NonEmpty rejection test.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (199 lib + 4
  doctests + 1 main, +1 net new test vs R608 baseline of 198 —
  added 2, replaced 1)

## Remaining (A5 Phase-2.5+)

- Inner typed `TxOut era` parse (Shelley/Allegra/Mary 2-array,
  Alonzo 3-array, Babbage+ map).
- Full typed `Addr` Show parse (Shelley vs Bootstrap variant
  split, PaymentCredential + StakeReference).
- Wire typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire typed `ShelleyUtxowPredFailure` into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- `ShelleyDelegsPredFailure` decoder for LEDGER tag 1.
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras (Conway adds 4+ governance-specific
  variants).
