---
name: storage-src
description: Guidance for storage trait and implementation modules.
---

This directory owns storage interfaces and implementations, including rollback-aware persistence behavior.

## Scope
- Immutable, volatile, and ledger store implementations.
- Persistence helpers and typed snapshot interfaces.

## Non-Negotiable Rules
- Persistence semantics here MUST remain explicit and independent of node orchestration code.
- File-format pragmatism is acceptable, but migration and recovery paths MUST remain possible.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Consensus storage and ChainDB context: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/>
- Node storage integration reference: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Storage source modules currently provide in-memory and file-backed implementations behind stable traits while leaving room for future format and recovery upgrades.