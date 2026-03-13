---
name: upstream-cardano-crypto-praos
description: Guidance for vendored cardano-crypto-praos fixture content.
---

This directory contains vendored Praos crypto fixture content from upstream `cardano-crypto-praos`.

## Scope
- Praos VRF vector layout and upstream provenance.

## Non-Negotiable Rules
- Maintain upstream naming and corpus layout exactly.
- Treat this directory as read-only vendored input.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Commit-scoped package root: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-praos/>
- Commit-scoped vector directory: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-praos/test_vectors/>