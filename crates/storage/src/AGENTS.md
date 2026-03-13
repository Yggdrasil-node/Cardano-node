---
name: storage-src
description: Guidance for storage trait and implementation modules.
---

This directory owns storage interfaces and implementations, including rollback-aware persistence behavior.

## Scope
- Immutable, volatile, and ledger store implementations.
- Persistence helpers and typed snapshot interfaces.

## Non-Negotiable Rules
- Persistence semantics here MUST remain explicit and independent of node orchestration code.
- File-format pragmatism is acceptable, but migration and recovery paths MUST remain possible.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"