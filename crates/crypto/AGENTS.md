---
name: crypto-crate-agent
description: Guidance for pure Rust Cardano cryptography work
---

Focus on pure Rust implementations for hashing, signatures, VRF, and KES.

## Scope
- Hashing, signing, VRF, KES, and cryptographic encodings.
- Stable interfaces used by ledger, consensus, and networking code.

##  Rules *Non-Negotiable*
- Secret comparisons MUST remain constant-time.
- Dependencies MUST be audited for hidden FFI, native build steps, and parity risks before adoption.
- Public interfaces MUST remain stable unless a breaking change is clearly justified by protocol correctness.
- Test vectors MUST exist before any claim of protocol compatibility is accepted.
- Every public cryptographic type and function that defines protocol-relevant behavior, encoding, or security expectations MUST have proper Rustdocs.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Names MUST stay close to the official node, Cardano specs, and upstream crypto terminology unless a Rust-specific deviation is clearly justified.
- Parity-sensitive choices MUST be explained by reference to the official `cardano-node` ecosystem and the relevant upstream IntersectMBO crypto packages.
- Full cryptographic parity, vector coverage, and encoding compatibility are non-negotiable long-term targets.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- Crypto class abstractions (hashing, signatures, VRF, KES): <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/>
- Praos VRF and KES implementations and test vectors: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/>
- Peras-era crypto extensions: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-peras/>
- Shared Cardano base packages: <https://github.com/IntersectMBO/cardano-base/>
- Haddock documentation: <https://base.cardano.intersectmbo.org/>
- VRF batch-verification fork of libsodium: <https://github.com/input-output-hk/libsodium/tree/iquerejeta/vrf_batchverify> (behavioral reference only — this crate uses pure Rust)

## Current Phase
- Blake2b, Ed25519, and SimpleKES (two-period) are implemented with vector coverage.
- secp256k1 ECDSA and Schnorr (BIP-340) verification is implemented in `secp256k1.rs` using the pure-Rust `k256` crate.
  - ECDSA: 33-byte SEC1-compressed pubkey, 32-byte pre-hashed digest, 64-byte r‖s signature. Uses `PrehashVerifier`.
  - Schnorr: 32-byte x-only pubkey, arbitrary-length message, 64-byte BIP-340 signature.
  - 10 unit tests (length validation, invalid key rejection, sign+verify round trips).
- VRF batchcompat (ietfdraft13, 128-byte proof) verification is complete and passes all 7 upstream vectors.
- VRF standard (ietfdraft03, 80-byte proof) verification is complete and passes all 7 upstream vectors.
  - H2C uses `SHA-512(SUITE||ONE||pk||alpha)` → first 32 bytes → clear bit 255 → Elligator2 `from_representative::<Legacy>` → normalize Edwards X sign (clear bit 7 of compressed byte 31) → decompress → cofactor multiply.
  - Challenge uses `SHA-512(SUITE||TWO||H_string||gamma||U||V)` → first 16 bytes (no pk, no trailing ZERO — differs from batchcompat).
  - Sign normalization is required because `from_representative::<Legacy>` does NOT force non-negative X, unlike upstream C `ge25519_from_uniform`.
- VRF proof generation (`prove` and `prove_batchcompat`) is complete and produces byte-exact proofs for all 14 upstream vectors.
  - Secret scalar: `SHA-512(seed)` → clamp first 32 bytes (Ed25519 convention); bytes 32..64 are the nonce prefix.
  - Nonce: `SHA-512(nonce_prefix || H_string)` → `Scalar::from_bytes_mod_order_wide` (matches `sc25519_reduce`).
  - Response: `s = c * x + k mod l` using Dalek scalar arithmetic (matches `sc25519_muladd`).
  - Standard proof layout: Gamma(32) || challenge(16) || response(32) = 80 bytes.
  - Batchcompat proof layout: Gamma(32) || kB(32) || kH(32) || response(32) = 128 bytes.
- BLS12-381 elliptic curve operations implemented in `bls12_381.rs` for PlutusV3/CIP-0381:
  - Opaque wrappers: `G1Element`, `G2Element`, `MlResult` (all with `PartialEq`).
  - G1/G2: add, neg, scalar_mul, equal, compress/uncompress, hash_to_group, identity, generator.
  - Pairing: `miller_loop`, `mul_ml_result`, `final_verify`.
  - Hash-to-curve uses `sha2_09::Sha256` (renamed sha2 0.9 dep for digest 0.9 compatibility with `bls12_381` crate).
  - 12 unit tests + 5 upstream vector integration tests (ec_operations, pairing, serde, sig_aug, h2c_large_dst).
- Zeroize hardening applied across all secret-bearing types:
  - `VrfSecretKey` derives `ZeroizeOnDrop`; secret scalar, nonce prefix, and nonce temporaries are zeroized in `prove()`, `prove_batchcompat()`, and `derive_secret_scalar_and_nonce()`.
  - `ed25519::SigningKey` derives `Zeroize + ZeroizeOnDrop`.
  - `kes::KesSigningKey` and `kes::SimpleKesSigningKey` have manual `Zeroize` impl + `Drop` calling `zeroize()`.
- Tampering rejection tests cover bit-flips across all proof components for both standard and batchcompat formats.
- Next priorities: ledger type expansion, multi-era CBOR round-trip testing.
