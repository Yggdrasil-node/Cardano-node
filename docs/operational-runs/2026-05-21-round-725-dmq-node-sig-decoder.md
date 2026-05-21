---
title: "Round 725 dmq-node Sig decoder with signed-bytes capture (dmq-node arc, slice 9)"
parent: Reference
---

# Round 725 dmq-node Sig decoder with signed-bytes capture (dmq-node arc, slice 9)

Date: 2026-05-21

## Scope

Slice 9 of the dmq-node arc (codec sub-arc slice 4). Adds the
`Sig`-level decoder that captures `sigRawSignedBytes`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `decode_sig` — mirror of upstream `decodeSig`. Decodes the `SigRaw`
  CBOR 4-element array and additionally captures `sigRawSignedBytes`:
  the exact bytes of the payload sub-array (element 0) — the bytes
  the KES key signed. Upstream brackets the payload with
  `peekByteOffset` / `bytesBetweenOffsets`; the Rust port uses
  `Decoder::position()` around the payload decode. The returned
  `Sig` carries `sig_raw_bytes` = the full input.
- `decode_sig_payload` — a private helper for the `[sigId, sigBody,
  kesPeriod, expiresAt]` payload sub-array (upstream `decodeSig`'s
  `decodePayload` `where`-clause), now shared by `decode_sig_raw`
  (refactored to use it, behavior-preserving) and `decode_sig`.

1 unit test: `decode_sig` round-trips `sample_sig_raw`, and the
captured `sig_signed_bytes()` re-decode as a payload consuming every
byte (`Decoder::remaining() == 0`) — proof the offset bracket is
exactly the payload sub-array.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 67 lib (+1 vs R724's 66) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 10 — the `codecSigSubmission` TxSubmission2 wrapper
  (`SigSubmission = TxSubmission2 SigId Sig`); the
  `timeLimitsSigSubmission` / `byteLimitsSigSubmission` tables.
- `SigValidationError` `ToJSON`; validator (`Validate.hs`);
  NodeToClient / NodeToNode protocols; Diffusion wiring.
