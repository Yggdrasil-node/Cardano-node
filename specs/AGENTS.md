# Guidance for pinned specification inputs and vendored upstream artifacts under specs/.
This directory holds reproducible specification inputs and pinned upstream artifacts used by the workspace.

## Scope
- Checked-in specification inputs such as `mini-ledger.cddl`.
- Vendored upstream corpora and fixtures under child spec directories.
- Provenance tracking for pinned revisions used by generators or parity tests.

##  Rules *Non-Negotiable*
- Specification inputs in this directory MUST remain traceable to an upstream source or an explicitly documented local derivation.
- Vendored upstream artifacts MUST NOT be hand-edited.
- Pinned revisions used for generation or validation MUST be recorded alongside the affected implementation or tests.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- Per-era CDDL schemas: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Ledger binary support libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary/>
- Formal ledger specifications (Agda): <https://github.com/IntersectMBO/formal-ledger-specifications>
- Published formal spec site: <https://intersectmbo.github.io/formal-ledger-specifications/site>
- Node integration reference: <https://github.com/IntersectMBO/cardano-node/>
- Upstream crypto vectors: <https://github.com/IntersectMBO/cardano-base/>
- Plutus core specification: <https://github.com/IntersectMBO/plutus/>

## Current Contents
- `mini-ledger.cddl`: reduced pinned CDDL fixture used by `crates/cddl-codegen` tests and examples.
- `upstream-test-vectors/`: pinned official upstream vector corpora with separate folder-specific guidance.

## Maintenance Guidance
- When a pinned spec input changes, update its provenance in `docs/SPECS.md` and any affected crate guidance.
- Keep local reduced fixtures minimal and deterministic so regeneration and parser tests remain reproducible.