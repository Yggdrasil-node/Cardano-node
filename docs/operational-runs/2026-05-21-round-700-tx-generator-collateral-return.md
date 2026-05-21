---
title: "Round 700 tx-generator DumpToFile renders collateral return (A4)"
parent: Reference
---

# Round 700 tx-generator DumpToFile renders collateral return (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `StrictMaybe (Sized (TxOut era))`
collateral-return field (`btbrCollateralReturn` /
`ctbrCollateralReturn`) for the Babbage / Conway era renderers,
instead of rejecting any tx that sets it.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxBody.hs`
  (`btbrCollateralReturn :: !(StrictMaybe (Sized (TxOut era)))`);
  the Conway `TxBodyRaw` carries the analogous
  `ctbrCollateralReturn`.

## Changes

- Added `show_strict_maybe_collateral_return(Option<&BabbageTxOut>)`
  — renders `SNothing` / `SJust (Sized {sizedValue = (...),
  sizedSize = N})` by delegating the inner `Sized TxOut` render
  to the existing `show_babbage_tx_out`.
- `show_babbage_tx_for_dump` / `show_conway_tx_for_dump` drop
  the `ensure_absent(tx.body.collateral_return, …)` gate and
  render the field value.

1 new focused unit test:
- `dumptofile_collateral_return_render` — the `SNothing`
  branch; the `SJust` branch delegates to `show_babbage_tx_out`,
  already covered by the full Babbage/Conway renderer tests.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (248 lib + 5 main,
  +1 new test vs R699 baseline of 247)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, auxiliary data, update).
