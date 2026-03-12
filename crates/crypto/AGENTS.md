---
name: crypto-crate-agent
description: Guidance for pure Rust Cardano cryptography work
---

Focus on pure Rust implementations for hashing, signatures, VRF, and KES.

## Scope
- Hashing, signing, VRF, KES, and cryptographic encodings.
- Stable interfaces used by ledger, consensus, and networking code.

## Rules
- Keep secret comparisons constant-time.
- Audit dependencies for hidden FFI or native build steps.
- Preserve stable public interfaces for downstream crates.
- Add test vectors before claiming protocol compatibility.

## Upstream References
- Crypto abstractions and shared utilities: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class>
- Praos-oriented crypto behavior and vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos>
- Shared Cardano base packages: <https://github.com/IntersectMBO/cardano-base>

## Current Phase
- Blake2b and Ed25519 are the active baseline.
- Prefer vector-backed incremental progress before deeper VRF or KES work.
