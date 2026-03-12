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
- Blake2b, Ed25519, and SimpleKES (two-period) are implemented with vector coverage.
- VRF batchcompat (ietfdraft13, 128-byte proof) verification is complete and passes all 7 upstream vectors.
- VRF standard (ietfdraft03, 80-byte proof) verification is complete and passes all 7 upstream vectors.
  - H2C uses `SHA-512(SUITE||ONE||pk||alpha)` → first 32 bytes → clear bit 255 → Elligator2 `from_representative::<Legacy>` → normalize Edwards X sign (clear bit 7 of compressed byte 31) → decompress → cofactor multiply.
  - Challenge uses `SHA-512(SUITE||TWO||H_string||gamma||U||V)` → first 16 bytes (no pk, no trailing ZERO — differs from batchcompat).
  - Sign normalization is required because `from_representative::<Legacy>` does NOT force non-negative X, unlike upstream C `ge25519_from_uniform`.
- VRF proof generation (`prove`) is not yet implemented.
- Next priority: VRF proof generation.
