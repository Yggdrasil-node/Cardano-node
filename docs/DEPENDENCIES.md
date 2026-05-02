---
title: Dependencies
layout: default
parent: Reference
nav_order: 7
---

# Dependency Policy

This file defines how dependencies are introduced into Yggdrasil.

## Approved Now
- `blake2`: pure Rust hashing for Blake2b-based primitives.
- `clap`: pure Rust CLI argument parser with derive macros; needed for the node binary's command-line interface. Rejected alternative was manual `std::env::args` parsing which provides no help text, completion, or structured subcommands. No native build requirements.
- `curve25519-dalek`: curve operations used in the crypto foundation.
- `curve25519-elligator2`: pure Rust legacy elligator2 mapping support needed to mirror `cardano-crypto-praos` batch-compatible VRF hash-to-curve behavior; rejected alternatives were FFI bindings to libsodium/cardano C code and ad-hoc local finite-field reimplementations, and this crate introduces no hidden native build requirements.
- `ed25519-dalek`: pure Rust Ed25519 signing and verification built on RustCrypto and dalek primitives.
- `k256`: pure Rust secp256k1 elliptic curve implementation from RustCrypto; provides ECDSA (`PrehashVerifier`) and Schnorr (BIP-340) signature verification required by PlutusV2 builtins (`VerifyEcdsaSecp256k1Signature`, `VerifySchnorrSecp256k1Signature`). Rejected alternatives were `secp256k1` crate (wraps C libsecp256k1 — forbidden FFI) and `p256`/local reimplementation. Features enabled: `ecdsa`, `schnorr`, `std`. No native build requirements.
- `num-bigint`, `num-integer`, `num-traits`: pure Rust arbitrary-precision integer arithmetic from the `num` crate family; required by the consensus crate for deterministic Praos leader-value check (`check_leader_value`) and by the ledger/Plutus crates for upstream-compatible `Integer` handling in Plutus constants, `PlutusData`, CBOR bignums, and integer builtins. The upstream Haskell node uses `Natural` and `Rational` for the VRF output comparison and unbounded `Integer` for Plutus arithmetic; these crates provide the equivalent Rust primitives. Rejected alternatives were ad-hoc inline 512-bit integer implementations (error-prone and difficult to audit), fixed-width `i128`/`u256` limits (valid on-chain Plutus scripts can exceed them), and f64 floating-point arithmetic (non-deterministic across platforms). No native build requirements.
- `bls12_381`: pure Rust BLS12-381 pairing-friendly elliptic curve implementation from zkcrypto; required by CIP-0381 PlutusV3 BLS builtins (17 primitives: G1/G2 arithmetic, compress/uncompress, hash-to-curve, Miller loop, and final verify). Rejected alternatives were `blst` crate (wraps C code — forbidden FFI) and `ark-bls12-381` (large dependency tree with optional assembly backends). Features enabled: `experimental` (required for `HashToCurve` trait). Depends on `digest 0.9` which requires a companion `sha2 0.9` for hash-to-curve operations (see `sha2_09` below). No native build requirements.
- `sha2_09` (renamed `sha2 = "0.9"`): provides `digest 0.9`-compatible SHA-256 required by `bls12_381 0.8`'s `ExpandMsgXmd` hash-to-curve expander. The workspace's primary `sha2 0.10` uses `digest 0.10`, which is a different major version with incompatible trait signatures. This renamed dependency is scoped to the crypto crate's BLS12-381 module only. No native build requirements.
- `ripemd`: pure Rust RIPEMD-160 hash from RustCrypto; required by PlutusV3 builtin `Ripemd_160`. No alternative exists in the pure Rust ecosystem. No native build requirements.
- `sha2`: pure Rust SHA-512 required for Praos-compatible VRF proof-to-output hashing; rejected alternatives were hidden FFI wrappers and reimplementing SHA-512 locally, and it introduces no native build requirements.
- `sha3`: pure Rust SHA3-256 and Keccak-256 hashes from RustCrypto; required by PlutusV2 builtin `Sha3_256` and PlutusV3 builtin `Keccak_256`. No alternative exists in the pure Rust ecosystem. No native build requirements.
- `serde`: structured data interchange where handwritten or generated types require it.
- `serde_json`: JSON serialization/deserialization for node configuration files; natural companion to `serde` with no additional native requirements. Matches the Cardano node's JSON-based configuration format.
- `serde_yaml`: pure Rust YAML serialization/deserialization used by the node CLI config loader as a fallback alongside JSON (`load_effective_config`), matching operator workflows that use YAML-formatted config files. Rejected alternatives were (1) hard-failing on non-JSON config files despite upstream ecosystem YAML usage and (2) introducing a custom ad-hoc YAML parser. No native build requirements.
- `serde_cbor`: pure Rust CBOR serialization/deserialization for file-backed storage block payloads (`FileImmutable`/`FileVolatile`) to move persisted block blobs from JSON to deterministic binary encoding while retaining legacy JSON read compatibility during migration. Rejected alternatives were (1) adding ad-hoc block CBOR codec duplication inside storage before ledger-level `Block` CBOR traits exist, and (2) keeping JSON block persistence, which is larger/slower and less parity-aligned with upstream binary storage expectations. No native build requirements.
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
