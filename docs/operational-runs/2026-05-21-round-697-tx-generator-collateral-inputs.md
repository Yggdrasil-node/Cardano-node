---
title: "Round 697 tx-generator DumpToFile renders collateral inputs (A4)"
parent: Reference
---

# Round 697 tx-generator DumpToFile renders collateral inputs (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body collateral-input set (`atbrCollateral` /
`btbrCollateralInputs` / `ctbrCollateralInputs`) for the
Alonzo / Babbage / Conway era renderers, instead of rejecting
any tx that carries a non-empty collateral set.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxBody.hs`
  (`atbrCollateral :: !(Set TxIn)`); the Babbage / Conway
  `TxBodyRaw` carry `btbrCollateralInputs` / `ctbrCollateralInputs`.

## Changes

- `show_alonzo_tx_for_dump` / `show_babbage_tx_for_dump` /
  `show_conway_tx_for_dump` drop the
  `ensure_empty_or_absent(tx.body.collateral, …)` gate and
  render the field value via the existing `show_tx_in_list`
  helper (which sorts to match upstream `Set TxIn` Show
  ordering), wrapped in `fromList [...]`.

1 new focused unit test:
- `dumptofile_collateral_list_renders_sorted` — two
  out-of-order collateral inputs render in sorted `TxIn` order
  (the collateral-set rendering contract).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (245 lib + 5 main,
  +1 new test vs R696 baseline of 244)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, reference inputs, collateral return, total collateral,
  auxiliary data, update).
