---
name: upstream-cardano-base-commit
description: Guidance for the pinned cardano-base commit snapshot used by vendored test vectors.
---

This directory corresponds to a single pinned upstream commit snapshot.

## Scope
- Vendored artifacts from `IntersectMBO/cardano-base` commit `db52f43b38ba5d8927feb2199d4913fe6c0f974d`.

##  Rules *Non-Negotiable*
- Preserve this directory as a faithful commit-scoped snapshot.
- Any refresh MUST use a new commit directory rather than mutating provenance in place.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Commit tree root: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/>
- Commit-scoped `cardano-crypto-praos`: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-praos/>
- Commit-scoped `cardano-crypto-class`: <https://github.com/IntersectMBO/cardano-base/tree/db52f43b38ba5d8927feb2199d4913fe6c0f974d/cardano-crypto-class/>