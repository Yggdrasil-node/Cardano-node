# The official upstream vector and golden artifacts used for parity validation.

This directory vendors official upstream vector and golden artifacts used for parity validation. Research the official IntersectMBO/Cardano repositories to find the relevant files and paths for implementation work, and use the crate-local `AGENTS.md` files to identify which corpora matter to each subsystem.

## Scope
- Pinned upstream vector corpora used by workspace parity tests.
- Provenance and sync guidance for fixture updates under commit-scoped directories.
- Read-only fixture organization, not feature implementation logic.

## Source Policy

- Source repositories are official IntersectMBO/Cardano repositories.
- Files are stored under a pinned upstream commit path for reproducibility.
- Vendored files must not be hand-edited.

##  Rules *Non-Negotiable*

- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- Upstream `cardano-base` repository: <https://github.com/IntersectMBO/cardano-base/>
- Praos vector path: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/test_vectors/>
- BLS12-381 vector path: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/bls12-381-test-vectors/test_vectors/>

## Vendored Set (Current)

- Repository: `IntersectMBO/cardano-base`
- Commit: `7a8a991945d401d89e27f53b3d3bb464a354ad4c`
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
