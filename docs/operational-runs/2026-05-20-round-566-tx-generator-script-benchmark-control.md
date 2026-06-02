---
title: "Round 566 tx-generator Script/Core Benchmark control"
parent: Reference
---

# Round 566 tx-generator Script/Core Benchmark control

Date: 2026-05-20

## Scope

This round wires upstream `Benchmarking.Script.Core.submitInEra`
`SubmitMode::Benchmark` through the R565 `GeneratorTx.walletBenchmark`
control layer. The upstream reference path is
`benchmarkTxStream -> GeneratorTx.walletBenchmark -> setEnvThreads`,
followed by `WaitBenchmark` consuming the stored
`AsyncBenchmarkControl`.

## Changes

- Added a real Rust `AsyncBenchmarkControl` carrier in `Script/Env.hs`
  mirror state, including the retained Tokio runtime, wallet benchmark
  control, shutdown callback, and cached submission summary.
- Changed `WalletBenchmarkControl::wait_summary` to wait by mutable
  reference and cache its summary, matching upstream's reusable
  `abcSummary` action shape more closely than a one-shot consuming
  method.
- Wired `SubmitMode::Benchmark` in `script/core.rs` to evaluate the
  generator, derive network magic from `Env`, launch
  `wallet_benchmark` with `LogErrors`, and store the resulting control
  with `set_env_threads`.
- Wired `wait_benchmark` / `cancel_benchmark` to the concrete control,
  preserving the existing missing-control error boundary and tracing
  `TraceBenchTxSubSummary` once the worker/feeder set completes.
- Added a script-core loopback test that starts a TxSubmission2 server,
  executes `submit_in_era` with `SubmitMode::Benchmark`, waits via
  `wait_benchmark`, and verifies submitted transactions plus summary
  accounting.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator benchmark_submit_stores_async_control_and_waits_for_summary`
- `cargo test -p yggdrasil-tx-generator generator_tx`
- `cargo clippy -p yggdrasil-tx-generator --all-targets`
- `cargo test -p yggdrasil-tx-generator`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-strict-mirror.py`
- `python dev/test/check-stale-placement.py`
- `python dev/test/filetree.py check`

## Remaining

- Extend `DumpToFile` Show rendering beyond the Allegra key-witnessed
  selftest fixture.
- Capture upstream-binary soak evidence for Benchmark scripts before
  promoting `tx-generator` out of `partial`.
