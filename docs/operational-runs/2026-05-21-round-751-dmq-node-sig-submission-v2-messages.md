---
title: "Round 751 dmq-node SigSubmissionV2 messages (dmq-node arc, slice 33)"
parent: Reference
---

# Round 751 dmq-node SigSubmissionV2 messages (dmq-node arc, slice 33)

Date: 2026-05-21

## Scope

Slice 33 of the dmq-node arc — the `SigSubmissionV2` mini-protocol
message enum.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `SigSubmissionV2Message` — the six protocol messages, mirror of
  upstream `Message (SigSubmissionV2 sigId sig)`:
  `MsgRequestSigIds { blocking, ack, req }`,
  `MsgReplySigIds { ids }`, `MsgReplyNoSigIds`,
  `MsgRequestSigs { ids }`, `MsgReplySigs { sigs }`, `MsgDone`. The
  `sigId` / `sig` type parameters collapse to the concrete DMQ
  `SigId` / `Sig`; `MsgReplySigIds` carries a flat `Vec<SigIdAndSize>`
  (the blocking style is tracked by the state, and `MsgReplyNoSigIds`
  is the explicit blocking-empty reply).
- `SigSubmissionV2Message::tag_name`.

1 unit test covering every message tag name.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 123 lib (+1 vs R750's 122) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `SigSubmissionV2` transitions and codec.
- The client / server protocol drivers; the `NodeKernel` /
  `Diffusion/*` run-loop wiring; the NtN / NtC protocol bundles;
  `Tracer.hs`.
