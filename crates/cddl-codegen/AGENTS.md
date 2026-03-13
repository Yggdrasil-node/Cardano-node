---
name: cddl-codegen-crate-agent
description: Guidance for generating Rust types from pinned Cardano CDDL sources
---

Focus on deterministic parsing and reproducible generated artifacts.

## Scope
- Parsing pinned CDDL inputs and generating Rust-facing type output.
- Separating supported subsets from future parsing ambitions.

## Non-Negotiable Rules
- Upstream schemas MUST be treated as pinned inputs and the exact revision used MUST be recorded.
- Handwritten parser logic and generated output templates MUST remain separated.
- Small supported CDDL subsets with tests MUST be preferred over broad speculative parsing.
- Generated output MUST NOT be edited by hand.
- Public parser and generator entry points MUST have Rustdocs when supported syntax, failure modes, or output guarantees are not obvious.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Generated type and field naming MUST remain traceable to upstream ledger schemas and official node terminology.
- Schema handling and generated output MUST be explained with reference to the official node path through cardano-ledger and related IntersectMBO sources.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research and add or update links as needed*
- Era CDDL roots: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/>
- Ledger binary and supporting libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary/>
- Ledger support libraries: <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/>
- Formal ledger specification: <https://github.com/IntersectMBO/formal-ledger-specifications/>

## Current Phase
- Parser supports: comments, aliases, flat maps, flat arrays, size constraints (`.size N`), integer-keyed map fields, optional fields (`?`), variable-length arrays (`[* type]`), nil alternatives (`type / nil`), named array fields, multi-line definitions, CBOR tag annotations (`#6.N(type)`), and group choices (`//`).
- Generator maps: `uint .size N` → `u8/u16/u32/u64`, `bytes .size N` → `[u8; N]`, `[* type]` → `Vec<T>`, optional → `Option<T>`, integer keys → `field_N`, named array fields → named struct fields, tagged types → inner type (tag is serialization-only), group choices → `enum` with named or indexed variants.
- Pinned fixture: `specs/mini-ledger.cddl` derived from upstream Shelley CDDL at IntersectMBO/cardano-ledger revision `ed5017c8`. Includes tagged sets (`#6.258`) and group-choice certificates.
- Not yet supported: inline tuples/groups, range constraints (`N..M`, `.le`), generic type parameters.
