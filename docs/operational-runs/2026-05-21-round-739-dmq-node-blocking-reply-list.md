---
title: "Round 739 dmq-node BlockingReplyList (dmq-node arc, slice 21)"
parent: Reference
---

# Round 739 dmq-node BlockingReplyList (dmq-node arc, slice 21)

Date: 2026-05-21

## Scope

Slice 21 of the dmq-node arc. Corrects the `LocalMsgNotification`
`MsgReply` model ahead of the codec slice.

## Why

R738 modelled `MsgReply` as `{ messages: Vec<Sig>, has_more }`. The
upstream `LocalMsgNotification/Codec.hs` review (R739) showed the
reply payload is a `BlockingReplyList blocking msg` — and the
blocking style drives two distinct wire encodings: a non-blocking
reply is `[1, [*msg], hasMore]`, a blocking reply is `[2, [*msg]]`
(no `hasMore` — upstream's documented "Issue #15"). The flat
`Vec<Sig>` could not express that.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_notification.rs`:

- `BlockingReplyList` — a 2-variant enum (`Blocking(Vec<Sig>)` /
  `NonBlocking(Vec<Sig>)`), mirror of upstream's `BlockingReplyList`
  GADT (the `blocking` type parameter flattened to the variant), plus
  a `messages` accessor.
- `LocalMsgNotificationMessage::MsgReply` now carries
  `reply: BlockingReplyList` instead of `messages: Vec<Sig>`; the
  `has_more` field is retained (a blocking reply carries it in the
  type but does not encode it).

1 new unit test; the two R738 `MsgReply` test sites updated.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 98 lib (+1 vs R738's 97) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 22 — the `LocalMsgNotification` message codec (the
  `[0,bool]` / `[1,[*msg],hasMore]` / `[2,[*msg]]` / `[3]` wire
  format, with indefinite-length message arrays).
- The client / server protocol drivers; the `Diffusion/*` wiring.
