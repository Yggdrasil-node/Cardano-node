---
title: "Round 734 dmq-node SigSubmission message codec (dmq-node arc, slice 16)"
parent: Reference
---

# Round 734 dmq-node SigSubmission message codec (dmq-node arc, slice 16)

Date: 2026-05-21

## Scope

Slice 16 of the dmq-node arc (SigSubmission protocol slice 3). Adds
the `SigSubmissionMessage` CBOR codec.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `SigSubmissionMessage::to_cbor` / `from_cbor` — the mini-protocol
  message codec. The message envelope is **byte-identical** to
  `crates/network`'s `TxSubmissionMessage::to_cbor` (mirror of
  upstream `encodeTxSubmission2`): `MsgInit` is `[6]`,
  `MsgRequestTxIds` is `[0, blocking, ack, req]`, `MsgReplyTxIds` is
  `[1, [[sigId, size], ...]]`, `MsgRequestTxs` is `[2, [sigId, ...]]`,
  `MsgReplyTxs` is `[3, [sig, ...]]`, `MsgDone` is `[4]`. The `SigId`
  identifiers and `Sig` payloads use the R722 / R724 `encode_sig_id`
  and `encode_sig` codecs.
- `SIG_SUBMISSION_LIST_MAX` — an anti-DoS pre-allocation cap for list
  decoding (the protocol-level in-flight limits are enforced
  separately).

`from_cbor` rejects an unknown tag, a wrong-arity envelope, and
trailing bytes — matching `crates/network`'s `TxSubmissionMessage`.

3 unit tests: round-trip of all six message variants, the
`[6]`/`[4]` envelope bytes, and unknown-tag rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 87 lib (+3 vs R733's 84) +
  2 golden, all green.

## Remaining (dmq-node SigSubmission protocol)

- Slice 4+ — the client / server protocol drivers, the
  `timeLimits` / `byteLimits` tables, and the
  `codecSigSubmission` integration into the dmq-node run loop.
