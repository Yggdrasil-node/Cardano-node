---
title: "Round 723 dmq-node SigOpCertificate codec (dmq-node arc, slice 7)"
parent: Reference
---

# Round 723 dmq-node SigOpCertificate codec (dmq-node arc, slice 7)

Date: 2026-05-21

## Scope

Slice 7 of the dmq-node arc (codec sub-arc slice 2). Adds the
`SigOpCertificate` CBOR codec to `protocol/sig_submission.rs`.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `encode_sig_op_certificate` — mirror of upstream
  `encodeSigOpCertificate`: a CBOR 4-element array of
  `encodeVerKeyKES (ocertVkHot)`, `toCBOR (ocertN)`,
  `toCBOR (ocertKESPeriod)`, `encodeSignedDSIGN (ocertSigma)`. The
  KES verkey and DSIGN signature encode as CBOR byte strings of their
  raw bytes; the counter and KES period as CBOR unsigned integers.
- `decode_sig_op_certificate` — mirror of `decodeSigOpCertificate`;
  rejects any list length other than 4
  (`LedgerError::CborInvalidLength`).
- `decode_fixed_bytes::<N>` — a private helper decoding a CBOR byte
  string of exactly `N` bytes into a fixed array.

3 unit tests: round-trip, the `0x84` array-header byte-check, and
the wrong-list-length rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 64 lib (+2 vs R722's 62) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 8 — `SigRaw` payload + `Sig` codec (`encodeSig` /
  `decodeSig`, the 4-element message with the payload sub-list).
- Slice 9 — the `codecSigSubmission` TxSubmission2 wrapper.
- `SigValidationError` `ToJSON`; validator (`Validate.hs`);
  NodeToClient / NodeToNode protocols; Diffusion wiring.
