---
name: storage-crate-agent
description: Guidance for durable storage and snapshot work
---

Focus on rollback-aware persistence interfaces and stable on-disk boundaries.

## Scope
- Immutable storage, volatile rollback windows, and snapshot persistence.
- Interfaces that consensus and node integration can build on without file-format lock-in.

## Non-Negotiable Rules
- Storage traits MUST be designed before file formats are treated as stable.
- Immutable and volatile concerns MUST remain separate.
- The design MUST preserve a path toward crash recovery and future migrations.
- Public storage interfaces MUST have Rustdocs when persistence guarantees, rollback behavior, or snapshot semantics matter to callers.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Storage naming MUST stay close to official node and Ouroboros consensus terminology such as immutable DB, volatile DB, ChainDB, and snapshots.
- Storage behavior MUST be explained with reference to the official node and upstream Ouroboros consensus implementation notes.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Consensus core package, including ChainDB and storage concerns: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository docs and reports: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Node integration reference for operational storage concerns: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Traits `ImmutableStore`, `VolatileStore`, and `LedgerStore` are landed and exported.
- In-memory implementations (`InMemoryImmutable`, `InMemoryVolatile`, `InMemoryLedgerStore`) back each trait.
- File-backed implementations (`FileImmutable`, `FileVolatile`, `FileLedgerStore`) provide JSON-based on-disk persistence with directory scanning on open, rollback-aware file deletion, and re-open persistence.
- Storage operates on typed `Block`, `HeaderHash`, `SlotNo`, and `Point` from `yggdrasil-ledger`.
- 19 integration tests cover all trait methods for both in-memory and file-backed implementations.
- File-backed stores use `serde_json` serialization; this is a pragmatic initial format, not a long-term commitment.

