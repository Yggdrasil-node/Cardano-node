---
title: "Round 666 Typed PoolParams owners + metadata (A5 Phase-2.5)"
parent: Reference
---

# Round 666 Typed PoolParams owners + metadata (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `PoolParams` owner-set and optional metadata —
`PoolParams` now carries `owners: Vec<KeyHash>` and `metadata:
StrictMaybePoolMetadata` instead of an opaque raw tail. Only the
`ppRelays` sequence remains raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/StakePool.hs:290-296,515-523`
  (`data PoolMetadata = PoolMetadata { pmUrl :: Url, pmHash ::
  ByteArray }`; CBOR 2-element record array).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/State/StakePool.hs:540-549`
  (`PoolParams` `encCBORGroup` — `ppOwners` = `Set (KeyHash
  Staking)`, `ppMetadata` via `encodeNullStrictMaybe`).

## Changes

- Added `PoolMetadata { url: String, hash: Vec<u8> }` — decodes
  the 2-element `[url, hash]` record. Display: `PoolMetadata
  {pmUrl = Url {urlToText = "<url>"}, pmHash = "<hex>"}`.
- Added `StrictMaybePoolMetadata(Option<PoolMetadata>)` —
  `null`-encoded `StrictMaybe PoolMetadata`. Display: `SNothing`
  / `SJust (<PoolMetadata>)`.
- Refactored `PoolParams` — replaced `rest: Vec<u8>` with
  `owners: Vec<KeyHash>`, `relays_raw: Vec<u8>`, `metadata:
  StrictMaybePoolMetadata`. `PoolParams::from_decoder` decodes
  the tag-258-tolerant owner-set and the null-encoded metadata,
  capturing only the `ppRelays` sequence raw.
- Display renders the typed `ppOwners = fromList [...]` and
  `ppMetadata` fields; `ppRelays` keeps a `<raw-cbor N bytes>`
  marker.

1 new focused unit test:
- `pool_params_decodes_owners_and_metadata` — a RegPool whose
  PoolParams carries a one-owner set and a `SJust PoolMetadata`,
  asserting both typed renders.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (334 lib + 4
  doctests + 1 main, +1 new test vs R665 baseline of 333)

## Remaining (A5 Phase-2.5+)

- `PoolParams` `ppRelays` (`StrictSeq StakePoolRelay`).
- Deepest leaf payloads: `PParamsUpdate`, `Constitution`,
  `ContextError`.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
