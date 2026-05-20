---
title: "Round 565 tx-generator walletBenchmark control"
parent: Reference
---

# Round 565 tx-generator walletBenchmark control

Date: 2026-05-20

## Scope

This round mirrors the orchestration layer from upstream
`Cardano.Benchmarking.GeneratorTx.walletBenchmark` after R561-R564
landed the TPS throttle, submission reports, stream source,
submission-client state machine, and TxSubmission2 wire driver.

## Changes

- Added `wallet_benchmark` to the `GeneratorTx.hs` mirror
  (`generator_tx.rs`).
- Added upstream-shaped IPv4 target resolution via `lookup_node_address`.
- Added the V14 initiator-only node-to-node version proposal used by
  upstream `benchmarkConnectTxSubmit`.
- Spawns one TxSubmission2 worker per target, plus a feeder that runs
  `start_sending` and then sends the throttle stop marker.
- Added `WalletBenchmarkControl` with shutdown and summary collection
  over the report refs.
- Added a real peer-accept/peer-connect loopback test that negotiates
  NtN V14 and submits generated transactions through TxSubmission2.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator generator_tx`
- `cargo clippy -p yggdrasil-tx-generator --all-targets`
- `cargo test -p yggdrasil-tx-generator selftest_command_dispatches_to_static_script`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-strict-mirror.py`
- `python scripts/check-stale-placement.py`
- `python .claude/scripts/filetree.py check`

## Remaining

- Wire `Script/Core.hs` `SubmitMode::Benchmark` to this control and
  carry real `AsyncBenchmarkControl` through `Script/Env.hs`.
- Extend `DumpToFile` Show rendering beyond the Allegra key-witnessed
  selftest fixture and capture upstream binary soak evidence.
