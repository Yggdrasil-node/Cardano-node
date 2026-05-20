---
title: "Round 615 ShelleyDelegPredFailure scaffold + wire DELPL tag 1 (A5 Phase-2.5)"
parent: Reference
---

# Round 615 ShelleyDelegPredFailure scaffold + wire DELPL tag 1 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ShelleyDelegPredFailure` 16-variant scaffold (the DELEG
sub-rule that DELPL tag 1 dispatches into — sibling to POOL) and
wires `ShelleyDelplPredFailure::DelegFailure` to the typed enum.
9 of 16 DELEG variants carry typed payloads; 7 keep raw payloads
pending the more elaborate Credential/MIRPot/Mismatch decoders.

After R615, the LEDGER → DELEGS → DELPL → {POOL, DELEG} chain
renders typed end-to-end for the substantial majority of leaves.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Deleg.hs:90-123,154-196`
  (`data ShelleyDelegPredFailure era` 16-variant ADT, CBOR
  encoder with tags 0-9 + 11-16, tag 10 deliberately skipped).

## Changes

- Added `VrfVerKeyHash([u8; 32])` newtype with Display matching
  upstream stock-derived record Show: `VRFVerKeyHash
  {unVRFVerKeyHash = "<hex>"}`.
- Added `ShelleyDelegPredFailure` 16-variant enum:
  - **Typed (9 variants):**
    - Tag 2 `StakeKeyNonZeroAccountBalanceDELEG(u64)` — Coin.
    - Tag 4 `WrongCertificateTypeDELEG` — no payload.
    - Tag 5 `GenesisKeyNotInMappingDELEG(KeyHash)`.
    - Tag 6 `DuplicateGenesisDelegateDELEG(KeyHash)`.
    - Tag 9 `DuplicateGenesisVRFDELEG(VrfVerKeyHash)`.
    - Tag 11 `MIRTransferNotCurrentlyAllowed` — no payload.
    - Tag 12 `MIRNegativesNotCurrentlyAllowed` — no payload.
    - Tag 14 `MIRProducesNegativeUpdate` — no payload.
    - Tag 16 `DelegateeNotRegisteredDELEG(KeyHash)`.
  - **Raw (7 variants):**
    - Tags 0/1/3 (Credential Staking — pending Credential
      decoder).
    - Tags 7/13 (MIRPot + Mismatch RelLTEQ Coin).
    - Tag 8 (Mismatch RelLT SlotNo).
    - Tag 15 (MIRPot + Coin).
- Helpers: `tag()` returns Word8 per upstream encoding (with tag
  10 deliberately absent); `constructor()` returns upstream
  stock-derived Show name; `from_cbor` walks the outer CBOR
  array (length 1-3), reads the Word8 tag, dispatches per-variant.
  Length enforcement: tags 4/11/12/14 require 1-element envelope;
  typed payload variants require 2-element. Unknown tags (incl.
  tag 10) reject explicitly.
- Display routes typed payloads through their typed inner Display;
  raw variants emit `<Constructor> <raw-cbor N bytes>`.
- Refactored `ShelleyDelplPredFailure::DelegFailure(Vec<u8>)` →
  `DelegFailure(ShelleyDelegPredFailure)`. DELPL `from_cbor`
  decodes through `ShelleyDelegPredFailure::from_cbor`. Display
  routes the typed nested payload.

6 new focused unit tests:
- `_deleg_failure_decodes_tag1` end-to-end DELPL → DELEG.
- `_no_payload_variants` parameterized test for tags 4/11/12/14.
- `_coin_decodes_tag2` end-to-end tag-2 coin.
- `_keyhash_decodes_tag5` end-to-end tag-5 KeyHash.
- `_vrf_decodes_tag9` end-to-end tag-9 VRFVerKeyHash.
- `_routes_unported_tag0_to_raw` (tag 0 raw routing).
- `_unknown_tag_rejects` (tag 10 — deliberately skipped by
  upstream — must reject).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (215 lib + 4
  doctests + 1 main, +6 new tests vs R614 baseline of 209)

## Remaining (A5 Phase-2.5+)

- DELEG variants 0/1/3 (Credential Staking decoder).
- DELEG variants 7/13 (MIRPot + Mismatch RelLTEQ Coin).
- DELEG variant 8 (Mismatch RelLT SlotNo).
- DELEG variant 15 (MIRPot + Coin).
- POOL variants 1/3/4/5/6 payload decoders.
- Inner per-TxOut typed parse (era-specific Shelley/Babbage
  shapes).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
