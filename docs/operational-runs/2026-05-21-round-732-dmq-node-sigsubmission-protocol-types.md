---
title: "Round 732 dmq-node SigSubmission protocol types (dmq-node arc, slice 14)"
parent: Reference
---

# Round 732 dmq-node SigSubmission protocol types (dmq-node arc, slice 14)

Date: 2026-05-21

## Scope

Slice 14 of the dmq-node arc — opens the dmq-node-local
`SigSubmission` mini-protocol (state machine + messages).

## Decision

R731 found `crates/network`'s `TxSubmissionMessage` is concrete
(`TxId` / `Vec<u8>`), not generic, while upstream
`SigSubmission crypto = TxSubmission2 SigId (Sig crypto)` depends on
`TxSubmission2` being generic. The architectural fork — refactor
`crates/network`'s core `TxSubmission2` to generic, or give dmq-node
its own protocol module — was resolved (advisor-confirmed) in favour
of the **dmq-node-local** protocol: the wire format is identical
either way, so a generic refactor of the core network crate buys
zero wire-parity bytes and risks the node's tx-submission. The
dmq-local protocol is isolated to the sister tool — consistent with
yggdrasil's accepted concrete-vs-parameterized simplifications (the
unified `Block`, the db-analyser `block/` collapse).

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigSubmissionState` — the protocol state machine (`StInit`,
  `StIdle`, `StTxIds { blocking }`, `StTxs`, `StDone`), mirroring
  `crates/network`'s `TxSubmissionState`.
- `SigIdAndSize` — a `SigId` paired with its serialized size (mirror
  of `TxIdAndSize`).
- `SigSubmissionMessage` — the six protocol messages (`MsgInit`,
  `MsgRequestTxIds`, `MsgReplyTxIds`, `MsgRequestTxs`, `MsgReplyTxs`,
  `MsgDone`) with `SigId` identifiers and `Sig` payloads.
- `SigSubmissionMessage::wire_tag` — the CBOR envelope tags
  (`6`/`0`/`1`/`2`/`3`/`4`), byte-identical to `crates/network`'s
  `TxSubmissionMessage::wire_tag` (the wire-equivalence guarantee).

2 unit tests: the envelope-tag mapping and state/message
construction.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 82 lib (+2 vs R731's 80) +
  2 golden, all green.

## Remaining (dmq-node SigSubmission protocol)

- Slice 2 — `SigSubmissionState` transition validation.
- Slice 3+ — the message codec (the envelope plus the `SigId` / `Sig`
  payloads), then the client / server drivers and the
  `timeLimits` / `byteLimits` tables.
