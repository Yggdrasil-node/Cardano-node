---
name: ledger-crate-agent
description: Guidance for era-aware ledger work
---

Focus on reusable state-transition interfaces and explicit era boundaries.

## Scope
- Era modeling, transaction and block state transitions, and ledger state evolution.
- Separation between generated wire types and handwritten rules.

## Rules
- Keep specification provenance close to each ledger rule.
- Separate generated data types from handwritten transition logic.
- Build a full era roadmap, but implement one narrow slice at a time.

## Upstream References
- Ledger repository root: <https://github.com/IntersectMBO/cardano-ledger>
- Era-specific sources and CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications>
- Published formal spec site: <https://intersectmbo.github.io/formal-ledger-specifications/site>

## Current Phase
- Keep the full era roadmap visible, but land only narrow reusable slices.
- Prefer types and harnesses that will survive later era expansion.
