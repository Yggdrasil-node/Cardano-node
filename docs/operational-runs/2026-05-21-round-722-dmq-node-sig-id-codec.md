---
title: "Round 722 dmq-node SigId CBOR codec (dmq-node arc, slice 6)"
parent: Reference
---

# Round 722 dmq-node SigId CBOR codec (dmq-node arc, slice 6)

Date: 2026-05-21

## Scope

Slice 6 of the dmq-node arc — opens the `SigSubmission` CBOR codec
sub-arc. Adds `encode_sig_id` / `decode_sig_id`.

## Parity plan

Authored in-conversation (the codec is byte-level wire work). It
reviewed upstream `SigSubmission/Codec.hs:93-178` and decomposed the
codec sub-arc into: slice 6 `SigId` codec (this round), slice 7
`SigOpCertificate` codec, slice 8 `SigRaw`/`Sig` codec, slice 9 the
`codecSigSubmission` TxSubmission2 wrapper.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `encode_sig_id` — mirror of upstream `encodeSigId SigId{getSigId} =
  encodeBytes (getSigHash getSigId)`: a `SigId` encodes as a CBOR
  byte string of the underlying `SigHash` bytes.
- `decode_sig_id` — mirror of `decodeSigId = SigId . SigHash <$>
  decodeBytes`.

Built on the project's canonical CBOR primitives
(`yggdrasil_ledger::cbor::{Encoder, Decoder}`).

`crates/tools/dmq-node/Cargo.toml` — new workspace-internal
dependency `yggdrasil-ledger` (for the CBOR `Encoder`/`Decoder`).

2 unit tests: a byte-exact assertion (`SigId([0xAA,0xBB])` →
`[0x42, 0xAA, 0xBB]` — CBOR major type 2, length 2) and a
round-trip.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 62 lib (+2 vs R721's 60) +
  2 golden, all green.

## Remaining (dmq-node arc)

- Slice 7 — `SigOpCertificate` codec (`encodeListLen 4` + verKeyKES +
  Word64 + KESPeriod + signedDSIGN).
- Slice 8 — `SigRaw` payload + `Sig` codec.
- Slice 9 — the `codecSigSubmission` TxSubmission2 wrapper.
- `SigValidationError` `ToJSON`; validator (`Validate.hs`);
  NodeToClient / NodeToNode protocols; Diffusion wiring.
