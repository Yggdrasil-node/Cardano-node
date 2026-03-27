# Guidance for maintaining crate boundaries and shared conventions across the Rust workspace crates.
Keep this directory as a crate index, not as a place for cross-cutting implementation logic.

## Scope
- Adding, removing, or renaming workspace crates.
- Maintaining crate boundaries, ownership, and dependency direction.
- Keeping crate-local AGENTS files aligned with the actual responsibility of each crate.

##  Rules *Non-Negotiable*
- Each child crate MUST own a clear protocol or subsystem boundary before new code is added.
- Shared behavior MUST live in the appropriate crate, not in this directory.
- Cross-crate dependency direction MUST stay aligned with `docs/ARCHITECTURE.md`.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- [Workspace architecture anchor](https://github.com/IntersectMBO/cardano-node/)
- [Ledger era package layout](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/)
- [Ledger support libraries](https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/)
- [Consensus package layout](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/)
- [Consensus protocol modules](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Protocol/)
- [Networking package layout](https://github.com/IntersectMBO/ouroboros-network/)
- [Network mini-protocol packages](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/)
- [Crypto package layout](https://github.com/IntersectMBO/cardano-base/)
- [Plutus core and CEK machine](https://github.com/IntersectMBO/plutus/)

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