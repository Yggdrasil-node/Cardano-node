---
name: cddl-codegen-src-subagent
description: Guidance for parser and generator internals in cddl-codegen
---

Focus on small parser and generator internals that are deterministic, testable, and easy to extend without hidden heuristics.

## Scope
- Parser modules, generator modules, builtin type mapping, and fixture-driven behavior.
- Internal structure that supports reproducible generation from pinned schemas.

## Rules
- Keep parsing and generation concerns separated.
- Add supported syntax only with focused tests and explicit output expectations.
- Prefer conservative failures over permissive guessing when schema input is ambiguous.

## Upstream References
- Era CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Ledger binary library: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- The supported subset is intentionally small and explicit.
- Expand only where pinned fixtures demonstrate a concrete need.