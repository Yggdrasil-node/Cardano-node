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

## Upstream References
- Consensus core package, including mempool design context: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- Consensus repository root: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Transaction submission integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>
- Node integration reference: <https://github.com/IntersectMBO/cardano-node/>

## Current Phase
- Keep the queue deterministic and simple while ledger validation contracts are still evolving.
- Add concurrency and richer policy only after admission semantics are stable.
