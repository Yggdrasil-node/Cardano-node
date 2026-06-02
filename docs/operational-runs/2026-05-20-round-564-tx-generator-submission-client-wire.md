---
title: "Round 564 tx-generator SubmissionClient wire driver"
parent: Reference
---

# Round 564 tx-generator SubmissionClient wire driver

Date: 2026-05-20

## Scope

This round mirrors the runtime handoff from upstream
`GeneratorTx/SubmissionClient.hs` into the TxSubmission2 mini-protocol
runner used by `GeneratorTx.hs` and `GeneratorTx/NodeToNode.hs`.

## Changes

- Added `run_tx_submission_client` to
  `generator_tx/submission_client.rs`.
- Translates `yggdrasil_network::TxServerRequest` values into the
  upstream-shaped `request_tx_ids` / `request_txs` state transitions.
- Sends TxSubmission2 tx-id replies, tx-body replies, and `MsgDone`
  through the typed network `TxSubmissionClient`.
- Added a muxed TCP loopback test with `TxSubmissionServer` covering
  `MsgInit -> RequestTxIds -> RequestTxs -> MsgDone`.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator submission_client`
- `cargo clippy -p yggdrasil-tx-generator --all-targets`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-strict-mirror.py`
- `python dev/test/check-stale-placement.py`
- `python dev/test/filetree.py check`

## Remaining

- Wire `walletBenchmark` target resolution, connect/spawn, and
  feeder-summary orchestration around the throttle/submission-client/
  submission/wire-driver stack.
- Extend `DumpToFile` Show rendering beyond the Allegra key-witnessed
  selftest fixture and capture upstream binary soak evidence.
