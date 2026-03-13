---
name: consensus-src
description: Guidance for consensus source modules implementing typed chain selection and nonce evolution logic.
---

This directory owns consensus implementation modules, not integration glue.

## Scope
- `chain_state`, `nonce`, `header`, `leader`, and operational certificate logic.
- Typed consensus math and verification rules.

## Non-Negotiable Rules
- Consensus math and rollback rules MUST stay explicit and typed.
- Source modules here MUST remain independent of node runtime orchestration concerns.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Current Focus
- Preserve the current separation between header verification, epoch nonce evolution, and volatile chain tracking.