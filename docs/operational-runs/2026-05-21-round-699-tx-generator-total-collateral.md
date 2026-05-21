---
title: "Round 699 tx-generator DumpToFile renders total collateral (A4)"
parent: Reference
---

# Round 699 tx-generator DumpToFile renders total collateral (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `StrictMaybe Coin` total-collateral field
(`btbrTotalCollateral` / `ctbrTotalCollateral`) for the Babbage
/ Conway era renderers, instead of rejecting any tx that sets
it.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxBody.hs`
  (`btbrTotalCollateral :: !(StrictMaybe Coin)`); the Conway
  `TxBodyRaw` carries the analogous `ctbrTotalCollateral`.

## Changes

- `show_babbage_tx_for_dump` / `show_conway_tx_for_dump` drop
  the `ensure_absent(tx.body.total_collateral, …)` gate and
  render the field via the existing `show_strict_maybe_coin`
  helper (`SNothing` / `SJust (Coin <n>)`).

1 new focused unit test:
- `dumptofile_total_collateral_render` — `SNothing` + the set
  `SJust (Coin <n>)` form.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (247 lib + 5 main,
  +1 new test vs R698 baseline of 246)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, collateral return, auxiliary data, update).
