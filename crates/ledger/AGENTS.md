---
name: ledger-crate-agent
description: Guidance for era-aware ledger work
---

Focus on reusable state-transition interfaces and explicit era boundaries.

## Rules
- Keep specification provenance close to each ledger rule.
- Separate generated data types from handwritten transition logic.
- Build a full era roadmap, but implement one narrow slice at a time.
