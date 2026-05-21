---
title: "Round 753 dmq-node SigSubmissionV2 codec (dmq-node arc, slice 35)"
parent: Reference
---

# Round 753 dmq-node SigSubmissionV2 codec (dmq-node arc, slice 35)

Date: 2026-05-21

## Scope

Slice 35 of the dmq-node arc — the `SigSubmissionV2` message CBOR
codec.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission_v2.rs`:

- `SigSubmissionV2Message::wire_tag` — the envelope tags
  (`1`/`2`/`3`/`4`/`5`/`6`).
- `SigSubmissionV2Message::to_cbor` / `from_cbor` — mirror of upstream
  `encodeSigSubmissionV2` / `decodeSigSubmissionV2`:
  `MsgRequestSigIds` is `[1, blocking, ack, req]`, `MsgReplySigIds` is
  `[2, <indef [[sigId, size]]>]`, `MsgReplyNoSigIds` is `[3]`,
  `MsgRequestSigs` is `[4, <indef [sigId]>]`, `MsgReplySigs` is
  `[5, <indef [sig]>]`, `MsgDone` is `[6]`. The lists are CBOR
  indefinite-length arrays; `SigId` / `Sig` payloads use the R722 /
  R724 codecs.
- `decode_indef` — a generic indefinite-length-array decoder with an
  anti-DoS element cap.

The blocking / non-blocking distinction of `MsgReplySigIds` is a
protocol-state property (enforced by `transition`), so the decoded
message simply carries the identifier list.

2 unit tests: round-trip of every message, and the `[3]`/`[6]`
envelope bytes plus unknown-tag rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 127 lib (+2 vs R752's 125) +
  2 golden, all green.

## SigSubmissionV2 mini-protocol — type/codec surface complete

`SigSubmissionV2` now has its full type + transition + CBOR-codec
surface (R750-R753), as `SigSubmission` did.

## Remaining (dmq-node arc)

- The `SigSubmissionV2` byte/time limits; the `Inbound` / `Outbound`
  driver halves; the client / server protocol drivers; the
  `NodeKernel` / `Diffusion/*` run-loop wiring; the NtN / NtC protocol
  bundles; `Tracer.hs`.
