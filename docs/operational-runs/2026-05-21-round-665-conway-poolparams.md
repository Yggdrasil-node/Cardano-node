---
title: "Round 665 Typed RegPool PoolParams record (A5 Phase-2.5)"
parent: Reference
---

# Round 665 Typed RegPool PoolParams record (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `RegPool` certificate's `PoolParams` record —
`TxCert::ConwayTxCertPool` now carries a typed `PoolParams`
struct (VRF key hash, pledge, cost, margin, reward account)
instead of an opaque raw `RegPool` group.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/StakePool.hs:422-553`
  (`PoolParams` `EncCBORGroup`/`DecCBORGroup` — 9 flattened
  fields: `ppId`, `ppVrf`, `ppPledge`, `ppCost`, `ppMargin`,
  `ppRewardAccount`, `ppOwners`, `ppRelays`, `ppMetadata`;
  metadata via `encodeNullStrictMaybe`).
- `UnitInterval` (a `BoundedRatio`) — CBOR tag-30 rational
  `#6.30([numerator, denominator])`.

## Changes

- Added `UnitInterval { numerator, denominator }` — decodes the
  tag-30 rational. Display: `<num> % <den>`.
- Added `PoolParams { vrf: [u8; 32], pledge, cost, margin:
  UnitInterval, reward_account, rest: Vec<u8> }` —
  `from_decoder` reads the 5 scalar group fields after the pool
  operator key hash and captures the owners / relays / metadata
  collection tail raw.
- Refactored `TxCert::ConwayTxCertPool` — replaced the `rest:
  Vec<u8>` field with `params: Option<PoolParams>` (Some for
  tag 3 RegPool, None for tag 4 RetirePool). `TxCert::from_decoder`
  routes RegPool through `PoolParams::from_decoder`.
- Removed the now-unused `capture_rest` closure — all three
  certificate families decode their tails positionally.
- Display: `ConwayTxCertPool (RegPoolTxCert (<KeyHash>)
  (PoolParams {ppVrf, ppPledge, ppCost, ppMargin,
  ppRewardAccount, ...}))`.

2 tests updated:
- `tx_cert_decodes_pool_retire` — `rest` → `params` (asserts
  `params.is_none()`).
- `tx_cert_decodes_pool_register` — rewritten with a full
  10-element RegPool envelope, asserting the typed VRF / pledge
  / cost / margin fields.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (333 lib + 4
  doctests + 1 main — R665 strengthened the two pool tests with
  full `PoolParams` coverage)

## Remaining (A5 Phase-2.5+)

- `PoolParams` owners / relays / metadata collection fields.
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
