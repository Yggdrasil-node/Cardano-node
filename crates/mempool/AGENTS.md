---
name: mempool-crate-agent
description: Guidance for transaction admission, ordering, and eviction work
---

Focus on deterministic transaction intake and on keeping ledger validation and queue policy separate.

## Scope
- Transaction admission, prioritization, eviction, and rollback-aware removal.
- Boundaries between queue policy, ledger validation, and network submission.

## Rules
- Keep admission checks and prioritization logic explicit and testable.
- Avoid coupling mempool ordering to networking concerns.
- Treat ledger validation as an input contract, not a hidden side effect.
- Design for rollback and block-application eviction from the start.

## Upstream References
- Consensus core package, including mempool design context: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>
- Consensus repository root: <https://github.com/IntersectMBO/ouroboros-consensus>
- Transaction submission integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api>
- Node integration reference: <https://github.com/IntersectMBO/cardano-node>

## Current Phase
- Keep the queue deterministic and simple while ledger validation contracts are still evolving.
- Add concurrency and richer policy only after admission semantics are stable.
