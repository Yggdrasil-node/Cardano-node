---
name: storage-crate-agent
description: Guidance for durable storage and snapshot work
---

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
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Cardano dbsync storage reference: <https://github.com/IntersectMBO/cardano-db-sync>
- Consensus core package, including ChainDB and storage concerns: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository docs and reports: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Node integration reference for operational storage concerns: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Traits `ImmutableStore`, `VolatileStore`, and `LedgerStore` are landed and exported.
- In-memory implementations (`InMemoryImmutable`, `InMemoryVolatile`, `InMemoryLedgerStore`) back each trait.
- File-backed implementations (`FileImmutable`, `FileVolatile`, `FileLedgerStore`) provide JSON-based on-disk persistence with directory scanning on open, rollback-aware file deletion, and re-open persistence.
- A minimal `ChainDb` coordinator now lives in the crate to coordinate immutable, volatile, and ledger snapshot stores without pulling sync or consensus policy into `node`. It exposes best-known tip recovery, volatile-prefix promotion into immutable storage, and ledger-snapshot truncation on rollback; rollbacks to immutable points must clear the volatile suffix so the coordinated tip realigns with immutable storage.
- `VolatileStore` now exposes ordered prefix access and pruning helpers so stable volatile blocks can be immutalized through a crate-owned boundary instead of ad hoc node-side coordination.
- `LedgerStore` now supports latest-snapshot lookup at or before a slot plus snapshot truncation after rollback, so restart and rollback flows can reuse the storage layer directly.
- `ChainDb` now exposes typed `LedgerStateCheckpoint` save/load helpers plus typed ledger recovery replay via `recover_ledger_state()` over the existing raw-byte `LedgerStore` seam, giving recovery code a crate-owned path without hard-coding a permanent on-disk format into the trait.
- Storage operates on typed `Block`, `HeaderHash`, `SlotNo`, and `Point` from `yggdrasil-ledger`.
- Integration coverage now includes trait behavior plus ChainDb coordination for promotion, rollback, and snapshot lookup/truncation across in-memory and file-backed paths.
- File-backed stores use `serde_json` serialization; this is a pragmatic initial format, not a long-term commitment.

