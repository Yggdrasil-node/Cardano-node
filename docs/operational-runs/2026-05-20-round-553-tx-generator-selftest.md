---
title: "Round 553 tx-generator selftest"
parent: Reference
---

# Round 553 tx-generator selftest

Date: 2026-05-20

## Scope

Implemented the no-output-file `selftest` command path for the
pure-Rust `tx-generator` port. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Selftest.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/data/protocol-parameters.json`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Command.hs`

## Changes

- Added `script/selftest.rs` as the strict mirror for
  `Cardano.Benchmarking.Script.Selftest`.
- Added the upstream `data/protocol-parameters.json` fixture under the
  Rust `tx-generator` crate.
- Wired `Command::Selftest(None)` through `run_selftest`, executing the
  upstream static action list in `DiscardTX` mode.
- Kept `Command::Selftest(Some(path))` on the existing shared
  `DumpToFile` boundary, because exact upstream `Show (Tx)` rendering
  has not landed yet.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator --lib script::selftest
```

Observed result:

```text
script::selftest: 3 passed
```

The focused selftest execution ran the complete static script:
1 genesis fund import, 1 + 10 + 300 SplitN transactions, and the final
4,000 NtoM DiscardTX stream.

## Remaining Tx-Generator Gaps

The selftest no-output-file path is no longer a parity blocker.
Remaining concrete gaps are Plutus pre-execution / auto-budget fitting,
script-spend integrity hashing, exact `DumpToFile` rendering, Benchmark
submission, RoundRobin / OneOf upstream-TODO error-shape parity, and
upstream binary comparison evidence.
