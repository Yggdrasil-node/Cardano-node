---
title: "Round 740 dmq-node LocalMsgNotification codec (dmq-node arc, slice 22)"
parent: Reference
---

# Round 740 dmq-node LocalMsgNotification codec (dmq-node arc, slice 22)

Date: 2026-05-21

## Scope

Slice 22 of the dmq-node arc. Adds the `LocalMsgNotification`
message CBOR codec.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_notification.rs`:

- `LocalMsgNotificationMessage::to_cbor` / `from_cbor` — mirror of
  upstream `LocalMsgNotification/Codec.hs`:
  - `MsgRequest` is `[0, blocking]`
  - `MsgReply` non-blocking is `[1, <indef [msg]>, hasMore]`
  - `MsgReply` blocking is `[2, <indef [msg]>]` — **no `hasMore`**
    (the upstream documented "Issue #15"); it decodes as
    `HasMore::DoesNotHaveMore`
  - `MsgClientDone` is `[3]`
  The message list is a CBOR *indefinite*-length array
  (`array_indef` / `break_stop`); each message uses the R724
  `encode_sig` / `decode_sig`.
- `decode_indef_sigs` — decodes the indefinite-length message array,
  with `LOCAL_MSG_NOTIFICATION_LIST_MAX` as an anti-DoS cap.

2 unit tests: round-trip of every message shape, and the blocking
reply's `hasMore`-less encoding (a `Blocking` reply built with
`HasMore` decodes as `DoesNotHaveMore`).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 100 lib (+2 vs R739's 98) +
  2 golden, all green.

## dmq-node mini-protocols — codecs complete

All three dmq-node mini-protocols now have their full type +
transition + CBOR codec surface: `SigSubmission` (R717-R735),
`LocalMsgSubmission` (R736-R737), `LocalMsgNotification`
(R738-R740). The remaining dmq-node work is the client / server
protocol drivers and the `Diffusion/*` run-loop wiring.
