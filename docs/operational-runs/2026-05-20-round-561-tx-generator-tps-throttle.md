---
title: "Round 561 tx-generator Benchmarking.Types and TpsThrottle"
parent: Reference
---

# Round 561 tx-generator Benchmarking.Types and TpsThrottle

Date: 2026-05-20

## Scope

Opened the Benchmark submission foundation after the selftest
`DumpToFile` parity slice. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Types.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/TpsThrottle.hs`

The implementation is intentionally limited to the shared types and
throttle behavior needed before wiring
`Cardano.Benchmarking.GeneratorTx.SubmissionClient`,
`Cardano.Benchmarking.GeneratorTx.Submission`, and `walletBenchmark`.

## Changes

- Added `crate::benchmarking` as the namespace for upstream
  `Cardano.Benchmarking.*` leaves whose basenames would collide with
  existing `Cardano.TxGenerator.*` mirrors.
- Ported `ToAnnce`, `UnAcked`, `Ack`, `Req`, `Sent`, `Unav`, and
  `SubmissionErrorPolicy` in `benchmarking/types.rs`.
- Ported the `TpsThrottle` watermark model in
  `benchmarking/tps_throttle.rs` with blocking and non-blocking
  consumers, one-token non-blocking consumption, buffered watermark
  growth, and `sendStop` semantics that wait for the slot to drain
  before publishing stop.

## Validation

Focused validation:

```text
cargo test -p yggdrasil-tx-generator benchmarking
cargo test -p yggdrasil-tx-generator
cargo clippy -p yggdrasil-tx-generator --all-targets
```

Workspace validation:

```text
cargo fmt --all -- --check
cargo check-all
cargo lint
cargo test-all
python scripts/check-parity-matrix.py
python .claude/scripts/filetree.py check
```

Observed result: all commands passed locally.

## Remaining Tx-Generator Gaps

The Benchmark path still needs strict-mirror ports for
`GeneratorTx.SubmissionClient`, `GeneratorTx.Submission`, and
`walletBenchmark`, followed by live node-to-client submission parity
checks against the vendored upstream `tx-generator` binary.
