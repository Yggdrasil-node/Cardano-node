---
name: Workspace Specs
description: Guidance for pinned specification inputs and vendored upstream artifacts under specs/.
---

This directory holds reproducible specification inputs and pinned upstream artifacts used by the workspace.

## Scope
- Checked-in specification inputs such as `mini-ledger.cddl`.
- Vendored upstream corpora and fixtures under child spec directories.
- Provenance tracking for pinned revisions used by generators or parity tests.

## Non-Negotiable Rules
- Specification inputs in this directory MUST remain traceable to an upstream source or an explicitly documented local derivation.
- Vendored upstream artifacts MUST NOT be hand-edited.
- Pinned revisions used for generation or validation MUST be recorded alongside the affected implementation or tests.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Current Contents
- `mini-ledger.cddl`: reduced pinned CDDL fixture used by `crates/cddl-codegen` tests and examples.
- `upstream-test-vectors/`: pinned official upstream vector corpora with separate folder-specific guidance.

## Maintenance Guidance
- When a pinned spec input changes, update its provenance in `docs/SPECS.md` and any affected crate guidance.
- Keep local reduced fixtures minimal and deterministic so regeneration and parser tests remain reproducible.