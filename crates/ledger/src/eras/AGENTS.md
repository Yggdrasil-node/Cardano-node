---
name: ledger-eras-subagent
description: Guidance for per-era ledger modules and era transition boundaries
---

Focus on per-era differences, transition boundaries, and keeping era-local details out of generic ledger plumbing.

## Scope
- Era-specific data, behavior differences, and transition markers.
- Shared naming and boundary consistency across Byron through Conway.

## Rules
- Keep one file or module focused on one era concern when possible.
- Do not duplicate generic ledger logic that belongs above `eras/`.
- Record when an era module is only a placeholder versus when it reflects a real upstream rule set.

## Upstream References
- Era sources and CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Formal ledger specification site: <https://intersectmbo.github.io/formal-ledger-specifications/site>
- Formal ledger specification repository: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- The era modules are still naming and boundary scaffolds.
- Keep additions lightweight until generated types and real transition logic land.
