---
title: "Round 696 tx-generator DumpToFile renders script-integrity hash (A4)"
parent: Reference
---

# Round 696 tx-generator DumpToFile renders script-integrity hash (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `StrictMaybe ScriptIntegrityHash` field
(`atbrScriptIntegrityHash` / `btbrScriptIntegrityHash` /
`ctbrScriptIntegrityHash`) for the Alonzo / Babbage / Conway
era renderers, instead of rejecting any tx that sets it.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxBody.hs:125,170`
  (`type ScriptIntegrityHash = SafeHash
  EraIndependentScriptIntegrity` — a bare type alias;
  `atbrScriptIntegrityHash :: !(StrictMaybe ScriptIntegrityHash)`).

## Changes

- Added `show_strict_maybe_script_integrity_hash(Option<[u8;
  32]>)` — renders `SNothing` / `SJust (SafeHash "...")` (no
  newtype wrapper, since `ScriptIntegrityHash` is a bare type
  alias for `SafeHash`).
- `show_alonzo_tx_for_dump` / `show_babbage_tx_for_dump` /
  `show_conway_tx_for_dump` drop the
  `ensure_absent(tx.body.script_data_hash, …)` gate and render
  the field value.

1 new focused unit test:
- `dumptofile_script_integrity_hash_render` — `SNothing` + the
  set `SJust (SafeHash "...")` form. The pre-existing
  rendered-output tests for empty-hash txs still assert
  `…ScriptIntegrityHash = SNothing` and remain green (a
  `None` hash still renders `SNothing`).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (244 lib + 5 main,
  +1 new test vs R695 baseline of 243)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, collateral, reference inputs, collateral return, total
  collateral, auxiliary data, update).
