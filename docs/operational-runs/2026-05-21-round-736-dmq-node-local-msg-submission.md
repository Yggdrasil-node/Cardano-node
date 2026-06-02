---
title: "Round 736 dmq-node LocalMsgSubmission protocol (dmq-node arc, slice 18)"
parent: Reference
---

# Round 736 dmq-node LocalMsgSubmission protocol (dmq-node arc, slice 18)

Date: 2026-05-21

## Scope

Slice 18 of the dmq-node arc — the `LocalMsgSubmission`
node-to-client mini-protocol types.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_submission.rs` — new
file. Upstream `type LocalMsgSubmission sig = LocalTxSubmission sig
SigValidationError`, so the protocol *is* `LocalTxSubmission`; the
port mirrors `crates/network`'s `LocalTxSubmission` with a `Sig`
payload and a typed `SigValidationError` rejection (the same
dmq-node-local pattern as `SigSubmission`, R731 / R732 decision):

- `LocalMsgSubmissionState` — `StIdle` / `StBusy` / `StDone`.
- `LocalMsgSubmissionMessage` — `MsgSubmitTx { sig }`, `MsgAcceptTx`,
  `MsgRejectTx { reason }`, `MsgDone`, with `wire_tag` (envelope tags
  `0`/`1`/`2`/`3`, byte-identical to `crates/network`'s
  `LocalTxSubmissionMessage`) and `tag_name`.
- `LocalMsgSubmissionTransitionError` + `transition` — mirror of
  `crates/network`'s `LocalTxSubmissionState::transition`.

`protocol.rs` gains `pub mod local_msg_submission;`.

3 unit tests: envelope-tag mapping, the legal transition walk, and
illegal-message rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 92 lib (+3 vs R735's 89) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 19 — the `LocalMsgSubmission` message codec.
- The `LocalMsgNotification` mini-protocol; the client / server
  protocol drivers; the `Diffusion/*` wiring.
