---
title: "Round 763 dmq-node SigSubmissionV2 outbound driver (dmq-node runtime sub-arc, slice 5)"
parent: Reference
---

# Round 763 dmq-node SigSubmissionV2 outbound driver (dmq-node runtime sub-arc, slice 5)

Date: 2026-05-21

## Scope

Slice 5 of the dmq-node runtime/diffusion sub-arc — the
`SigSubmissionV2` outbound (server) peer driver.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `SigSubmissionV2Outbound` — the outbound peer driver, mirror of
  upstream `Protocol/SigSubmissionV2/Outbound.hs`
  (`sigSubmissionV2OutboundPeer`). The outbound side submits
  signatures: it answers the inbound side's requests. Methods:
  `recv_request` (awaits `MsgRequestSigIds` / `MsgRequestSigs` /
  `MsgDone`), `reply_sig_ids`, `reply_no_sig_ids`, `reply_sigs`.
- `SigSubmissionV2Request` — a Rust-idiomatic flattening of upstream
  `OutboundStIdle`'s continuation callbacks (`SigIds` / `Sigs` /
  `Done`).
- `SigSubmissionV2OutboundError` — the driver error enum.

2 unit tests cover the request-enum variants and the error-enum
`Display` rendering.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 140 lib (+2 vs R762's 138) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The `SigSubmissionV2` inbound peer driver (the pipelined
  identifier / signature request side).
- The NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`;
  the `run()` loop replacing `RunError::DiffusionWiringDeferred`.
