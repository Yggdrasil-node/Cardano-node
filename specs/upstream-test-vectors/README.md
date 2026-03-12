# Upstream Test Vectors

This directory vendors official upstream vector and golden artifacts used for parity validation.

## Source Policy

- Source repositories are official IntersectMBO/Cardano repositories.
- Files are stored under a pinned upstream commit path for reproducibility.
- Vendored files must not be hand-edited.

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
