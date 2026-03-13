---
name: workspace-docs
description: Guidance for maintaining project policy and architecture documents under docs/.
---

Keep these documents synchronized with the implemented workspace, not with speculative future goals.

## Scope
- `ARCHITECTURE.md`, `DEPENDENCIES.md`, `SPECS.md`, and `CONTRIBUTING.md`.
- Project-wide workflow, dependency policy, specification provenance, and architecture updates.
- Documentation changes that reflect implemented behavior or accepted policy.

## Non-Negotiable Rules
- Documentation in this directory MUST describe current behavior or explicitly labeled near-term policy, not aspirational features.
- Dependency decisions MUST be recorded in `DEPENDENCIES.md` before a new crate is treated as accepted.
- Architecture and workflow changes MUST stay consistent with the actual crate graph and verification commands used in the workspace.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Node integration and operational behavior: <https://github.com/IntersectMBO/cardano-node/>
- Ledger implementation and schemas: <https://github.com/IntersectMBO/cardano-ledger/>
- Formal ledger specifications: <https://github.com/IntersectMBO/formal-ledger-specifications>
- Consensus implementation and reports: <https://github.com/IntersectMBO/ouroboros-consensus/>
- Networking implementation and specification: <https://github.com/IntersectMBO/ouroboros-network/>

## Maintenance Guidance
- Update these docs in the same change when a subsystem milestone materially changes.
- Keep references to upstream IntersectMBO and Cardano sources current and traceable.
- Prefer concise policy and architecture guidance over long narrative explanation.