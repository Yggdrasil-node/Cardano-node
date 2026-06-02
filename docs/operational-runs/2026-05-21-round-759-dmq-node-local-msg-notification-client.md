---
title: "Round 759 dmq-node LocalMsgNotification client driver (dmq-node runtime sub-arc, slice 1)"
parent: Reference
---

# Round 759 dmq-node LocalMsgNotification client driver (dmq-node runtime sub-arc, slice 1)

Date: 2026-05-21

## Scope

Slice 1 of the dmq-node runtime/diffusion sub-arc (scoped at R758) —
the first per-protocol peer driver.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_notification.rs`:

- `LocalMsgNotificationClient` — the `LocalMsgNotification` client
  driver, mirror of upstream `Protocol/LocalMsgNotification/Client.hs`
  (`localMsgNotificationClientPeer`). Follows the `crates/network`
  mini-protocol-driver pattern (`keepalive_client.rs`): a struct
  wrapping a `yggdrasil_network::MessageChannel` plus typed protocol
  methods — `request` (send `MsgRequest`, await `MsgReply`) and `done`
  (`MsgClientDone`). `send_msg` / `recv_msg` thread the
  `LocalMsgNotificationState` machine through every message via
  `transition`.
- `LocalMsgNotificationClientError` — the driver error enum
  (`Mux` / `ConnectionClosed` / `Protocol` / `Decode` /
  `UnexpectedMessage`), mirroring the `crates/network` per-driver
  error enums.

3 unit tests cover the error-enum `Display` rendering — the
established `crates/network` driver test bar (`keepalive_client.rs`).
The driver's protocol-I/O methods are exercised over the mux at
integration time.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 133 lib (+3 vs R758's 130) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The `LocalMsgNotification` *server* driver; the `LocalMsgSubmission`
  client / server drivers; the `SigSubmission` / `SigSubmissionV2`
  inbound / outbound drivers.
- The NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`;
  the `run()` loop replacing `RunError::DiffusionWiringDeferred`.
