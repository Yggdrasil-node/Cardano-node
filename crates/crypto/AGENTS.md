---
name: crypto-crate-agent
description: Guidance for pure Rust Cardano cryptography work
---

Focus on pure Rust implementations for hashing, signatures, VRF, and KES.

## Rules
- Keep secret comparisons constant-time.
- Audit dependencies for hidden FFI or native build steps.
- Preserve stable public interfaces for downstream crates.
- Add test vectors before claiming protocol compatibility.

## Current Phase
- Foundation only. Interfaces and harnesses are acceptable placeholders.
- Prefer Blake2b and key material types before deeper VRF or KES work.
