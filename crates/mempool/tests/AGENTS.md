---
name: mempool-tests
description: Guidance for mempool admission, ordering, and eviction tests.
---

Use this directory to pin queue semantics and mempool boundary behavior.

## Scope
- Admission and duplicate rejection.
- Ordering, TTL expiry, snapshot traversal, and block-confirmation eviction.

## Non-Negotiable Rules
- Tests here MUST assert queue behavior directly rather than relying on node integration side effects.
- Snapshot and shared-reader coverage MUST protect monotonic cursor semantics.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"