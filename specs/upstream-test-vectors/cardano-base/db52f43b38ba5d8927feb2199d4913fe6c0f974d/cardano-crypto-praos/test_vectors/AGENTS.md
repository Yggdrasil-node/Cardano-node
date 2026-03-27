---
name: upstream-praos-vector-files
description: Guidance for the raw vendored Praos VRF vector files.
---

This directory contains raw upstream Praos VRF vector files used by crypto parity tests.

## Scope
- File-level standard and batch-compatible VRF vector corpora.

##  Rules *Non-Negotiable*
- Files here are raw vendored fixtures and MUST remain byte-for-byte upstream copies.
- Do not rewrite, normalize, or rename upstream vector files locally.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Commit-scoped raw vector directory: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-praos/test_vectors/>