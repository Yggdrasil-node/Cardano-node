---
title: "Round 750 dmq-node SigSubmissionV2 protocol state (dmq-node arc, slice 32)"
parent: Reference
---

# Round 750 dmq-node SigSubmissionV2 protocol state (dmq-node arc, slice 32)

Date: 2026-05-21

## Scope

Slice 32 of the dmq-node arc — opens the `SigSubmissionV2`
mini-protocol module with its count newtypes and state machine.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs` — new file.
Ports the `Protocol/SigSubmissionV2/Type.hs` foundation:

- `NumIdsAck`, `NumIdsReq`, `NumReq`, `NumUnacknowledged` — the four
  `Word16` count newtypes.
- `SigSubmissionV2State` — the protocol state machine (`StIdle`,
  `StSigIds { blocking }`, `StSigs`, `StDone`).

`SigSubmissionV2` is based on upstream's
`Ouroboros.Network.Protocol.ObjectDiffusion` mini-protocol
(originally designed for Peras) — a pull-based protocol where the
inbound side requests signature identifiers and then signatures.
`protocol.rs` gains `pub mod sig_submission_v2;`.

2 unit tests covering the count newtypes and the state variants.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 122 lib (+2 vs R749's 120) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `SigSubmissionV2` message enum, transitions, and codec.
- The client / server protocol drivers; the `NodeKernel` /
  `Diffusion/*` run-loop wiring; the NtN / NtC protocol bundles;
  `Tracer.hs`.
