---
name: consensus-tests
description: Guidance for consensus crate tests and parity-oriented fixtures.
---

Use this directory for deterministic tests of consensus behavior and boundary conditions.

## Scope
- Roll-forward and rollback tests.
- Nonce evolution, header verification, and leadership threshold coverage.

## Non-Negotiable Rules
- Tests here MUST prefer protocol edge cases and rollback invariants over broad smoke coverage.
- Reproducible fixtures MUST back any claim of parity-sensitive consensus behavior.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"