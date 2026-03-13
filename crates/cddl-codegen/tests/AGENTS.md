---
name: cddl-codegen-tests
description: Guidance for parser and generator regression coverage in the cddl-codegen crate.
---

Keep tests in this directory focused on deterministic parser and generator behavior.

## Scope
- Parser fixtures and generator regression tests.
- Reproducibility checks for pinned CDDL inputs and generated output shape.

##  Rules *Non-Negotiable*
- Tests here MUST validate supported syntax and generation behavior, not speculative future grammar.
- Fixture inputs MUST remain pinned and traceable to upstream or documented local reductions.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Era CDDL roots used for reduced fixtures: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Ledger binary support library: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary/>

## Current Phase
- Tests in this directory protect the supported CDDL subset and deterministic Rust output generation against fixture regressions.
- Keep coverage aligned with `specs/mini-ledger.cddl` and the supported subset documented by the crate.