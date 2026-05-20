---
title: "Round 554 tx-generator RoundRobin and OneOf"
parent: Reference
---

# Round 554 tx-generator RoundRobin and OneOf

Date: 2026-05-20

## Scope

Closed the `RoundRobin` / `OneOf` runtime parity gap as an
upstream-TODO error-shape slice. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Types.hs`

## Upstream Behavior

Upstream `Core.hs` still intentionally crashes for both constructors:

- `RoundRobin` errors with `return $ foldr1 Streaming.interleaves gList`.
- `OneOf` errors with `todo: implement Quickcheck style oneOf generator`.

These are not yggdrasil implementation stalls. The correct parity
behavior for the 11.0.1 target is to preserve the same unsupported
runtime shape while continuing to parse and serialize both constructors.

## Changes

- Updated `script/core.rs` so `Generator::RoundRobin` returns the exact
  upstream TODO error text.
- Updated `script/core.rs` so `Generator::OneOf` returns the exact
  upstream TODO error text.
- Added focused tests pinning both messages through `submit_in_era`.
- Removed `RoundRobin` / `OneOf` from the remaining tx-generator
  implementation blocker list.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator --lib script::core
```

Observed result:

```text
script::core: 27 passed
```

## Remaining Tx-Generator Gaps

Remaining concrete gaps are Plutus pre-execution / auto-budget fitting,
script-spend integrity hashing, exact `DumpToFile` rendering, Benchmark
submission, and upstream binary comparison evidence.
