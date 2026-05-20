---
title: "Round 616 ShelleyPoolPredFailure typed payloads for tags 3/4/5/6 (A5 Phase-2.5)"
parent: Reference
---

# Round 616 ShelleyPoolPredFailure typed payloads for tags 3/4/5/6 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires typed payload decoders for 4 of the 5 remaining
`ShelleyPoolPredFailure` raw-payload variants:
- Tag 3 `StakePoolCostTooLowPOOL`: `Mismatch<u64>` (Coin) with
  RelGTEQ relation.
- Tag 4 `WrongNetworkPOOL`: struct variant `{ expected: Network,
  supplied: Network, pool_id: KeyHash }`.
- Tag 5 `PoolMedataHashTooBig`: struct variant `{ pool_id:
  KeyHash, size: u32 }` (Int narrowed to Word32 at decode time).
- Tag 6 `VRFKeyHashAlreadyRegistered`: struct variant `{ pool_id:
  KeyHash, vrf_key_hash: VrfVerKeyHash }`.

Tag 1 (`StakePoolRetirementWrongEpochPOOL`) remains raw pending a
dedicated decoder for the flattened-3-EpochNo Mismatch encoding.

After R616, **5 of 6 POOL variants carry fully-typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Pool.hs:96-114,158-171`
  (variant payloads + CBOR encoder for tags 3/4/5/6).

## Changes

- Refactored `ShelleyPoolPredFailure` variants:
  - Tag 3: `StakePoolCostTooLowPOOL(Vec<u8>)` →
    `StakePoolCostTooLowPOOL(Mismatch<u64>)`.
  - Tag 4: `WrongNetworkPOOL(Vec<u8>)` →
    `WrongNetworkPOOL { expected: Network, supplied: Network,
    pool_id: KeyHash }`.
  - Tag 5: `PoolMedataHashTooBig(Vec<u8>)` →
    `PoolMedataHashTooBig { pool_id: KeyHash, size: u32 }`.
  - Tag 6: `VRFKeyHashAlreadyRegistered(Vec<u8>)` →
    `VRFKeyHashAlreadyRegistered { pool_id: KeyHash,
    vrf_key_hash: VrfVerKeyHash }`.
- Updated `tag()` and `constructor()` to use struct-variant
  destructuring with `{ .. }`.
- Updated `from_cbor` dispatcher: each typed tag now enforces
  exact envelope length and reads/narrows its payload directly
  from the in-progress decoder. Tag 1 keeps the raw-bytes carrier
  with explicit length-4 enforcement.
- Updated `Display`:
  - Tag 3 routes through `Mismatch<CoinShow>` for Quiet-Show Coin
    rendering.
  - Tag 4 wraps the network mismatch in `Mismatch<Network>` then
    appends the typed KeyHash.
  - Tag 5 emits `PoolMedataHashTooBig (<KeyHash>) <size>`.
  - Tag 6 emits `VRFKeyHashAlreadyRegistered (<KeyHash>)
    (<VRFVerKeyHash>)`.

5 new tests + 1 replaced:
- Replaced R614's `_routes_unported_tags_to_raw` with a typed
  end-to-end `_cost_too_low_decodes_tag3` (Mismatch Coin payload).
- New `_wrong_network_decodes_tag4` (Network + KeyHash struct).
- New `_metadata_hash_too_big_decodes_tag5` (KeyHash + Word32
  with narrowing).
- New `_vrf_already_registered_decodes_tag6` (KeyHash +
  VRFVerKeyHash).
- New `_retirement_wrong_epoch_stays_raw_tag1` confirms the last
  raw POOL variant still routes through raw-cbor pending its
  decoder.

Lint cleanup: fixed a `clippy::doc-lazy-continuation` warning on
a wrapped doc comment.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (219 lib + 4
  doctests + 1 main, +4 net new tests vs R615 baseline of 215 —
  added 5, replaced 1)

## Remaining (A5 Phase-2.5+)

- POOL tag 1 (`StakePoolRetirementWrongEpochPOOL` flattened
  3-EpochNo encoding) decoder.
- DELEG variants 0/1/3 (Credential), 7/13 (MIRPot + Mismatch
  Coin), 8 (Mismatch SlotNo), 15 (MIRPot + Coin) decoders.
- Inner per-TxOut typed parse (era-specific Shelley/Babbage).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror per-era predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway.
