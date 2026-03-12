---
name: cddl-codegen-crate-agent
description: Guidance for generating Rust types from pinned Cardano CDDL sources
---

Focus on deterministic parsing and reproducible generated artifacts.

## Scope
- Parsing pinned CDDL inputs and generating Rust-facing type output.
- Separating supported subsets from future parsing ambitions.

## Rules
- Treat upstream schemas as pinned inputs and record the exact revision used.
- Keep handwritten parser logic separate from generated output templates.
- Prefer small supported CDDL subsets with tests over broad speculative parsing.
- Do not edit generated output by hand.

## Upstream References
- Era CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Ledger binary and supporting libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary>
- Ledger support libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Support a narrow tested subset first: aliases, flat maps, arrays, and builtin type mapping.
- Expand coverage only when fixtures and generation expectations are pinned.
