---
name: mempool-crate-agent
description: Guidance for transaction admission, ordering, and eviction work
---

Focus on deterministic transaction intake and on keeping ledger validation and queue policy separate.

## Scope
- Transaction admission, prioritization, eviction, and rollback-aware removal.
- Boundaries between queue policy, ledger validation, and network submission.

## Non-Negotiable Rules
- Admission checks and prioritization logic MUST remain explicit and testable.
- Mempool ordering MUST NOT be coupled to networking concerns.
- Ledger validation MUST be treated as an input contract, not a hidden side effect.
- Rollback and block-application eviction MUST be accounted for from the start.
- Public mempool types and functions MUST have Rustdocs when queue semantics, ordering rules, or eviction behavior matter to callers.
- Naming MUST stay close to official node and consensus mempool terminology.
- Transaction flow and admission policy MUST be explained with reference to the official node and upstream mempool-adjacent sources such as Ouroboros consensus and `cardano-submit-api`.

## Upstream References (add or update as needed)
- Consensus core package, including mempool design context: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository root: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Transaction submission integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>
- Node integration reference: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Mempool uses real ledger `TxId` for transaction identification and carries raw CBOR body bytes.
- Fee-descending ordering is implemented with duplicate detection and capacity enforcement.
- `remove_by_id`, `contains`, `len`, `is_empty`, `size_bytes`, `iter`, and `remove_confirmed` are implemented.
- Block-application eviction via `remove_confirmed` enables post-sync snipping of confirmed transactions.
- TTL-aware admission via `insert_checked(entry, current_slot)` rejects transactions whose TTL has expired.
- Periodic TTL expiry purge via `purge_expired(current_slot)` removes all stale entries.
- Node-side `evict_confirmed_from_mempool` in `node/src/sync.rs` wires mempool eviction into the sync pipeline.
- Next: add concurrency support for multi-producer intake, and integrate with ledger validation for full admission checks.
