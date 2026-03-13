---
name: Workspace Crates
description: Guidance for maintaining crate boundaries and shared conventions across the Rust workspace crates.
---

Keep this directory as a crate index, not as a place for cross-cutting implementation logic.

## Scope
- Adding, removing, or renaming workspace crates.
- Maintaining crate boundaries, ownership, and dependency direction.
- Keeping crate-local AGENTS files aligned with the actual responsibility of each crate.

## Non-Negotiable Rules
- Each child crate MUST own a clear protocol or subsystem boundary before new code is added.
- Shared behavior MUST live in the appropriate crate, not in this directory.
- Cross-crate dependency direction MUST stay aligned with `docs/ARCHITECTURE.md`.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Current Layout
- `crypto`: cryptographic primitives and encodings.
- `cddl-codegen`: pinned CDDL parsing and code generation.
- `ledger`: era modeling and state transitions.
- `storage`: durable storage and snapshots.
- `consensus`: chain selection, rollback, and epoch math.
- `mempool`: transaction intake, ordering, and eviction.
- `network`: mux, mini-protocols, codecs, and peer management.

## Maintenance Guidance
- When a crate boundary changes, update the child crate AGENTS file, `docs/ARCHITECTURE.md`, and the workspace root `AGENTS.md` together.
- Do not add umbrella instructions here that conflict with more specific crate-local guidance.