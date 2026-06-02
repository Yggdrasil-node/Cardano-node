---
title: "Round 557 tx-generator Plutus auto-budget"
parent: Reference
---

# Round 557 tx-generator Plutus auto-budget

Date: 2026-05-20

## Scope

Advanced the pure-Rust `tx-generator` AutoScript path from an explicit
runtime boundary to upstream-shaped loop-budget fitting. This round
mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/PlutusContext.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`

## Changes

- `Cardano.TxGenerator.PlutusContext` now carries
  `PlutusAutoBudget`, `PlutusBudgetFittingStrategy`,
  `PlutusAutoLimitingFactor`, and `PlutusBudgetSummary`.
- `plutusAutoBudgetMaxOut` and `plutusAutoScaleBlockfit` now use the
  upstream binary-search strategy to fit loop-script redeemers against
  per-transaction, per-block, or fixed-transactions-per-block targets.
- The Rust CEK runner's evaluator-level out-of-budget result at a high
  search bound is treated as an over-target limiting factor so the
  search can converge instead of failing before it reaches the fitted
  loop count.
- `Benchmarking.Script.Core.makePlutusContext` now resolves
  `AutoScript`, parses `maxBlockExecutionUnits`, sets the environment
  budget summary, and writes `plutus-budget-summary.json`.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator tx_generator::plutus_context --lib
cargo test -p yggdrasil-tx-generator script::core --lib
cargo test -p yggdrasil-tx-generator --lib
cargo check -p yggdrasil-tx-generator
cargo clippy -p yggdrasil-tx-generator --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check-all
cargo lint
cargo test-all
python dev/test/check-parity-matrix.py
python dev/test/check-strict-mirror.py
python dev/test/filetree.py check
```

Observed result:

```text
tx_generator::plutus_context: 9 passed
script::core: 29 passed
tx-generator lib: 158 passed
yggdrasil-tx-generator check: passed
yggdrasil-tx-generator clippy: passed
workspace cargo gates: passed
parity matrix: 22 entries validated
strict mirror: 0 violations
filetree check: clean
```

## Remaining Tx-Generator Gaps

Exact `DumpToFile` rendering, Benchmark submission, and upstream binary
comparison evidence remain open. R558 follows this round by wiring
`previewNtoMTransaction` projected size/fee values back into the budget
summary before the dump.
