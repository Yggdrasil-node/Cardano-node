# Guidance for vendored upstream test vectors and pinned specification artifacts under specs/.
This directory holds vendored upstream test corpora that drive parity tests in the workspace.

## Scope
- `upstream-test-vectors/`: pinned official upstream vector corpora (cardano-base BLS12-381 + Praos VRF/KES vectors). See child `AGENTS.md` for per-tree provenance.
- Provenance tracking for pinned revisions used by parity tests.

##  Rules *Non-Negotiable*
- Vendored upstream artifacts MUST NOT be hand-edited.
- Pinned revisions MUST be recorded alongside the affected implementation or tests (commit SHAs in the vendored child directory names).
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References
- [Per-era CDDL schemas](.reference-haskell-cardano-node/deps/cardano-ledger/eras/) — authoritative documentation for the hand-coded `crates/ledger/src/eras/*/cbor.rs` impls.
- [Ledger binary support libraries](.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-binary/)
- [Formal ledger specifications (Agda)](https://github.com/IntersectMBO/formal-ledger-specifications)
- [Published formal spec site](https://intersectmbo.github.io/formal-ledger-specifications/site)
- [Node integration reference](.reference-haskell-cardano-node/cardano-node/)
- [Upstream crypto vectors](.reference-haskell-cardano-node/deps/cardano-base/)
- [Plutus core specification](.reference-haskell-cardano-node/deps/plutus/)

## Current Contents
- `upstream-test-vectors/`: pinned official upstream vector corpora with separate folder-specific guidance.

## Maintenance Guidance
- When a pinned vector tree changes, update its provenance in `docs/SPECS.md` and any affected crate guidance.