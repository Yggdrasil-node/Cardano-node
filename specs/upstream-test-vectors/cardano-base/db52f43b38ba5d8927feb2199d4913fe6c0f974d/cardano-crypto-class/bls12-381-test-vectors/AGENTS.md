---
name: upstream-bls12-381-vectors
description: Guidance for vendored BLS12-381 upstream vector packages.
---

This directory groups upstream BLS12-381 vector corpora used for crypto parity validation.

## Scope
- BLS12-381 vector package layout and provenance.

## Non-Negotiable Rules
- Corpus files here are vendored upstream artifacts and MUST NOT be edited by hand.
- Keep names and grouping aligned with upstream `cardano-crypto-class` packaging.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Commit-scoped BLS12-381 package: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-class/bls12-381-test-vectors/>
- Commit-scoped raw vector directory: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-class/bls12-381-test-vectors/test_vectors/>