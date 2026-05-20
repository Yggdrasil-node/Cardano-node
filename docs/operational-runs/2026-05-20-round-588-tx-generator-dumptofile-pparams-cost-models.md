---
title: "Round 588 tx-generator DumpToFile PParamsUpdate CostModels"
parent: Reference
---

# Round 588 tx-generator DumpToFile PParamsUpdate CostModels

Date: 2026-05-20

## Scope

This round closes the final composite PParamsUpdate field
`cppCostModels`. **All 30/30 Conway PParamsUpdate fields now render
— the PParamsUpdate Show surface is complete for the Conway era.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/CostModels.hs:376-380`
  (`CostModels` 2-field record: `_costModelsValid :: Map Language
  CostModel`, `_costModelsUnknown :: Map Word8 [Int64]`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/CostModels.hs:103-123`
  (`CostModel` custom `Show`: `"CostModel " <> show lang <> " " <>
  show cm`).

## Changes

- Added `show_pparam_cost_models` rendering yggdrasil's
  `Option<BTreeMap<u8, Vec<i64>>>` into upstream `SJust (CostModels
  {_costModelsValid = fromList [(<Language>, CostModel <Language>
  [<cost-array>]),...], _costModelsUnknown = fromList [(<tag>,
  [<costs>]),...]})`. Language tag mapping:
  - tag 0 → PlutusV1
  - tag 1 → PlutusV2
  - tag 2 → PlutusV3
  - other tags → `_costModelsUnknown` (forward-compat slot for
    future Plutus versions that the current ledger doesn't yet
    validate).
- Updated `show_conway_pparams_update` to wire the field and
  removed it from the rejection list.
- Added `dumptofile_show_conway_gov_action_parameter_change_with_cost_models`
  unit test setting a 3-entry CostModels (PlutusV1 + PlutusV3 +
  unknown tag 7) and asserting the full rendered shape.
- Moved the rejection regression test from `cppCostModels` (now
  renders) to `min_utxo_value` (Shelley-era-only — Conway dropped
  it; the boundary error remains for that field).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (54 tests)
- `cargo test -p yggdrasil-tx-generator` (237 lib tests + 5
  CLI/golden, +1 from R587 baseline)

## Remaining (DumpToFile)

The PParamsUpdate Show surface is structurally complete. Remaining
work for full byte-equivalent parity with upstream:

- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets (needs Byron AddressInfo packing port).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage
  (`\NUL` through `\DEL` for the 0x00-0x1F + 0x7F range).
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
