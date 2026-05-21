---
title: "Round 738 dmq-node LocalMsgNotification protocol (dmq-node arc, slice 20)"
parent: Reference
---

# Round 738 dmq-node LocalMsgNotification protocol (dmq-node arc, slice 20)

Date: 2026-05-21

## Scope

Slice 20 of the dmq-node arc — the `LocalMsgNotification`
node-to-client mini-protocol types.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_notification.rs` — new
file. Unlike `SigSubmission` / `LocalMsgSubmission`,
`LocalMsgNotification` is DMQ's *own* node-to-client protocol — the
server pushes newly-diffused signatures to a local client:

- `HasMore` — `HasMore` / `DoesNotHaveMore` (whether the server has
  further messages).
- `LocalMsgNotificationState` — `StIdle` / `StBusy { blocking }` /
  `StDone`, mirror of upstream's `type data LocalMsgNotification`.
- `LocalMsgNotificationMessage` — `MsgRequest { blocking }`,
  `MsgReply { messages, has_more }`, `MsgClientDone`, with
  `tag_name`. The `msg` type parameter is instantiated to `Sig`.
- `LocalMsgNotificationTransitionError` + `transition` — the
  state-machine transitions (`StIdle`+`MsgRequest`→`StBusy`,
  `StBusy`+`MsgReply`→`StIdle`, `StIdle`+`MsgClientDone`→`StDone`).

`protocol.rs` gains `pub mod local_msg_notification;`.

3 unit tests: the legal transition walk, illegal-message rejection,
and `HasMore` distinctness.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 97 lib (+3 vs R737's 94) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 21 — the `LocalMsgNotification` message codec.
- The client / server protocol drivers; the `Diffusion/*` wiring.
