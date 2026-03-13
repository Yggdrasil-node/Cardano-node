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
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References (add or update as needed)
- Consensus mempool design context: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/>
- Submit API integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>

## Current Phase
- Preserve the separation between fee ordering and TxSubmission snapshot ordering.