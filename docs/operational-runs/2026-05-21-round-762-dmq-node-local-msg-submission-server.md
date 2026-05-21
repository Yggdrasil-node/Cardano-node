---
title: "Round 762 dmq-node LocalMsgSubmission server driver (dmq-node runtime sub-arc, slice 4)"
parent: Reference
---

# Round 762 dmq-node LocalMsgSubmission server driver (dmq-node runtime sub-arc, slice 4)

Date: 2026-05-21

## Scope

Slice 4 of the dmq-node runtime/diffusion sub-arc — the
`LocalMsgSubmission` server peer driver.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_submission.rs`:

- `LocalMsgSubmissionServer` — the server driver, mirror of upstream
  `Protocol/LocalMsgSubmission/Server.hs`, following the
  `crates/network` driver pattern: a struct wrapping a
  `MessageChannel` with `recv_submission` (awaits the client's
  `MsgSubmitTx` / `MsgDone`, returning `Some(sig)` / `None`),
  `accept` (`MsgAcceptTx`) and `reject` (`MsgRejectTx` with a
  `SigValidationError` reason).
- `LocalMsgSubmissionServerError` — the server driver error enum.

1 unit test covers the error-enum `Display` rendering.

## LocalMsgSubmission — protocol + both drivers complete

`LocalMsgSubmission` now has its full surface: types, transition,
codec (incl. the reject codec), and the client + server peer drivers.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 138 lib (+1 vs R761's 137) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The `SigSubmission` / `SigSubmissionV2` inbound / outbound drivers.
- The NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`;
  the `run()` loop replacing `RunError::DiffusionWiringDeferred`.
