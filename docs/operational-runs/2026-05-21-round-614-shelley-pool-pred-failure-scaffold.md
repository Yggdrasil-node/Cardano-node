---
title: "Round 614 ShelleyPoolPredFailure scaffold + wire DELPL tag 0 (A5 Phase-2.5)"
parent: Reference
---

# Round 614 ShelleyPoolPredFailure scaffold + wire DELPL tag 0 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ShelleyPoolPredFailure` 6-variant scaffold (the POOL
sub-rule that DELPL tag 0 dispatches into) and wires
`ShelleyDelplPredFailure::PoolFailure` to the typed enum. The
simplest variant (tag 0 `StakePoolNotRegisteredOnKeyPOOL`) carries
a fully typed `KeyHash` payload. Tags 1/3/4/5/6 carry raw payloads
pending the Mismatch-relation + KeyHash-Int per-variant decoders.

After R614, the LEDGER → DELEGS → DELPL → POOL chain renders typed
end-to-end through nested Display.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Pool.hs:92-115,154-172`
  (`data ShelleyPoolPredFailure era` 6-variant ADT, CBOR encoder
  with tags 0/1/3/4/5/6 — tag 2 deliberately skipped).

## Changes

- Added `ShelleyPoolPredFailure` 6-variant enum:
  - Tag 0 `StakePoolNotRegisteredOnKeyPOOL(KeyHash)` — typed.
  - Tag 1 `StakePoolRetirementWrongEpochPOOL(Vec<u8>)` — raw
    pending `(Mismatch RelGT EpochNo, Mismatch RelLTEQ EpochNo)`
    decoder (4-element envelope).
  - Tag 3 `StakePoolCostTooLowPOOL(Vec<u8>)` — raw pending
    `Mismatch RelGTEQ Coin` decoder (3-element envelope).
  - Tag 4 `WrongNetworkPOOL(Vec<u8>)` — raw pending
    `Mismatch RelEQ Network + KeyHash StakePool` decoder
    (4-element envelope).
  - Tag 5 `PoolMedataHashTooBig(Vec<u8>)` — raw pending
    `KeyHash + Int` decoder (3-element envelope).
  - Tag 6 `VRFKeyHashAlreadyRegistered(Vec<u8>)` — raw pending
    `KeyHash + VRFVerKeyHash` decoder (3-element envelope).
- Helpers: `tag()` returns Word8 0/1/3/4/5/6; `constructor()`
  returns upstream stock-derived Show name; `from_cbor` walks the
  outer CBOR array (length 2-4), reads the Word8 tag, dispatches.
  Tag 0 decodes the KeyHash directly; tags 1/3/4/5/6 capture raw
  inner bytes. Unknown tags reject explicitly.
- Display routes tag 0 through typed `KeyHash` Display:
  `StakePoolNotRegisteredOnKeyPOOL (KeyHash {unKeyHash = "<hex>"})`.
  Other tags emit `<Constructor> <raw-cbor N bytes>`.
- Refactored `ShelleyDelplPredFailure::PoolFailure(Vec<u8>)` →
  `PoolFailure(ShelleyPoolPredFailure)`. DELPL `from_cbor` decodes
  through `ShelleyPoolPredFailure::from_cbor`. Display routes the
  typed nested payload.

Test surface updates:
- R613's `_display_routes_typed_delegs` test now constructs a
  typed POOL payload and asserts the full LEDGER → DELEGS → DELPL
  → POOL Display chain.
- Updated `_from_cbor_decodes_tag1` to assert the chain through
  the new typed POOL sub-rule.
- Updated `_pool_failure_decodes_tag0` to assert the new typed
  inner POOL variant.
- New `_stake_pool_not_registered_decodes_tag0` end-to-end POOL
  decode for tag 0.
- New `_routes_unported_tags_to_raw` (tag 3 raw routing).
- New `_unknown_tag_rejects` (tag 77).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (209 lib + 4
  doctests + 1 main, +3 net new tests vs R613 baseline of 206 —
  added 4, replaced 1)

## Remaining (A5 Phase-2.5+)

- `ShelleyDelegPredFailure` decoder (DELPL tag 1).
- POOL variants 1/3/4/5/6 typed payload decoders.
- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
