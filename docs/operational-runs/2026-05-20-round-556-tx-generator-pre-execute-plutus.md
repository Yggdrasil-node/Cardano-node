---
title: "Round 556 tx-generator Plutus pre-execution"
parent: Reference
---

# Round 556 tx-generator Plutus pre-execution

Date: 2026-05-20

## Scope

Advanced the pure-Rust `tx-generator` static Plutus context path from
declared-budget-only witnesses to upstream-shaped pre-execution checks.
This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/TxGenerator/Setup/Plutus.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`

## Changes

- `Cardano.TxGenerator.Setup.Plutus.preExecutePlutusScript` now loads
  the active Plutus cost model from protocol parameters, decodes the
  Plutus V1/V2/V3 script with the shared pure-Rust Flat decoder, builds
  upstream dummy spending `ScriptContext` data, and runs the CEK
  evaluator under the per-transaction execution-unit limit.
- The measured budget is returned as tx-generator `ExecutionUnits`.
- `Benchmarking.Script.Core.makePlutusContext` now honors
  `StaticScriptBudget(..., withCheck = true)` and rejects stated budgets
  that differ from pre-execution results, matching the upstream
  `WalletError` path.
- `yggdrasil-tx-generator` now depends directly on the internal
  `yggdrasil-plutus` crate; the dependency is documented in
  `docs/DEPENDENCIES.md`.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator --lib setup::plutus
cargo test -p yggdrasil-tx-generator --lib script::core
cargo test -p yggdrasil-tx-generator --lib
```

Observed result:

```text
setup::plutus: 5 passed
script::core: 29 passed
tx-generator lib: 152 passed
```

## Remaining Tx-Generator Gaps

Plutus auto-budget fitting is still open:
`plutusAutoScaleBlockfit` remains an explicit runtime boundary. Exact
`DumpToFile` rendering, Benchmark submission, and upstream binary
comparison evidence also remain.
