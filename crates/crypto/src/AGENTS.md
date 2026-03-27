# Guidance for cryptographic implementation modules in the crypto crate.
This directory is for pure Rust cryptographic implementation code and protocol-facing encodings.

## Scope
- Hashing, Ed25519, KES, VRF, and key or proof encodings.
- Secret-bearing types and low-level cryptographic helpers.

##  Rules *Non-Negotiable*
- Secret material handled here MUST be zeroized or compared in constant time where required.
- Encodings and proof layouts MUST remain byte-accurate relative to the upstream format being implemented.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- [`cardano-crypto-class` source tree](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/src/Cardano/Crypto) (hash, Ed25519, VRF, KES abstractions)
- [`cardano-crypto-praos` source tree](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos/src/Cardano/Crypto) (Praos VRF and KES implementations)
- [Peras-era crypto extensions](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-peras)
- [BLS12-381 class bindings](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/src/Cardano/Crypto/EllipticCurve)
- [`cardano-base` root](https://github.com/IntersectMBO/cardano-base/tree/master/) (shared crypto packages)
- [Haddock documentation](https://cardano-base.cardano.intersectmbo.org/haddocks/)

## Current Phase
- Preserve upstream-compatible VRF and KES behavior while avoiding any hidden FFI dependency.
- `blake2b.rs`: `hash_bytes` (512-bit), `hash_bytes_256` (256-bit), `hash_bytes_224` (224-bit, used for credential/script hashes).
- `secp256k1.rs`: Pure-Rust ECDSA and Schnorr (BIP-340) signature verification via `k256`. ECDSA uses `PrehashVerifier` (33-byte SEC1 pubkey, 32-byte digest, 64-byte sig). Schnorr uses 32-byte x-only pubkey, arbitrary-length message, 64-byte sig. Used by PlutusV2 builtins.
- `bls12_381.rs`: BLS12-381 curve operations for PlutusV3/CIP-0381. Opaque wrappers `G1Element`, `G2Element`, `MlResult`. All 17 Plutus builtins: G1/G2 add/neg/scalar_mul/equal/compress/uncompress/hash_to_group, miller_loop, mul_ml_result, final_verify. Hash-to-curve uses renamed `sha2_09` dep for digest 0.9 compat. 12 unit tests + 5 upstream vector integration tests.
- Zeroize: `ed25519::SigningKey` derives `Zeroize + ZeroizeOnDrop`. `kes::KesSigningKey` and `kes::SimpleKesSigningKey` have manual `Zeroize + Drop`.