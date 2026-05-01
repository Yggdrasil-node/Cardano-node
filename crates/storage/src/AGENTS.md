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
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [ChainDB orchestration](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ChainDB)
- [ImmutableDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ImmutableDB)
- [VolatileDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/VolatileDB)
- [LedgerDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/LedgerDB)
- [Consensus storage documentation](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs)
- [Consensus Haddock (storage modules)](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/)

## Current Phase
- Storage source modules now provide in-memory and file-backed implementations behind stable traits, plus a minimal `ChainDb` coordination layer that owns best-known tip recovery, volatile-prefix promotion, and rollback-time ledger snapshot truncation while leaving room for future format and recovery upgrades. Rollbacks to points that are already in immutable storage must clear the volatile suffix so recovery metadata does not keep a stale volatile tip.
- `ChainDb` now also exposes typed ledger-checkpoint save/load helpers built on the ledger crate's deterministic CBOR codec, while the underlying `LedgerStore` trait remains raw-byte oriented for format flexibility.
- `recover_ledger_state()` uses `try_restore_checkpoint()` to iterate backward through available snapshots when the latest checkpoint is corrupt, providing resilience against partial or interrupted writes.
- File-backed store modules (`file_immutable.rs`, `file_volatile.rs`, `file_ledger.rs`) use atomic file writes (write-to-temp then `fs::rename`) so a crash mid-write never leaves a corrupt primary file.
- `FileVolatile` now adds write-ahead delete journaling for multi-step mutations: before `prune_up_to`, `rollback_to`, and `garbage_collect` delete batches, it writes `wal.pending.json`; open-time recovery replays and clears pending plans so interrupted delete sequences converge deterministically.
- `FileImmutable` and `FileVolatile` persist block payloads as deterministic CBOR files (`*.cbor`) while retaining backward-compatible reads of legacy JSON block files (`*.json`) during open.
- Open-path loading deduplicates dual-format files by block hash and prefers CBOR over JSON when both exist for the same hash.
- `VolatileStore::suffix_after(point)` returns blocks strictly after a given point, enabling rollback transaction capture and ledger state replay from volatile storage. Implemented for both `InMemoryVolatile` and `FileVolatile`.
- `ImmutableStore` now exposes ordered suffix replay after a chain point so node-side recovery can rebuild ledger state from checkpoints through immutable storage before replaying the volatile window.
- `ImmutableStore::trim_before_slot(SlotNo)` enables garbage collection of old immutable blocks; implemented for both `InMemoryImmutable` (Vec::retain) and `FileImmutable` (file deletion + index cleanup). `ChainDb::gc_immutable_before_slot()` coordinates GC without disturbing volatile or ledger stores.
- `ImmutableStore::get_block_by_slot(SlotNo)` provides slot-indexed block lookup; `FileImmutable` uses binary search over the sorted chain vector for O(log n) performance.
- `FileImmutable::open()`, `FileVolatile::open()`, and `FileLedgerStore::open()` are crash-tolerant: corrupted or unreadable files are silently skipped instead of failing the open. `skipped_on_open()` exposes the count of skipped files for monitoring.
- `VolatileStore::garbage_collect(slot: SlotNo) -> usize` removes all blocks with slot strictly below the given slot (upstream `garbageCollect` semantics). `VolatileStore::block_count() -> usize` exposes the number of blocks in the volatile store. Both implemented for `InMemoryVolatile` and `FileVolatile`. `ChainDb::gc_volatile_before_slot(slot)` coordinates volatile GC.
- `FileVolatile::compact() -> Result<usize, StorageError>` scans the data directory for orphaned `.cbor`/`.json` files not referenced by the in-memory index plus stale `.tmp` files, removing them and returning the count of deleted files.
- `ocert_sidecar.rs` owns opaque sidecar byte persistence for consensus-adjacent state. Slot-indexed `chain_dep_state/<slot-hex>.cbor` helpers are the canonical nonce/OpCert ChainDepState history, so node rollback/restart/LSQ recovery can restore at-or-before a chain point and replay forward without changing ledger checkpoint CBOR.
