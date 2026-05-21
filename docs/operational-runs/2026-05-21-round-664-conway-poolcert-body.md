---
title: "Round 664 Typed ConwayTxCertPool body (A5 Phase-2.5)"
parent: Reference
---

# Round 664 Typed ConwayTxCertPool body (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `ConwayTxCertPool` body — `TxCert::ConwayTxCertPool`
now carries the typed stake-pool key hash and the `RetirePool`
retirement epoch instead of an opaque raw body. All three
`ConwayTxCert` certificate families now carry typed payloads.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxCert.hs:435-515`
  (`encodePoolCert` / `poolTxCertDecoder` — tag 3 RegPool =
  `[3, ...PoolParams encCBORGroup...]` (the operator key hash
  `ppId` leads the group), tag 4 RetirePool = `[4, poolKeyHash,
  epoch]`).

## Changes

- Refactored `TxCert::ConwayTxCertPool` from `{ cert_tag, raw }`
  → `{ cert_tag, pool: KeyHash, epoch: Option<u64>, rest:
  Vec<u8> }`. `TxCert::from_decoder` decodes the leading
  stake-pool key hash (present in both `RegPool` and
  `RetirePool`); for tag 4 it also decodes the retirement
  epoch, and for tag 3 it captures the remaining `PoolParams`
  group fields raw.
- Display: `ConwayTxCertPool (<CertConstructor> (<KeyHash>)
  [(EpochNo <n>)] [<raw-cbor N bytes>])`.

2 new focused unit tests:
- `tx_cert_decodes_pool_retire` — tag 4 RetirePool, full typed
  round-trip (pool key hash + EpochNo).
- `tx_cert_decodes_pool_register` — tag 3 RegPool, typed
  operator key hash + raw PoolParams tail.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (333 lib + 4
  doctests + 1 main, +2 new tests vs R663 baseline of 331)

## Remaining (A5 Phase-2.5+)

- `RegPool` `PoolParams` record body (VRF / pledge / cost /
  margin / reward account / owners / relays / metadata).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
