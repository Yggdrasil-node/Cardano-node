---
title: "Round 618 Credential decoder + wire DELEG tags 0/1/3 (A5 Phase-2.5)"
parent: Reference
---

# Round 618 Credential decoder + wire DELEG tags 0/1/3 (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `Credential` enum (KeyHashObj / ScriptHashObj) with CBOR
decoder and Display, wires `ShelleyDelegPredFailure` tags 0/1/3
(Credential-carrying variants) to typed payloads.

**After R618, all 16 DELEG variants now carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Credential.hs:96-99,320-332`
  (`data Credential (kr :: KeyRole) = ScriptHashObj !ScriptHash
  | KeyHashObj !(KeyHash kr)`; CBOR encoder uses 2-element array
  `[tag, hash]` with tag 0 = KeyHashObj, tag 1 = ScriptHashObj).

## Changes

- Added `Credential` enum mirroring upstream `Credential kr`:
  - `KeyHashObj(KeyHash)` — tag 0.
  - `ScriptHashObj(ScriptHash)` — tag 1.
- `Credential::from_decoder` walks the canonical 2-element CBOR
  array, reads the Word8 tag, and decodes the 28-byte hash.
  Unknown tags reject explicitly.
- Display matches upstream stock-derived constructor Show wrapped
  in the appropriate hash newtype's Show:
  `KeyHashObj (KeyHash {unKeyHash = "..."})` and
  `ScriptHashObj (ScriptHash "...")`.
- Refactored `ShelleyDelegPredFailure` variants 0/1/3 from
  `Vec<u8>` to `Credential`. Updated `from_cbor` dispatcher: all
  three Credential-bearing variants share a typed branch that
  reads a 2-element envelope then decodes the inner Credential.
  Display routes typed payloads.

4 new tests + 1 replaced:
- `_stake_key_already_registered_decodes_tag0` typed end-to-end
  (KeyHashObj inner).
- `_stake_key_not_registered_decodes_tag1_scripthash` typed
  end-to-end (ScriptHashObj inner — exercises the alternate
  Credential variant).
- `_stake_delegation_impossible_decodes_tag3` typed end-to-end.
- `credential_from_decoder_rejects_unknown_tag` (tag 5 — outside
  Credential's known set).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (227 lib + 4
  doctests + 1 main, +3 net new tests vs R617 baseline of 224 —
  added 4, replaced 1)

## Remaining (A5 Phase-2.5+)

- POOL tag 1 (`StakePoolRetirementWrongEpochPOOL` flattened
  3-EpochNo encoding) decoder.
- Inner per-TxOut typed parse (era-specific Shelley/Babbage).
- Full typed `Addr` Show parse (Shelley vs Bootstrap split).
- Mirror the per-era predicate-failure tree for Allegra/Mary/
  Alonzo/Babbage/Conway eras.
