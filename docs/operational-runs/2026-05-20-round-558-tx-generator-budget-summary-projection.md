---
title: "Round 558 tx-generator budget-summary projection"
parent: Reference
---

# Round 558 tx-generator budget-summary projection

Date: 2026-05-20

## Scope

Closed the `previewNtoMTransaction` follow-up left after the R557
AutoScript budget-fitting slice. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`

## Changes

- `Benchmarking.Script.Core.previewNtoMTransaction` now computes the
  serialized preview transaction size once and reuses it for tracing
  and summary projection.
- Successful `NtoM` previews now calculate a projected transaction fee
  from the current ledger protocol parameters, serialized transaction
  size, and any script execution units carried by the generated
  preview transaction.
- The fee trace now uses the upstream-shaped Haskell `Maybe Coin`
  rendering (`Nothing` or `Just (Coin n)`) instead of always logging
  `Nothing`.
- When an AutoScript budget summary is present, the preview updates
  `projectedTxSize` and `projectedTxFee` before refreshing
  `plutus-budget-summary.json`.
- Unit tests cover the upstream-shaped fee trace text, summary JSON
  projection fields, and cleanup of the budget-summary file produced by
  the existing script-spend and selftest `NtoM` runtime paths.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator script::core --lib
cargo test -p yggdrasil-tx-generator script::selftest --lib
cargo check -p yggdrasil-tx-generator
cargo clippy -p yggdrasil-tx-generator --all-targets -- -D warnings
cargo fmt --all -- --check
python .claude/scripts/filetree.py check
```

Workspace closeout gates:

```text
cargo check-all
cargo lint
cargo test-all
python scripts/check-parity-matrix.py
python scripts/check-strict-mirror.py
```

Observed result:

```text
script::core: 31 passed
script::selftest: 3 passed
yggdrasil-tx-generator check: passed
yggdrasil-tx-generator clippy: passed
workspace cargo gates: passed
parity matrix: 22 entries validated
strict mirror: 0 violations
filetree check: clean
plutus-budget-summary.json test artifact: absent after full test-all
```

## Remaining Tx-Generator Gaps

Exact `DumpToFile` rendering, Benchmark submission, and upstream binary
comparison evidence remain open.
