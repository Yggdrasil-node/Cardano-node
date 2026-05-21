---
title: "Round 661 Typed staking credential in Conway delegation certs (A5 Phase-2.5)"
parent: Reference
---

# Round 661 Typed staking credential in Conway delegation certs (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the staking credential in every Conway delegation-family
certificate — `TxCert::ConwayTxCertDeleg` now surfaces a typed
`Credential` instead of an opaque raw payload.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxCert.hs:656-705`
  (`conwayTxCertDelegDecoder` — every delegation certificate
  (`Sum` tags 0-2, 7-13) decodes a `cred <- decCBOR` as its
  first payload field, followed by certificate-specific fields:
  pool key hash, `DRep`, deposit `Coin`).

## Changes

- Refactored `TxCert::ConwayTxCertDeleg` from `{ cert_tag, raw }`
  → `{ cert_tag, credential: Credential, rest: Vec<u8> }`. The
  staking credential is decoded via `Credential::from_decoder`;
  `rest` captures the certificate's remaining fields raw (pool
  key hash / DRep / deposit, pending their typed decoders).
- `TxCert::from_decoder` now decodes the credential for
  delegation-family tags before capturing the tail; a
  `capture_rest` closure handles the per-family byte-range
  capture (deleg from index 2, pool/gov from index 1).
- Display: delegation certs render `ConwayTxCertDeleg
  (<CertConstructor> (<Credential>))` — plus a `<raw-cbor N
  bytes>` marker when the certificate carries trailing fields.

1 new test + 1 updated:
- `_missing_redeemers_certifying_reg_deposit` — a
  `RegDepositTxCert` (tag 7, `[tag, credential, deposit]`)
  asserting the typed `ScriptHashObj` credential and the
  3-byte raw deposit tail.
- `_missing_redeemers_certifying_txcert` updated to assert the
  typed `KeyHashObj` credential of the `UnRegTxCert`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (328 lib + 4
  doctests + 1 main, +1 new test vs R660 baseline of 327)

## Remaining (A5 Phase-2.5+)

- `TxCert` delegation-cert tail fields (pool key hash, `DRep`,
  deposit) and the `ConwayTxCertPool` / `ConwayTxCertGov`
  bodies.
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
