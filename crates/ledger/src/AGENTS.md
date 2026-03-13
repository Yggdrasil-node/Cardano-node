---
name: ledger-src-subagent
description: Guidance for shared ledger internals outside era-specific modules
---

Focus on core ledger plumbing shared across eras: CBOR codec, core types, and state integration surfaces.

## Scope
- `cbor.rs`, `types.rs`, generic ledger state helpers, and module wiring under `crates/ledger/src`.
- Boundaries between shared ledger infrastructure and `eras/` era-specific logic.

## Non-Negotiable Rules
- Keep CBOR behavior deterministic and round-trip tested.
- Do not duplicate era-specific rules in shared modules.
- Maintain strong type wrappers for protocol-relevant identifiers (`SlotNo`, `BlockNo`, `HeaderHash`, `Point`, `TxId`).
- Public shared APIs MUST have Rustdocs when semantics are non-obvious.

## Upstream References (add or update as needed)
- Ledger repository: <https://github.com/IntersectMBO/cardano-ledger>
- Formal specs: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Hand-rolled CBOR encoder/decoder supports major Cardano-required primitives including signed integers (`integer()`).
- Shared typed core identifiers and point/nonce primitives are in place.
- Credential and address types landed: `StakeCredential` (key-hash/script-hash), `RewardAccount` (29-byte structured), `Address` (Base/Enterprise/Pointer/Reward/Byron variants), with CBOR codecs and variable-length natural encoding for pointer addresses.
- Certificate hierarchy landed in `types.rs`: `Anchor` (moved from conway.rs), `UnitInterval` (tag-30 rational), `Relay` (3-variant), `PoolMetadata`, `PoolParams` (9-field inline group), `DRep` (4-variant Conway), `DCert` (19-variant flat enum covering Shelley tags 0–5 and Conway tags 7–18), all with CBOR codecs in `cbor.rs`.
- `LedgerState` owns a `ShelleyUtxo` and performs atomic block application with CBOR decode + UTxO validation.
- Era-specific structures live under `eras/`; Shelley and Allegra types are implemented. Shared layer should stay lightweight and stable.\n- `plutus.rs` module landed: `PlutusData` recursive AST with full CBOR codec (constr tags 121–127, general form tag 102, bignum encoding), `Script` enum (Native/PlutusV1/V2/V3), `ScriptRef` (tag-24 double encoding).
- `PlutusData` is integrated into: `Redeemer.data` (typed payload), `DatumOption::Inline` (typed inline datum with tag-24 double encoding), `ShelleyWitnessSet.plutus_data` (typed `Vec<PlutusData>`).
