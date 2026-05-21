---
title: "Round 721 dmq-node SigRaw/Sig payload types (dmq-node arc, slice 5)"
parent: Reference
---

# Round 721 dmq-node SigRaw/Sig payload types (dmq-node arc, slice 5)

Date: 2026-05-21

## Scope

Slice 5 of the dmq-node arc. Adds the composed `SigSubmission`
payload types to `protocol/sig_submission.rs`, completing the
`Type.hs` data-type surface.

## What shipped

`crates/tools/dmq-node/src/protocol/sig_submission.rs`:

- `PosixTime` — `u32` whole seconds. Upstream `sigRawExpiresAt ::
  POSIXTime`, but the `SigSubmission` codec decodes it as a bare
  `Word32` (`realToFrac <$> CBOR.decodeWord32`, `Codec.hs:181`), so
  the wire representation is `u32`.
- `SigRaw` — the 7-field signature payload (`sigRawId`, `sigRawBody`,
  `sigRawKESPeriod`, `sigRawOpCertificate`, `sigRawColdKey`,
  `sigRawExpiresAt`, `sigRawKESSignature`).
- `SigRawWithSignedBytes` — `SigRaw` paired with the exact bytes the
  KES key signed.
- `Sig` — the wire signature (`sigRawBytes` + `sigRawWithSignedBytes`),
  with 9 flat accessor methods (`sig_id`, `sig_body`,
  `sig_kes_period`, `sig_op_certificate`, `sig_cold_key`,
  `sig_expires_at`, `sig_kes_signature`, `sig_signed_bytes`,
  `sig_bytes`) mirroring upstream's bidirectional `Sig` pattern
  synonym.

This completes the `SigSubmission/Type.hs` data-type surface — the
byte-wrapper newtypes (R717), the validation-error tree (R718), the
three crypto newtypes (R719–R720), and now the composed payload.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 60 lib (+2 vs R720's 58) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `SigSubmission` type alias (`= TxSubmission2 SigId Sig`) — wiring
  to the network `TxSubmission2` mini-protocol.
- `SigValidationError` `ToJSON` rendering.
- `SigSubmission` CBOR codec (`Codec.hs`; warrants a `parity-plan`)
  and validator (`Validate.hs`).
- NodeToClient / NodeToNode protocols, Diffusion wiring.
