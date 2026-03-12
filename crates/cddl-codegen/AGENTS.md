---
name: cddl-codegen-crate-agent
description: Guidance for generating Rust types from pinned Cardano CDDL sources
---

Focus on deterministic parsing and reproducible generated artifacts.

## Scope
- Parsing pinned CDDL inputs and generating Rust-facing type output.
- Separating supported subsets from future parsing ambitions.

## Non-Negotiable Rules
- Upstream schemas MUST be treated as pinned inputs and the exact revision used MUST be recorded.
- Handwritten parser logic and generated output templates MUST remain separated.
- Small supported CDDL subsets with tests MUST be preferred over broad speculative parsing.
- Generated output MUST NOT be edited by hand.
- Public parser and generator entry points MUST have Rustdocs when supported syntax, failure modes, or output guarantees are not obvious.
- Generated type and field naming MUST remain traceable to upstream ledger schemas and official node terminology.
- Schema handling and generated output MUST be explained with reference to the official node path through cardano-ledger and related IntersectMBO sources.

## Upstream References
- Era CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Ledger binary and supporting libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary>
- Ledger support libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications>

## Current Phase
- Support a narrow tested subset first: aliases, flat maps, arrays, and builtin type mapping.
- Expand coverage only when fixtures and generation expectations are pinned.
