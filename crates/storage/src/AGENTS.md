---
name: storage-src
description: Guidance for storage trait and implementation modules.
---

This directory owns storage interfaces and implementations, including rollback-aware persistence behavior.

## Scope
- Immutable, volatile, and ledger store implementations.
- Persistence helpers and typed snapshot interfaces.

##  Rules *Non-Negotiable*
- Persistence semantics here MUST remain explicit and independent of node orchestration code.
- File-format pragmatism is acceptable, but migration and recovery paths MUST remain possible.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- ChainDB orchestration: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ChainDB>
- ImmutableDB: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ImmutableDB>
- VolatileDB: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/VolatileDB>
- LedgerDB: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/LedgerDB>
- Consensus storage documentation: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs>
- Consensus Haddock (storage modules): <https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/>

## Current Phase
- Storage source modules now provide in-memory and file-backed implementations behind stable traits, plus a minimal `ChainDb` coordination layer that owns best-known tip recovery, volatile-prefix promotion, and rollback-time ledger snapshot truncation while leaving room for future format and recovery upgrades. Rollbacks to points that are already in immutable storage must clear the volatile suffix so recovery metadata does not keep a stale volatile tip.
- `ChainDb` now also exposes typed ledger-checkpoint save/load helpers built on the ledger crate's deterministic CBOR codec, while the underlying `LedgerStore` trait remains raw-byte oriented for format flexibility.
- `recover_ledger_state()` uses `try_restore_checkpoint()` to iterate backward through available snapshots when the latest checkpoint is corrupt, providing resilience against partial or interrupted writes.
- File-backed store modules (`file_immutable.rs`, `file_volatile.rs`, `file_ledger.rs`) use atomic file writes (write-to-temp then `fs::rename`) so a crash mid-write never leaves a corrupt primary file.
- `VolatileStore::suffix_after(point)` returns blocks strictly after a given point, enabling rollback transaction capture and ledger state replay from volatile storage. Implemented for both `InMemoryVolatile` and `FileVolatile`.
- `ImmutableStore` now exposes ordered suffix replay after a chain point so node-side recovery can rebuild ledger state from checkpoints through immutable storage before replaying the volatile window.