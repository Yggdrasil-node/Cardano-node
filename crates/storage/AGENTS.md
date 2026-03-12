---
name: storage-crate-agent
description: Guidance for durable storage and snapshot work
---

Focus on rollback-aware persistence interfaces and stable on-disk boundaries.

## Scope
- Immutable storage, volatile rollback windows, and snapshot persistence.
- Interfaces that consensus and node integration can build on without file-format lock-in.

## Rules
- Design storage traits before committing to file formats.
- Keep immutable and volatile concerns separate.
- Preserve a path toward crash recovery and future migrations.

## Upstream References
- Consensus core package, including ChainDB and storage concerns: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>
- Consensus repository docs and reports: <https://github.com/IntersectMBO/ouroboros-consensus>
- Node integration reference for operational storage concerns: <https://github.com/IntersectMBO/cardano-node>

## Current Phase
- Keep implementations simple and in-memory friendly while interfaces stabilize.
- Delay irreversible file-format decisions until consensus and ledger expectations are clearer.

