---
name: cddl-codegen-tests
description: Guidance for parser and generator regression coverage in the cddl-codegen crate.
---

Keep tests in this directory focused on deterministic parser and generator behavior.

## Scope
- Parser fixtures and generator regression tests.
- Reproducibility checks for pinned CDDL inputs and generated output shape.

## Non-Negotiable Rules
- Tests here MUST validate supported syntax and generation behavior, not speculative future grammar.
- Fixture inputs MUST remain pinned and traceable to upstream or documented local reductions.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Current Focus
- Keep coverage aligned with `specs/mini-ledger.cddl` and the supported subset documented by the crate.