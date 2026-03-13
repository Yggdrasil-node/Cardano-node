---
name: upstream-bls12-381-vector-files
description: Guidance for the raw vendored BLS12-381 vector files.
---

This directory contains raw upstream BLS12-381 vector files.

## Scope
- File-level vector corpora consumed by crypto tests.

##  Rules *Non-Negotiable*
- Files here are raw vendored fixtures and MUST remain byte-for-byte upstream copies.
- Additions or refreshes MUST come from the pinned upstream source, not local editing.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Commit-scoped raw vector directory: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-class/bls12-381-test-vectors/test_vectors/>