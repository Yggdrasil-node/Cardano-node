---
title: "Round 563 tx-generator GeneratorTx.Submission"
parent: Reference
---

# Round 563 tx-generator GeneratorTx.Submission

Date: 2026-05-20

## Scope

Round 563 mirrors
`.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx/Submission.hs`
after the R561 `Cardano.Benchmarking.Types` / `TpsThrottle` and R562
`GeneratorTx.SubmissionClient` slices.

## Changes

- Added `crates/tools/tx-generator/src/generator_tx/submission.rs`.
- Ported upstream-shaped `SubmissionParams`, `ReportRef`,
  `SubmissionThreadReport`, `submitThreadReport`,
  `submitSubmissionThreadStats`, and `mkSubmissionSummary`.
- Ported `StreamState`, `SharedTxStream`, and `txStreamSource` over
  the R561 TPS throttle plus the R562 `TxSource` submission boundary.
- Updated tx-generator parity docs and the parity matrix to move the
  next milestone to `walletBenchmark` scheduler/network wiring.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `cargo test -p yggdrasil-tx-generator submission`
- `cargo clippy -p yggdrasil-tx-generator --all-targets`
- `python scripts/check-parity-matrix.py`
- `python .claude/scripts/filetree.py check`

## Remaining

- Wire `walletBenchmark` node-to-node scheduler/network behavior around
  the throttle, submission-client, and submission state machine.
- Extend exact `DumpToFile` `Show` rendering beyond the Allegra
  key-witnessed selftest fixture.
- Capture end-to-end soak evidence against the upstream tx-generator
  binary before promoting the tool beyond `partial`.
