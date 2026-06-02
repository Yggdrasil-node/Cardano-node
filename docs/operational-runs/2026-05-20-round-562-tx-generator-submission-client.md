---
title: "Round 562 tx-generator LogTypes and SubmissionClient"
parent: Reference
---

# Round 562 tx-generator LogTypes and SubmissionClient

Date: 2026-05-20

## Scope

Continued the Benchmark submission arc after the R561 throttle
foundation. This round mirrors:

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/LogTypes.hs`
- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/GeneratorTx/SubmissionClient.hs`

The slice ports the pure request/response bookkeeping that can be
verified without an active node-to-node socket. Actual peer connection
orchestration remains in the `GeneratorTx.Submission` /
`walletBenchmark` follow-up.

## Changes

- Added `benchmarking/log_types.rs` with upstream-shaped
  `TraceBenchTxSubmit`, `SubmissionSummary`, and
  `NodeToNodeSubmissionTrace` carriers.
- Added `generator_tx/submission_client.rs` with
  `SubmissionThreadStats`, `BlockingStyle`, `TxSource`, finite
  `VecTxSource`, and `SubmissionClientState`.
- Ported upstream `requestTxIds` behavior for blocking/non-blocking
  requests, acknowledgement-window trimming, new tx-id announcements,
  `SendMsgDone` decisions, and the blocking ack mismatch error shape.
- Ported upstream `requestTxs` behavior for outstanding transaction
  lookup, unavailable-id accounting, tx-body replies, and sent /
  unavailable counters.

## Validation

Focused validation:

```text
cargo fmt --all -- --check
cargo test -p yggdrasil-tx-generator submission
cargo clippy -p yggdrasil-tx-generator --all-targets
```

Workspace validation:

```text
cargo fmt --all -- --check
cargo check-all
cargo lint
cargo test-all
python dev/test/check-parity-matrix.py
python dev/test/filetree.py check
```

Observed result: all commands passed locally.

## Remaining Tx-Generator Gaps

The Benchmark path still needs the `GeneratorTx.Submission` scheduler
and `walletBenchmark` node-to-node socket wiring around the new
throttle and submission-client state machine, followed by live
comparison against the vendored upstream `tx-generator` binary.
