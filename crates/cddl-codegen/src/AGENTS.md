---
name: cddl-codegen-src-subagent
description: Guidance for parser and generator internals in cddl-codegen
---

Focus on small parser and generator internals that are deterministic, testable, and easy to extend without hidden heuristics.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.

## Scope
- Parser modules, generator modules, builtin type mapping, and fixture-driven behavior.
- Internal structure that supports reproducible generation from pinned schemas.

## Non-Negotiable Rules
- Parsing and generation concerns MUST remain separated.
- Supported syntax MUST only be added with focused tests and explicit output expectations.
- Conservative failures MUST be preferred over permissive guessing when schema input is ambiguous.
- Public parser and generator internals that define supported syntax boundaries, normalization rules, or output guarantees MUST have Rustdocs.
- Names MUST remain traceable to upstream schema terminology and official node-adjacent ledger naming wherever practical.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Era CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Ledger binary library: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary/>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications/>

## Current Phase
- Parser AST uses `TypeExpr` enum (Named, Sized, VarArray, Optional) for type expressions, `FieldKey` enum (Label, Index) for map keys, and `ArrayItem` with optional names for array elements.
- The supported subset is intentionally explicit and fixture-driven.
- Expand only where pinned fixtures demonstrate a concrete need.