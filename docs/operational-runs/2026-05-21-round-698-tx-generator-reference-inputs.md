---
title: "Round 698 tx-generator DumpToFile renders reference inputs (A4)"
parent: Reference
---

# Round 698 tx-generator DumpToFile renders reference inputs (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body reference-input set (`btbrReferenceInputs` /
`ctbrReferenceInputs`) for the Babbage / Conway era renderers,
instead of rejecting any tx that carries a non-empty reference
set.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxBody.hs`
  (`btbrReferenceInputs :: !(Set TxIn)`); the Conway `TxBodyRaw`
  carries the analogous `ctbrReferenceInputs`. Reference inputs
  are a Babbage+ feature (no Alonzo field).

## Changes

- `show_babbage_tx_for_dump` / `show_conway_tx_for_dump` drop
  the `ensure_empty_or_absent(tx.body.reference_inputs, …)`
  gate and render the field via the existing `show_tx_in_list`
  helper (sorted to match upstream `Set TxIn` Show ordering),
  wrapped in `fromList [...]`.

1 new focused unit test:
- `dumptofile_reference_inputs_render` — absent set → empty
  `show_tx_in_list` output (interpolates into `fromList []`);
  one entry → the typed `TxIn` render.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (246 lib + 5 main,
  +1 new test vs R697 baseline of 245)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, collateral return, total collateral, auxiliary data,
  update).
