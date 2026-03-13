# Dependency Policy

This file defines how dependencies are introduced into Yggdrasil.

## Approved Now
- `blake2`: pure Rust hashing for Blake2b-based primitives.
- `clap`: pure Rust CLI argument parser with derive macros; needed for the node binary's command-line interface. Rejected alternative was manual `std::env::args` parsing which provides no help text, completion, or structured subcommands. No native build requirements.
- `curve25519-dalek`: curve operations used in the crypto foundation.
- `curve25519-elligator2`: pure Rust legacy elligator2 mapping support needed to mirror `cardano-crypto-praos` batch-compatible VRF hash-to-curve behavior; rejected alternatives were FFI bindings to libsodium/cardano C code and ad-hoc local finite-field reimplementations, and this crate introduces no hidden native build requirements.
- `ed25519-dalek`: pure Rust Ed25519 signing and verification built on RustCrypto and dalek primitives.
- `sha2`: pure Rust SHA-512 required for Praos-compatible VRF proof-to-output hashing; rejected alternatives were hidden FFI wrappers and reimplementing SHA-512 locally, and it introduces no native build requirements.
- `serde`: structured data interchange where handwritten or generated types require it.
- `serde_json`: JSON serialization/deserialization for node configuration files; natural companion to `serde` with no additional native requirements. Matches the Cardano node's JSON-based configuration format.
- `thiserror`: library error types.
- `eyre`: binary error reporting.
- `subtle`: constant-time comparisons for secret material.
- `tokio`: async runtime for networking and orchestration work.
- `zeroize`: deterministic zeroing of secret material on drop; already a transitive dependency via `curve25519-dalek` and `ed25519-dalek`, so adding it directly introduces no new supply chain surface.

## Review Required
- Any new cryptography crate.
- Any dependency that enables native code, assembly, or bundled C libraries.
- Any storage dependency that constrains on-disk format or migration strategy.
- Any parser or code generation framework used by `cddl-codegen`.

## Forbidden
- Haskell runtime bindings.
- C-backed cryptography wrappers.
- Dependencies that hide FFI behind default features.

## Process
When adding a new dependency, record why it is needed, what alternatives were rejected, and whether the crate brings in any native toolchain requirements.
