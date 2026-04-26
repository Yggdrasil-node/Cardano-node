# Guidance for generating Rust types from pinned Cardano CDDL sources
Focus on deterministic parsing and reproducible generated artifacts.

## Scope
- Parsing pinned CDDL inputs and generating Rust-facing type output.
- Separating supported subsets from future parsing ambitions.

##  Rules *Non-Negotiable*
- Upstream schemas MUST be treated as pinned inputs and the exact revision used MUST be recorded.
- Handwritten parser logic and generated output templates MUST remain separated.
- Small supported CDDL subsets with tests MUST be preferred over broad speculative parsing.
- Generated output MUST NOT be edited by hand.
- Public parser and generator entry points MUST have Rustdocs when supported syntax, failure modes, or output guarantees are not obvious.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Generated type and field naming MUST remain traceable to upstream ledger schemas and official node terminology.
- Schema handling and generated output MUST be explained with reference to the official node path through cardano-ledger and related IntersectMBO sources.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.

## Official Upstream References *Always research references and add or update links as needed*
- [Per-era CDDL schemas (each era subdirectory has `impl/cddl/data/` with per-era `.cddl` files)](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/)
- [Byron CDDL](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/byron/ledger/impl/cddl-spec/)
- [Shelley CDDL](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/shelley/impl/cddl/data/)
- [Alonzo CDDL](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/alonzo/impl/cddl/data/)
- [Babbage CDDL](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/babbage/impl/cddl/data/)
- [Conway CDDL](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/conway/impl/cddl/data/)
- [Binary serialization library (`cardano-ledger-binary`)](https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary/)
- [Ledger support libraries](https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/)
- [Formal ledger specification (type definitions)](https://github.com/IntersectMBO/formal-ledger-specifications/)

## Current Phase
- Parser supports: comments, aliases, flat maps, flat arrays, size constraints (`.size N`), size ranges (`.size N..M`, `.size N..`, `.size ..M`), inequality constraints (`.le`, `.ge`, `.lt`, `.gt`), integer-keyed map fields, optional fields (`?`), variable-length arrays (`[* type]`), nil alternatives (`type / nil`), named array fields, multi-line definitions, CBOR tag annotations (`#6.N(type)`), and group choices (`//`).
- Generator maps: `uint .size N` → `u8/u16/u32/u64`, `bytes .size N` → `[u8; N]`, `bytes .size N..M` → `Vec<u8>` + runtime length check, `text .size N..M` → `String` + runtime length check, `uint .le|.ge|.lt|.gt N` → `u64` + runtime value check, `[* type]` → `Vec<T>`, optional → `Option<T>`, integer keys → `field_N`, named array fields → named struct fields, tagged types → inner type (tag is serialization-only), group choices → `enum` with named or indexed variants.
- `generate_module_with_codecs()` generates struct/enum definitions **plus** `CborEncode`/`CborDecode` implementations for every concrete type:
  - **Array structs**: CBOR array encode/decode with positional fields.
  - **Map structs (integer-keyed)**: CBOR map encode/decode with integer key dispatch, optional field handling (conditional map length, `Option<T>` for absent keys), forward-compatible unknown key skipping.
  - **Map structs (string-keyed)**: CBOR map encode/decode with text key dispatch.
  - **GroupChoice enums**: CBOR array encode/decode with variant discrimination by field count (and first-element tag for ambiguous lengths).
  - **Aliases**: No codec impl generated (aliased type carries its own).
  - Type expression codec mapping: `uint`→`unsigned`, `int`→`integer`, `bool`→`bool`, `bytes`→`bytes`, `text`→`text`, `[* T]`→array loop, `#6.N(T)`→tag+inner, `bytes .size N`→`try_into` with error handling, `uint .size N`→cast, `<T> .size N..M` / `<T> .le|.ge|.lt|.gt N`→post-decode bound check returning `LedgerError::CborInvalidLength`. Trivial lower-bound checks (`.size 0..M`) are elided so generated code stays clippy-clean.
- Pinned fixtures:
  - `specs/mini-ledger.cddl` — Shelley subset at IntersectMBO/cardano-ledger revision `ed5017c8`. Tagged sets (`#6.258`) and group-choice certificates.
  - `specs/upstream-cddl-fragments/conway-ranges-min.cddl` — Conway range/inequality constraint constructs (`.size N..M`, `.le`/`.ge`/`.lt`/`.gt`) at IntersectMBO/cardano-ledger revision `9ae77d611ad86ae58add04b6042ab730272f2327`. Source path `eras/conway/impl/cddl/data/conway.cddl`. Drift-tracked via `node/src/upstream_pins.rs`.
- 42+ integration tests cover parsing, generation, and codec generation for all supported patterns including range/inequality constraints (Slice B). The fixed `.size N` fast path remains bit-identical (`Sized(_, u64)` AST variant) so existing snapshots do not regress.
- Not yet supported: inline tuples/groups, generic type parameters, standalone CDDL value-ranges (`type = N..M`), signed-integer ranges. These require RFC 8610 syntax the workspace does not yet need; re-evaluate when an upstream consumer lands.
