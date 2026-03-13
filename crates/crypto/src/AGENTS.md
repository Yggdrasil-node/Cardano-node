---
name: crypto-src
description: Guidance for cryptographic implementation modules in the crypto crate.
---

This directory is for pure Rust cryptographic implementation code and protocol-facing encodings.

## Scope
- Hashing, Ed25519, KES, VRF, and key or proof encodings.
- Secret-bearing types and low-level cryptographic helpers.

## Non-Negotiable Rules
- Secret material handled here MUST be zeroized or compared in constant time where required.
- Encodings and proof layouts MUST remain byte-accurate relative to the upstream format being implemented.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Current Focus
- Preserve upstream-compatible VRF and KES behavior while avoiding any hidden FFI dependency.