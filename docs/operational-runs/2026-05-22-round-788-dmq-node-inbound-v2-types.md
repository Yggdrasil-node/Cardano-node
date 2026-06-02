---
title: "Round 788 dmq-node inbound-V2 foundational types"
parent: Reference
---

# Round 788 dmq-node inbound-V2 foundational types

Date: 2026-05-22

## Scope

dmq-node — opens `inbound_v2.rs`, the port of upstream
`Ouroboros.Network.TxSubmission.Inbound.V2.Types` (the inbound
tx-submission governor types the DMQ `NodeKernel` holds).

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs` — new file:

- `TxSubmissionLogicVersion` — `V1` / `V2`, mirror of upstream
  `data TxSubmissionLogicVersion`, with `ALL`.
- `ProcessedTxCount` — accepted / rejected counts and the resulting
  peer score, mirror of upstream `data ProcessedTxCount`
  (`PartialEq` only — `ptxc_score` is `f64`).
- `TxSubmissionInitDelay` — an optional tx-submission start delay,
  mirror of upstream `data TxSubmissionInitDelay`, with
  `DEFAULT_TX_SUBMISSION_INIT_DELAY` (60 s, mirror of
  `defaultTxSubmissionInitDelay`).

dmq-node carries its own copy (the R732 dmq-node-local decision —
`crates/consensus`'s inbound governor is concrete over ledger
transactions, so it cannot be reused for `SigId` / `Sig`). This slice
ports the foundational standalone types; `PeerTxState`,
`SharedTxState`, the `TxDecision` record, and the governor logic land
in subsequent rounds.

`lib.rs` gains `pub mod inbound_v2;`.

3 unit tests cover the version ordering, `ProcessedTxCount`
construction, and the default init delay.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 161 lib (+3 vs R787's 158) +
  2 golden, all green.
