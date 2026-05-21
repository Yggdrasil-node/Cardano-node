---
title: "Round 724 dmq-node SigRaw/Sig codec (dmq-node arc, slice 8)"
parent: Reference
---

# Round 724 dmq-node SigRaw/Sig codec (dmq-node arc, slice 8)

Date: 2026-05-21

## Scope

Slice 8 of the dmq-node arc (codec sub-arc slice 3). Adds the
`SigRaw` 4-element-array codec and the `Sig` encoder to
`protocol/sig_submission.rs`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `encode_sig` — mirror of upstream `encodeSig = encodeBytes .
  sigRawBytes`: a `Sig` carries the already-encoded `SigRaw`, so
  encoding wraps those cached bytes in a CBOR byte string.
- `encode_sig_raw` / `decode_sig_raw` — the `SigRaw` CBOR 4-element
  array `[payload, kesSignature, opCertificate, coldKey]`, where
  `payload` is itself the 4-element array `[sigId, sigBody,
  kesPeriod, expiresAt]` (the bytes the KES key signs). Mirror of
  the structure upstream `decodeSig` parses; the KES signature and
  cold key are CBOR byte strings (`encodeSigKES` /
  `encodeVerKeyDSIGN`). `expiresAt` decodes as a `Word32` (the
  codec's `decodeWord32`), so an out-of-`u32`-range value is a
  `LedgerError::ValueOverflow`.
- `expect_array_len` — a private CBOR array-header length helper.

3 unit tests: the `SigRaw` round-trip (with the `0x84` outer-array
check), and `encode_sig` emitting the cached bytes as a CBOR byte
string.

## Scope boundary

`decode_sig_raw` parses the structure only. The `Sig` /
`SigRawWithSignedBytes` decoder — which additionally captures
`sigRawSignedBytes` (the exact payload-sub-array bytes the KES key
signed, via upstream's `peekByteOffset` / `bytesBetweenOffsets`) —
is slice 9, together with the `codecSigSubmission` TxSubmission2
wrapper.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 66 lib (+2 vs R723's 64) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 9 — `Sig` / `SigRawWithSignedBytes` decode with
  `sigRawSignedBytes` capture; the `codecSigSubmission` TxSubmission2
  wrapper.
- `SigValidationError` `ToJSON`; validator (`Validate.hs`);
  NodeToClient / NodeToNode protocols; Diffusion wiring.
