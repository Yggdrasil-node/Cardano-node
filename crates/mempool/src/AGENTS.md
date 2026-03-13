---
name: mempool-src
description: Guidance for mempool queue, snapshot, and admission implementation modules.
---

This directory owns queue policy and typed mempool views, not ledger validation semantics.

## Scope
- Queue ordering, snapshot traversal, duplicate detection, and eviction helpers.
- Shared and non-shared mempool reader implementations.

## Non-Negotiable Rules
- Queue semantics MUST remain explicit and locally testable from this directory.
- Networking protocol concerns MUST not leak into mempool internals here.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Current Focus
- Preserve the separation between fee ordering and TxSubmission snapshot ordering.