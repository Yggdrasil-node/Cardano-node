---
name: Upstream Test Vectors
description: The official upstream vector and golden artifacts used for parity validation.
---
# Upstream Test Vectors

This directory vendors official upstream vector and golden artifacts used for parity validation. you need to research the official IntersectMBO/Cardano repositories to find the relevant files and paths for your implementation work, and you can use the `AGENTS.md` files in each crate for guidance on where to look for new vectors.

## Source Policy

- Source repositories are official IntersectMBO/Cardano repositories.
- Files are stored under a pinned upstream commit path for reproducibility.
- Vendored files must not be hand-edited.

## Non-Negotiable Rules

- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Vendored Set (Current)

- Repository: `IntersectMBO/cardano-base`
- Commit: `db52f43b38ba5d8927feb2199d4913fe6c0f974d`
- Paths:
  - `cardano-crypto-praos/test_vectors`
  - `cardano-crypto-class/bls12-381-test-vectors/test_vectors`

## Layout

- `cardano-base/<commit>/cardano-crypto-praos/test_vectors/*`
- `cardano-base/<commit>/cardano-crypto-class/bls12-381-test-vectors/test_vectors/*`

## Sync Guidance

To refresh these files:

1. Choose the target upstream commit from `IntersectMBO/cardano-base`.
2. Download raw files from the two paths above into the corresponding commit directory.
3. Run crypto vector validation tests to confirm shape and fixture parity are preserved.
