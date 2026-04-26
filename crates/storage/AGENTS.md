# Guidance for durable storage and snapshot work
Focus on rollback-aware persistence interfaces and stable on-disk boundaries.

## Scope
- Immutable storage, volatile rollback windows, and snapshot persistence.
- Interfaces that consensus and node integration can build on without file-format lock-in.

##  Rules *Non-Negotiable*
- Storage traits MUST be designed before file formats are treated as stable.
- Immutable and volatile concerns MUST remain separate.
- The design MUST preserve a path toward crash recovery and future migrations.
- Public storage interfaces MUST have Rustdocs when persistence guarantees, rollback behavior, or snapshot semantics matter to callers.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Storage naming MUST stay close to official node and Ouroboros consensus terminology such as immutable DB, volatile DB, ChainDB, and snapshots.
- Storage behavior MUST be explained with reference to the official node and upstream Ouroboros consensus implementation notes.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Storage modules (ChainDB, ImmutableDB, VolatileDB, LedgerDB)](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/)
- [ChainDB coordination](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ChainDB/)
- [ImmutableDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ImmutableDB/)
- [VolatileDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/VolatileDB/)
- [LedgerDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/LedgerDB/)
- [Consensus tech report (storage design rationale)](https://ouroboros-consensus.cardano.intersectmbo.org/pdfs/report.pdf)
- [Consensus documentation and architecture notes](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/)
- [Node integration reference for operational storage concerns](https://github.com/IntersectMBO/cardano-node/)

## Current Phase
- Traits `ImmutableStore`, `VolatileStore`, and `LedgerStore` are landed and exported.
- In-memory implementations (`InMemoryImmutable`, `InMemoryVolatile`, `InMemoryLedgerStore`) back each trait.
- File-backed implementations (`FileImmutable`, `FileVolatile`, `FileLedgerStore`) provide deterministic on-disk persistence with directory scanning on open, rollback-aware file deletion, and re-open persistence.
- A minimal `ChainDb` coordinator now lives in the crate to coordinate immutable, volatile, and ledger snapshot stores without pulling sync or consensus policy into `node`. It exposes best-known tip recovery, volatile-prefix promotion into immutable storage, ledger-checkpoint retention helpers, and ledger-snapshot truncation on rollback; rollbacks to immutable points must clear the volatile suffix so the coordinated tip realigns with immutable storage.
- `VolatileStore` now exposes ordered prefix access and pruning helpers so stable volatile blocks can be immutalized through a crate-owned boundary instead of ad hoc node-side coordination.
- `LedgerStore` now supports latest-snapshot lookup at or before a slot plus snapshot truncation after rollback, so restart and rollback flows can reuse the storage layer directly.
- `ocert_sidecar` module exposes `save_ocert_counters(dir, &[u8])` / `load_ocert_counters(dir) -> Option<Vec<u8>>` for the OpCert counter sidecar (`ocert_counters.cbor`). Bytes are opaque to storage — the consensus crate owns the `OcertCounters` CBOR codec — but the writer is the same atomic write-temp-then-rename + parent-dir-fsync discipline used by `FileLedgerStore::save_snapshot`, mirroring the upstream `Ouroboros.Consensus.Storage.LedgerDB` snapshot durability contract. Loader returns `Ok(None)` when the file is absent (fresh node / migration from a pre-slice deployment) so startup can fall back to an empty counter map without surfacing an error. Re-exported from the crate root as `OCERT_COUNTERS_FILENAME`, `save_ocert_counters`, `load_ocert_counters`. Reference: `PraosState.csCounters` in `Ouroboros.Consensus.Protocol.Praos`, persisted as part of `ChainDepState`.
- `ImmutableStore::contains_block(&HeaderHash) -> bool` is now part of the trait surface (default impl delegates to `get_block(...).is_some()`; `FileImmutable` overrides for `O(1)` index lookup; `InMemoryImmutable` overrides for `O(n)` linear scan). `ChainDb::promote_volatile_prefix` uses it to skip blocks that are already in the immutable store, making the promotion idempotent under partial-completion crashes. Without this, a crash between two `append_block` calls — or between the final append and `prune_up_to` — would leave overlap between the volatile and immutable stores; the next promotion attempt would then fail with `StorageError::DuplicateBlock` from the very first overlapping block, blocking all subsequent sync until manual cleanup. The append-then-prune ordering is preserved on purpose so every block stays present in at least one store across the crash window. Reference: `Ouroboros.Consensus.Storage.ChainDB.Impl` `copyToImmutableDB`. Three new regression tests in `tests/integration.rs` lock the contract: `promote_volatile_prefix_is_idempotent_after_partial_promotion_crash` (pre-populates immutable with the first block of the prefix and asserts the next promote succeeds without `DuplicateBlock`), `promote_volatile_prefix_is_idempotent_when_replayed_back_to_back` (a second consecutive call is a no-op), and `immutable_store_contains_block_default_matches_get_block` (pins the trait-default delegation contract).
- `ChainDb` now exposes typed `LedgerStateCheckpoint` save/load helpers plus typed ledger recovery replay via `recover_ledger_state()` and checkpoint coordination helpers (`clear_ledger_checkpoints`, `truncate_ledger_checkpoints_after_point`, `persist_ledger_checkpoint`) over the existing raw-byte `LedgerStore` seam, giving recovery code a crate-owned path without hard-coding a permanent on-disk format into the trait.
- `recover_ledger_state()` uses a fallback strategy: when the latest checkpoint is corrupt (CBOR decode failure), it iterates backward through older snapshots via `try_restore_checkpoint()` until a valid one is found, falling through to the base ledger state when all checkpoints are unreadable. This makes recovery resilient to partial or interrupted checkpoint writes.
- File-backed stores (`FileImmutable`, `FileVolatile`, `FileLedgerStore`) use atomic file writes (write-to-temp + `fs::rename`) for crash safety, preventing corrupt on-disk state from incomplete writes.
- `FileVolatile` now persists a delete-plan WAL (`wal.pending.json`) for multi-step delete mutations (`prune_up_to`, `rollback_to`, `garbage_collect`) and replays that plan on open, so interrupted delete sequences are completed deterministically during recovery.
- Storage operates on typed `Block`, `HeaderHash`, `SlotNo`, and `Point` from `yggdrasil-ledger`.
- Integration coverage now includes trait behavior, ChainDb coordination for promotion, rollback, and snapshot lookup/truncation across in-memory and file-backed paths, checkpoint fallback recovery tests, and atomic write verification.
- File-backed block stores (`FileImmutable`, `FileVolatile`) now persist blocks as CBOR (`*.cbor`) and keep backward-compatible reads for legacy JSON (`*.json`) block files. `FileLedgerStore` remains raw-byte (`*.dat`) for typed checkpoint encoding flexibility.
- During open, when both CBOR and legacy JSON files exist for the same block hash, file-backed block stores deduplicate to a single in-memory block and prefer the CBOR representation.
- Active crash recovery on stale `dirty.flag`: all three file-backed stores (`FileImmutable`, `FileVolatile`, `FileLedgerStore`) now actively recover when a stale sentinel is found on open — incomplete `.tmp` files from interrupted atomic writes are deleted, and the dirty flag itself is cleared after the recovery scan completes so subsequent opens do not emit spurious warnings.

