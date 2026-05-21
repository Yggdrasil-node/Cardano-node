---
title: "Round 752 dmq-node SigSubmissionV2 transitions (dmq-node arc, slice 34)"
parent: Reference
---

# Round 752 dmq-node SigSubmissionV2 transitions (dmq-node arc, slice 34)

Date: 2026-05-21

## Scope

Slice 34 of the dmq-node arc — the `SigSubmissionV2` state-machine
transition validation.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `SigSubmissionV2TransitionError` — an illegal-transition error.
- `SigSubmissionV2State::transition` — the next state after an
  incoming message, mirror of the upstream `SigSubmissionV2`
  `Message` transitions: `StIdle`+`MsgRequestSigIds`→`StSigIds`,
  `StSigIds`+`MsgReplySigIds`→`StIdle`, blocking
  `StSigIds`+`MsgReplyNoSigIds`→`StIdle`,
  `StIdle`+`MsgRequestSigs`→`StSigs`,
  `StSigs`+`MsgReplySigs`→`StIdle`, `StIdle`+`MsgDone`→`StDone`.
  `MsgReplyNoSigIds` is accepted only from a *blocking* `StSigIds`.

2 unit tests: the legal happy-path walk (both the identifier and
signature exchanges, plus `MsgReplyNoSigIds`) and illegal-message
rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 125 lib (+2 vs R751's 123) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `SigSubmissionV2` message codec.
- The client / server protocol drivers; the `NodeKernel` /
  `Diffusion/*` run-loop wiring; the NtN / NtC protocol bundles;
  `Tracer.hs`.
