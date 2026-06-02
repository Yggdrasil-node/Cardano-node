---
title: "Round 764 dmq-node SigSubmissionV2 inbound driver (dmq-node runtime sub-arc, slice 6)"
parent: Reference
---

# Round 764 dmq-node SigSubmissionV2 inbound driver (dmq-node runtime sub-arc, slice 6)

Date: 2026-05-21

## Scope

Slice 6 of the dmq-node runtime/diffusion sub-arc — the
`SigSubmissionV2` inbound (client) peer driver.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `SigSubmissionV2Inbound` — the inbound peer driver, mirror of
  upstream `Protocol/SigSubmissionV2/Inbound.hs`. The inbound side
  requests signature identifiers and then signatures. Methods:
  `request_sig_ids` (sends `MsgRequestSigIds`, returns `Some(ids)` /
  `None` for a `MsgReplyNoSigIds`), `request_sigs` (sends
  `MsgRequestSigs`, awaits `MsgReplySigs`), `done` (`MsgDone`).
- `SigSubmissionV2InboundError` — the driver error enum.

Upstream's inbound peer is pipelined
(`SigSubmissionInboundPipelined`); the Rust port is the
non-pipelined linear driver — consistent with yggdrasil's other
mini-protocol drivers, and a correct implementation of the inbound
side's wire behaviour (pipelining is a throughput optimisation, not a
wire-format property).

1 unit test covers the error-enum `Display` rendering.

## SigSubmissionV2 — protocol + both peer drivers complete

`SigSubmissionV2` now has its full surface: count types, state
machine, transition, codec, limits, the `Collect` type, and the
inbound + outbound peer drivers.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 141 lib (+1 vs R763's 140) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`;
  the `run()` loop replacing `RunError::DiffusionWiringDeferred`.
