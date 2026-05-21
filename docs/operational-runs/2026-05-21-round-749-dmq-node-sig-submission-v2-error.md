---
title: "Round 749 dmq-node SigSubmissionV2 protocol error (dmq-node arc, slice 31)"
parent: Reference
---

# Round 749 dmq-node SigSubmissionV2 protocol error (dmq-node arc, slice 31)

Date: 2026-05-21

## Scope

Slice 31 of the dmq-node arc — opens the `sig_submission_v2` module
with the V2 peer-protocol-violation enum.

## What shipped

`crates/tools/dmq-node/src/sig_submission_v2.rs` — new file. Ports
`SigSubmissionV2/Types.hs`:

- `SigSubmissionProtocolError` — the 8-variant peer-misbehaviour
  enum (`AckedTooManySigIds`, `RequestedNothing`,
  `RequestedTooManySigIds`, `RequestBlocking`, `RequestNonBlocking`,
  `RequestedUnavailableSig`, `SigIdsNotRequested`,
  `SigNotRequested`), a `thiserror::Error` reproducing upstream's
  `displayException` strings. The `RequestedTooManySigIds` count
  fields are `u16` (upstream's `NumIdsReq` / `NumIdsAck` `Word16`
  newtypes arrive with the `SigSubmissionV2` protocol-type slice).

`lib.rs` gains `pub mod sig_submission_v2;`.

2 unit tests: the field-less variants' upstream messages, and the
`RequestedTooManySigIds` count formatting.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 120 lib (+2 vs R748's 118) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `SigSubmissionV2` protocol (`Protocol/SigSubmissionV2/*`) and
  its `Inbound` / `Outbound` driver halves.
- The client / server protocol drivers; the `NodeKernel` /
  `Diffusion/*` run-loop wiring; the NtN / NtC protocol bundles;
  `Tracer.hs`.
