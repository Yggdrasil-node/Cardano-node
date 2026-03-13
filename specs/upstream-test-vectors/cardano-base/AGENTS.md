---
name: upstream-cardano-base-vectors
description: Guidance for vendored cardano-base upstream fixture layout.
---

This directory is a vendored mirror root for upstream `cardano-base` fixture content.

## Scope
- Pinned commit snapshots of `IntersectMBO/cardano-base` vector material.

## Non-Negotiable Rules
- Do not hand-edit vendored upstream files below this directory.
- Add or update only by syncing from an explicitly pinned upstream commit.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- `cardano-base` repository root: <https://github.com/IntersectMBO/cardano-base/>
- `cardano-crypto-praos` vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/test_vectors/>
- `cardano-crypto-class` BLS12-381 vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/bls12-381-test-vectors/test_vectors/>