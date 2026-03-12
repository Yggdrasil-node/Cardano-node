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
- Storage naming MUST stay close to official node and Ouroboros consensus terminology such as immutable DB, volatile DB, ChainDB, and snapshots.
- Storage behavior MUST be explained with reference to the official node and upstream Ouroboros consensus implementation notes.

## Upstream References
- Consensus core package, including ChainDB and storage concerns: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository docs and reports: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Node integration reference for operational storage concerns: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Keep implementations simple and in-memory friendly while interfaces stabilize.
- Delay irreversible file-format decisions until consensus and ledger expectations are clearer.

