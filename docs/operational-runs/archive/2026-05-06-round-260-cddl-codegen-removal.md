## Round 260 — Removed `tools/cddl-codegen` (hand-coded CBOR is authoritative)

Date: 2026-05-06
Branch: main
Type: Workspace cleanup (build-time tooling removal)

### Goal

Remove the dormant `tools/cddl-codegen` build-time tool. It generated
Rust types + `CborEncode`/`CborDecode` impls from CDDL, but no
production code in the workspace consumed its output. The actual
per-era CBOR codecs in `crates/ledger/src/eras/*/cbor.rs` are
hand-coded against upstream CDDL and have been since the ledger
ports landed — codegen was scaffolding for a path that never
materialized.

### Why hand-coding decisively replaced codegen

Real upstream parity needs Byron / array-vs-map / optional-field
semantics that CDDL underspecifies:

- Byron-era envelopes (`mainnet-byron-genesis.json` shape, AVVM
  distributions, the Byron ProtocolMagic codec) have multiple
  legacy edge cases that CDDL doesn't capture.
- Several Shelley-family fields use definite-length arrays where
  upstream Haskell emits indefinite-length, or vice versa, in ways
  that CDDL leaves implementation-defined.
- Optional fields (`?` in CDDL) in Conway use both null-marker and
  field-omission encodings depending on the surrounding map shape.
- Generated codecs would produce either compile-correct Rust that
  emits the wrong bytes, or generated stubs with TODO holes that
  hand-rolling fills in anyway.

The 2026-Q2 audit shipped byte-for-byte parity by hand-rolling each
era's CBOR codec against upstream CDDL plus careful comparison
against `Cardano.Ledger.<Era>.<Module>` Haskell sources. The
codegen tool sat as an unused asset since.

### Removal scope

- `tools/cddl-codegen/` (the entire tool — ~3 K LOC of parser +
  generator + integration tests)
- `tools/` (now empty, removed)
- `crates/ledger/tests/generated_intake.rs` (the only consumer)
- `specs/mini-ledger.cddl` (codegen-only fixture)
- `specs/upstream-cddl-fragments/` (codegen-only fixture tree)
- `tools/cddl-codegen` workspace member in root `Cargo.toml`
- `yggdrasil-cddl-codegen` dev-dep in `crates/ledger/Cargo.toml`
- All AGENTS.md / CLAUDE.md / docs references to the tool

### Documentation reframing

Per-era CBOR codec authority is now uniformly stated as:

- **Source of truth**: upstream CDDL at
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/cddl/data/<era>.cddl`
  — read as documentation, not regenerated.
- **Implementation site**: `crates/ledger/src/eras/<era>/cbor.rs` —
  hand-coded `CborEncode`/`CborDecode` impls.
- **Cross-check**: Haskell `Cardano.Ledger.<Era>.<Module>`
  implementations under
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/src/`.
- **Regression coverage**: golden round-trip tests against vendored
  upstream test vectors under `specs/upstream-test-vectors/`.

Updated in this round: CLAUDE.md, root AGENTS.md, crates/AGENTS.md,
specs/AGENTS.md, README.md, SECURITY.md, docs/ARCHITECTURE.md,
docs/CONTRIBUTING.md, docs/DEPENDENCIES.md, docs/PARITY_PLAN.md,
docs/REFACTOR_BLUEPRINT.md, node/src/upstream_pins.rs.

Historical entries inside the rolling implementation journal (root
AGENTS.md slices 110, 123, 127, etc.) and dated audit docs
(docs/AUDIT_VERIFICATION_2026Q2.md, docs/code-audit.md) intentionally
preserved as evidence per the journal-preservation rule — they
describe past slices that mentioned the tool.

### Test impact

Workspace test count: **4 904 → 4 844 (-60)**. The 60 lost tests
were all codegen-internal:

- `tools/cddl-codegen/tests/integration.rs` (26 tests) — parser-accept,
  parser-reject, generator-output, codec-generation across the
  supported CDDL subset.
- `tools/cddl-codegen/src/{parser,generator}.rs` unit tests
  (~33 tests) — internal AST and emit-side correctness.
- `crates/ledger/tests/generated_intake.rs` (1 test) — the consumer
  proof-of-life.

**All production-side tests pass unchanged.** The hand-coded codecs
have always been the byte-for-byte authority; nothing in the
runtime stack lost coverage.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 844 passed, 0 failed
```

### Operator-facing notes

- **Build time** drops slightly because the workspace member is gone.
- **Crate package list** simplifies: `yggdrasil-crypto`,
  `yggdrasil-ledger`, `yggdrasil-storage`, `yggdrasil-consensus`,
  `yggdrasil-network`, `yggdrasil-plutus`, `yggdrasil-node`. No more
  `yggdrasil-cddl-codegen`.
- **Workspace topology** loses the `tools/` directory entirely. Any
  future build-time tooling can re-introduce it as needed.
- **No runtime change.** Operators see no behavior difference; only
  the development workspace shape simplified.

### References

- Original Phase B move: `2026-04-{date}-round-256-...` (Phase B —
  `crates/cddl-codegen/` → `tools/cddl-codegen/` for build-vs-runtime
  segregation; preserved as historical step). This round (R260)
  retires the tool entirely.
- Authoritative upstream CDDL:
  `.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/cddl/data/<era>.cddl`
  for `era` ∈ {byron, shelley, allegra, mary, alonzo, babbage, conway}.
- Hand-coded codecs: `crates/ledger/src/eras/<era>/cbor.rs`.
