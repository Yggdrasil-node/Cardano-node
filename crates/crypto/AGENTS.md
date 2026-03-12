---
name: crypto-crate-agent
description: Guidance for pure Rust Cardano cryptography work
---

Focus on pure Rust implementations for hashing, signatures, VRF, and KES.

## Scope
- Hashing, signing, VRF, KES, and cryptographic encodings.
- Stable interfaces used by ledger, consensus, and networking code.

## Non-Negotiable Rules
- Secret comparisons MUST remain constant-time.
- Dependencies MUST be audited for hidden FFI, native build steps, and parity risks before adoption.
- Public interfaces MUST remain stable unless a breaking change is clearly justified by protocol correctness.
- Test vectors MUST exist before any claim of protocol compatibility is accepted.
- Every public cryptographic type and function that defines protocol-relevant behavior, encoding, or security expectations MUST have proper Rustdocs.
- Names MUST stay close to the official node, Cardano specs, and upstream crypto terminology unless a Rust-specific deviation is clearly justified.
- Parity-sensitive choices MUST be explained by reference to the official `cardano-node` ecosystem and the relevant upstream IntersectMBO crypto packages.
- Full cryptographic parity, vector coverage, and encoding compatibility are non-negotiable long-term targets.

## Upstream References
- Crypto abstractions and shared utilities: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/>
- Praos-oriented crypto behavior and vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/>
- Shared Cardano base packages: <https://github.com/IntersectMBO/cardano-base/>

## Current Phase
- Blake2b and Ed25519 are the active baseline.
- Prefer vector-backed incremental progress before deeper VRF or KES work.
