---
title: "Round 737 dmq-node LocalMsgSubmission codec (dmq-node arc, slice 19)"
parent: Reference
---

# Round 737 dmq-node LocalMsgSubmission codec (dmq-node arc, slice 19)

Date: 2026-05-21

## Scope

Slice 19 of the dmq-node arc. Adds the `LocalMsgSubmission` message
CBOR codec.

## What shipped

`crates/tools/dmq-node/src/protocol/local_msg_submission.rs`:

- `LocalMsgSubmissionMessage::to_cbor` / `from_cbor` — the
  `LocalTxSubmission` message envelope (mirror of `crates/network`'s
  `LocalTxSubmissionMessage`): `MsgSubmitTx` is `[0, sig]`,
  `MsgAcceptTx` is `[1]`, `MsgRejectTx` is `[2, reject]`, `MsgDone` is
  `[3]`. The `Sig` payload uses the R724 `encode_sig` / `decode_sig`.
- `encode_reject` / `decode_reject` — the `SigValidationError` reject
  codec, mirror of upstream `LocalMsgSubmission/Codec.hs`'s
  `encodeReject` / `decodeReject`: `SigDuplicate` is `[1]`,
  `SigExpired` is `[2]`, `SigResultOther` is `[3, text]`, every other
  variant is `[0, text]`. Tags `0` and `3` both decode to
  `SigResultOther` — upstream's documented `FIXME SigInvalid` (the
  `[0, ...]` "invalid" form does not round-trip to the original
  variant). The reject `text` uses Rust `Debug` where upstream uses
  Haskell `show` — the same documented divergence as
  `SigValidationError::to_json`; the wire *structure* is exact.

2 unit tests: the message codec round-trip, and the reject codec's
upstream-faithful behaviour (field-less variants round-trip; every
other collapses to `SigResultOther`).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 94 lib (+2 vs R736's 92) +
  2 golden, all green.

## Remaining (dmq-node arc)

- The `LocalMsgNotification` mini-protocol; the client / server
  protocol drivers; the `Diffusion/*` wiring.
