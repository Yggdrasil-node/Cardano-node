# Guidance for maintaining crate boundaries and shared conventions across the Rust workspace crates.
Keep this directory as a crate index, not as a place for cross-cutting implementation logic.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate: `python3 scripts/check-strict-mirror.py`
(warn-only since R275; fail-build at R288). Allowlist source-of-truth:
[`docs/strict-mirror-audit.tsv`](../docs/strict-mirror-audit.tsv).

## Scope
- Adding, removing, or renaming workspace crates.
- Maintaining crate boundaries, ownership, and dependency direction.
- Keeping crate-local AGENTS files aligned with the actual responsibility of each crate.

##  Rules *Non-Negotiable*
- Each child crate MUST own a clear protocol or subsystem boundary before new code is added.
- Shared behavior MUST live in the appropriate crate, not in this directory.
- Cross-crate dependency direction MUST stay aligned with `docs/ARCHITECTURE.md`.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Workspace architecture anchor](.reference-haskell-cardano-node/cardano-node/)
- [Ledger era package layout](.reference-haskell-cardano-node/deps/cardano-ledger/eras/)
- [Ledger support libraries](.reference-haskell-cardano-node/deps/cardano-ledger/libs/)
- [Consensus package layout](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/)
- [Consensus protocol modules](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Protocol/)
- [Networking package layout](.reference-haskell-cardano-node/deps/ouroboros-network/)
- [Network mini-protocol packages](.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-protocols/)
- [Crypto package layout](.reference-haskell-cardano-node/deps/cardano-base/)
- [Plutus core and CEK machine](.reference-haskell-cardano-node/deps/plutus/)

## Current Layout
- `crypto`: cryptographic primitives and encodings.
- `ledger`: era modeling and state transitions. Per-era `CborEncode`/`CborDecode` impls under `crates/ledger/src/eras/*/cbor.rs` are hand-coded against upstream CDDL (`.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/cddl/data/`); CDDL is treated as authoritative documentation. Codegen scaffolding was removed in favor of hand-coding because real upstream parity needs Byron / array-vs-map / optional-field semantics that CDDL underspecifies.
- `storage`: durable storage and snapshots.
- `consensus`: chain selection, rollback, and epoch math.
- `mempool`: transaction intake, ordering, and eviction.
- `network`: mux, mini-protocols, codecs, and peer management.

## Maintenance Guidance
- When a crate boundary changes, update the child crate AGENTS file, `docs/ARCHITECTURE.md`, and the workspace root `AGENTS.md` together.
- Do not add umbrella instructions here that conflict with more specific crate-local guidance.