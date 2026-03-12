# Specification Sources

Yggdrasil is specification-driven. When sources disagree, use them in this order.

## Priority Order
1. Formal ledger specifications and protocol papers.
2. Cardano ledger CDDL schemas.
3. Accepted CIPs that define era or protocol behavior.
4. Haskell node behavior for compatibility checks and fixture validation.

## Core References (add or update as needed)
- Cardano ledger CDDL schemas: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras>
- Formal ledger specifications: <https://github.com/IntersectMBO/formal-ledger-specifications>
- Ouroboros papers: <https://iohk.io/research/papers/>
- Cardano blueprint: <https://cardano-scaling.github.io/cardano-blueprint/>

## Usage Rules
- Pin the exact upstream revision used for generated artifacts.
- Keep generated code reproducible from checked-in source specifications.
- Add fixture provenance for any Haskell parity test data.
- The current `crates/crypto` 80-byte Praos VRF fixtures are draft03-era vectors mirrored from `cardano-crypto-praos`; do not treat them as RFC 9381 final-format verification fixtures without explicit translation or replacement.

## Vendored Upstream Test Vectors
- Vendored cryptographic vectors live under `specs/upstream-test-vectors/` with pinned upstream commit provenance.
- Current pinned `cardano-base` source revision: `db52f43b38ba5d8927feb2199d4913fe6c0f974d`.
- Included corpora:
	- `cardano-crypto-praos/test_vectors/` (Praos VRF vectors)
	- `cardano-crypto-class/bls12-381-test-vectors/test_vectors/` (BLS12-381 vectors)
- Crypto integration tests in `crates/crypto/tests/upstream_vectors.rs` validate that the vendored files are present, well-formed, and aligned with embedded standard VRF fixtures where applicable.
