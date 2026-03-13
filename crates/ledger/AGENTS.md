---
name: ledger-crate-agent
description: Guidance for era-aware ledger work
---

Focus on reusable state-transition interfaces and explicit era boundaries.

## Scope
- Era modeling, transaction and block state transitions, and ledger state evolution.
- Separation between generated wire types and handwritten rules.

## Non-Negotiable Rules
- Specification provenance MUST stay close to each ledger rule.
- Generated data types and handwritten transition logic MUST remain separated.
- The project MUST keep a full era roadmap visible, but implementation MUST proceed one narrow slice at a time.
- Public ledger modules, types, and state-transition functions MUST have Rustdocs where rule intent or invariants are not obvious from the signature.
- Era, transaction, and rule naming MUST stay close to official ledger and `cardano-node` terminology.
- Ledger behavior MUST be explained by reference to the official node, the ledger repository, and the formal ledger specifications rather than only local interpretation.

## Upstream References (add or update as needed)
- Ledger repository root: <https://github.com/IntersectMBO/cardano-ledger/>
- Era-specific sources and CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications/>
- Published formal spec site: <https://intersectmbo.github.io/formal-ledger-specifications/site/>

## Current Phase
- Core protocol types (`SlotNo`, `BlockNo`, `EpochNo`, `HeaderHash`, `TxId`, `Point`) are landed in `types.rs`.
- `Block` and `BlockHeader` use typed identifiers; `LedgerState` tracks tip via `Point` and owns a `ShelleyUtxo` with atomic per-block application.
- CBOR codec (`cbor.rs`) supports all 8 major types plus signed integer helpers (`Encoder::integer`, `Decoder::integer`). Includes `skip()` for recursive item skipping and `CborEncode`/`CborDecode` traits.
- Allegra era types landed: `AllegraTxBody` (optional TTL + validity interval start), `NativeScript` (6-variant timelock/multi-sig enum with recursive CBOR codec).
- Mary era types landed: `Value` (coin/multi-asset), `MultiAsset`, `MintAsset`, `MaryTxOut`, `MaryTxBody` (key 9 mint) with CBOR codecs; `pub(crate)` helpers shared cross-era.
- Alonzo era types landed: `ExUnits`, `Redeemer` (opaque PlutusData as raw CBOR), `AlonzoTxOut` (optional datum hash), `AlonzoTxBody` (keys 11/13/14/15).
- Byron envelope landed: `ByronBlock` enum (EBB/MainBlock) with lightweight decode for slot tracking, `BYRON_SLOTS_PER_EPOCH`.
- Keep the full era roadmap visible, but land only narrow reusable slices.
- Prefer types and harnesses that will survive later era expansion.
