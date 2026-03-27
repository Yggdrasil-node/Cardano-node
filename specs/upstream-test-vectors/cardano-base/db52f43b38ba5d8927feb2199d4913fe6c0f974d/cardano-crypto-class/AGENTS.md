---
name: upstream-cardano-crypto-class
description: Guidance for vendored cardano-crypto-class fixture content.
---

This directory contains vendored fixture content from upstream `cardano-crypto-class`.

## Scope
- BLS12-381 upstream test-vector packages and related fixture layout.

##  Rules *Non-Negotiable*
- Maintain upstream directory naming and corpus layout exactly.
- Treat this directory as read-only vendored input.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- Commit-scoped package root: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-class/>
- Commit-scoped BLS12-381 vectors: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-class/bls12-381-test-vectors/>