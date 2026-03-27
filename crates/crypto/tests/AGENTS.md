---
name: crypto-tests
description: Guidance for cryptographic vector and regression tests in the crypto crate.
---

Use this directory for deterministic vector-backed crypto validation.

## Scope
- Upstream vector ingestion tests.
- Encoding, signing, verification, and tamper-rejection regressions.

##  Rules *Non-Negotiable*
- New cryptographic behavior MUST land with vectors or deterministic fixtures in this directory.
- Do not weaken exact-byte or parity assertions into loose shape-only checks.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- Praos VRF/KES test vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/test_vectors/>
- BLS12-381 test vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/bls12-381-test-vectors/test_vectors/>
- `cardano-crypto-class` tests: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/test>
- `cardano-crypto-praos` tests: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/test>
- `cardano-base` root: <https://github.com/IntersectMBO/cardano-base/tree/master/>

## Current Phase
- Tests in this directory validate Ed25519, KES, Praos VRF proof generation and verification, and upstream vector parity.