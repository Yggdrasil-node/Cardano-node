---
title: "Round 555 tx-generator script spend"
parent: Reference
---

# Round 555 tx-generator script spend

Date: 2026-05-20

## Scope

Advanced the pure-Rust `tx-generator` runtime from key-only
transaction construction to static-budget Plutus script-spend
transaction assembly. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Tx.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Fund.hs`

## Changes

- `Cardano.TxGenerator.Tx.genTx` now accepts ledger protocol
  parameters, matching the upstream `LedgerProtocolParameters era`
  argument used by `createTransactionBody`.
- Script-witnessed input funds now populate Plutus V1/V2/V3 scripts,
  datum witnesses, spending redeemers, and `script_data_hash`.
- Collateral funds remain key-witnessed and are signed with the
  generated transaction body hash.
- `Script/Core.submitInEra` now threads protocol cost models from
  local/query protocol-parameter state into `genTx`.
- Finite `DiscardTX` `NtoM` streams can now spend static-budget script
  funds and store generated destination funds.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator --lib tx_generator::tx
cargo test -p yggdrasil-tx-generator --lib script::core
cargo clippy -p yggdrasil-tx-generator --all-targets
```

Observed result:

```text
tx_generator::tx: 6 passed
script::core: 28 passed
clippy: clean
```

## Remaining Tx-Generator Gaps

Plutus pre-execution / auto-budget fitting is still open:
`preExecutePlutusScript` and `plutusAutoScaleBlockfit` remain explicit
runtime boundaries. Exact `DumpToFile` rendering, Benchmark submission,
and upstream binary comparison evidence also remain.
