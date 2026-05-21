---
title: "Round 760 dmq-node LocalMsgNotification server driver (dmq-node runtime sub-arc, slice 2)"
parent: Reference
---

# Round 760 dmq-node LocalMsgNotification server driver (dmq-node runtime sub-arc, slice 2)

Date: 2026-05-21

## Scope

Slice 2 of the dmq-node runtime/diffusion sub-arc — the
`LocalMsgNotification` server-side peer driver.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_notification.rs`:

- `LocalMsgNotificationServer` — the server driver, mirror of
  upstream `Protocol/LocalMsgNotification/Server.hs`, following the
  `crates/network` driver pattern (`keepalive_server.rs`): a struct
  wrapping a `MessageChannel` with typed methods — `recv_request`
  (awaits the client's `MsgRequest` / `MsgClientDone`, returning
  `Some(blocking)` / `None`) and `reply` (sends `MsgReply`).
  `send_msg` / `recv_msg` thread the protocol state machine.
- `LocalMsgNotificationServerError` — the server driver error enum.

The `reply` content is supplied by the caller (the notification
source is the NodeKernel's queue — wired at integration time), so
this slice ships the driver primitives, not a full `serve_loop`.

2 unit tests cover the error-enum `Display` rendering.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 135 lib (+2 vs R759's 133) +
  2 golden, all green.

## LocalMsgNotification — protocol + both drivers complete

`LocalMsgNotification` now has its full surface: types, transition,
codec, and the client + server peer drivers.

## Remaining (dmq-node runtime sub-arc)

- The `LocalMsgSubmission` client / server drivers; the
  `SigSubmission` / `SigSubmissionV2` inbound / outbound drivers.
- The NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`;
  the `run()` loop replacing `RunError::DiffusionWiringDeferred`.
