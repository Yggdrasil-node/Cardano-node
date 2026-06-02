---
title: "Round 761 dmq-node LocalMsgSubmission client driver (dmq-node runtime sub-arc, slice 3)"
parent: Reference
---

# Round 761 dmq-node LocalMsgSubmission client driver (dmq-node runtime sub-arc, slice 3)

Date: 2026-05-21

## Scope

Slice 3 of the dmq-node runtime/diffusion sub-arc — the
`LocalMsgSubmission` client peer driver.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_submission.rs`:

- `LocalMsgSubmissionClient` — the client driver, mirror of upstream
  `Protocol/LocalMsgSubmission/Client.hs`, following the
  `crates/network` driver pattern: a struct wrapping a
  `MessageChannel` with `submit` (send `MsgSubmitTx`, await the
  server's verdict) and `done` (`MsgDone`).
- `SubmitResult` — the `submit` outcome: `Accepted` (`MsgAcceptTx`)
  or `Rejected(SigValidationError)` (`MsgRejectTx`). A rejection is a
  protocol verdict, not a driver error, so it is an `Ok` value.
- `LocalMsgSubmissionClientError` — the driver error enum.

2 unit tests cover the `SubmitResult` accept/reject distinction and
the error-enum `Display` rendering.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 137 lib (+2 vs R760's 135) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The `LocalMsgSubmission` *server* driver; the `SigSubmission` /
  `SigSubmissionV2` inbound / outbound drivers.
- The NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`;
  the `run()` loop replacing `RunError::DiffusionWiringDeferred`.
