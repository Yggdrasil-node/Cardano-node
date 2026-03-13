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
- Shared transaction plumbing now includes `compute_tx_id` plus submitted-transaction wrappers for Shelley-family and Alonzo-family wire shapes, with `MultiEraSubmittedTx` as the era-directed decode boundary for node-to-node relay work.
- Credential and address types landed: `StakeCredential` (key-hash/script-hash), `RewardAccount` (29-byte structured), `Address` (Base/Enterprise/Pointer/Reward/Byron variants), with CBOR codecs and variable-length natural encoding for pointer addresses.
- Certificate hierarchy landed in `types.rs`: `Anchor` (moved from conway.rs), `UnitInterval` (tag-30 rational), `Relay` (3-variant), `PoolMetadata`, `PoolParams` (9-field inline group), `DRep` (4-variant Conway), `DCert` (19-variant flat enum covering Shelley tags 0–5 and Conway tags 7–18), all with CBOR codecs in `cbor.rs`.
- `LedgerState` owns dual UTxO sets: `ShelleyUtxo` (legacy, for backward compat) and `MultiEraUtxo` (generalized). `apply_block()` dispatches per era with atomic block application and full CBOR decode + UTxO validation.
- `utxo.rs` module landed: `MultiEraTxOut` enum (Shelley/Mary/Alonzo/Babbage), `MultiEraUtxo` with per-era apply methods including TTL, validity interval start, coin preservation, and multi-asset preservation checks.
- Era-specific structures live under `eras/`; all eras Shelley through Conway are implemented. Shared layer should stay lightweight and stable.
- `PlutusData` is integrated into: `Redeemer.data` (typed payload), `DatumOption::Inline` (typed inline datum with tag-24 double encoding), `ShelleyWitnessSet.plutus_data` (typed `Vec<PlutusData>`).
