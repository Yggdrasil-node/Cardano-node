---
title: "Round 733 dmq-node SigSubmission transition validation (dmq-node arc, slice 15)"
parent: Reference
---

# Round 733 dmq-node SigSubmission transition validation (dmq-node arc, slice 15)

Date: 2026-05-21

## Scope

Slice 15 of the dmq-node arc (SigSubmission protocol slice 2). Adds
the `SigSubmission` state-machine transition validation.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigSubmissionMessage::tag_name` — the human-readable message tag
  used in transition-error messages.
- `SigSubmissionTransitionError` — a `thiserror::Error` for an
  illegal transition (mirror of `crates/network`'s
  `TxSubmissionTransitionError`).
- `SigSubmissionState::transition` — the next state after an incoming
  message, or a `SigSubmissionTransitionError` for an illegal one.
  Mirror of `crates/network`'s `TxSubmissionState::transition`: the
  `SigSubmission` protocol *is* `TxSubmission2`, so the transition
  table is identical (`StInit`+`MsgInit`→`StIdle`,
  `StIdle`+`MsgRequestTxIds`→`StTxIds`, `StIdle`+`MsgRequestTxs`→
  `StTxs`, `StTxIds`+`MsgReplyTxIds`→`StIdle`, blocking
  `StTxIds`+`MsgDone`→`StDone`, `StTxs`+`MsgReplyTxs`→`StIdle`).

2 unit tests: the legal happy-path walk through every state, and
rejection of illegal messages (`MsgDone` from a non-blocking
`StTxIds`, a reply in `StIdle`).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 84 lib (+2 vs R732's 82) +
  2 golden, all green.

## Remaining (dmq-node SigSubmission protocol)

- Slice 3+ — the message codec (the CBOR envelope plus the
  `SigId` / `Sig` payloads), then the client / server drivers and the
  `timeLimits` / `byteLimits` tables.
