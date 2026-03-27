# Guidance for consensus crate tests and parity-oriented fixtures.
Use this directory for deterministic tests of consensus behavior and boundary conditions.

## Scope
- Roll-forward and rollback tests.
- Nonce evolution, header verification, and leadership threshold coverage.

##  Rules *Non-Negotiable*
- Tests here MUST prefer protocol edge cases and rollback invariants over broad smoke coverage.
- Reproducible fixtures MUST back any claim of parity-sensitive consensus behavior.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Consensus test suites](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/test/)
- [Cardano consensus tests (era-specific)](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-cardano/test/)
- [Protocol Praos tests](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-protocol/test)
- [Consensus tech report](https://ouroboros-consensus.cardano.intersectmbo.org/pdfs/report.pdf)

## Current Phase
- Tests in this directory protect rollback depth, nonce evolution, header verification, and contiguous chain-state behavior.