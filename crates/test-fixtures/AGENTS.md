# yggdrasil-test-fixtures — shared test scaffolding (Wave 2)

## Scope

Dev-dependencies-only crate. Consumed by other workspace crates'
`[dev-dependencies]` blocks; never by `[dependencies]`.

Holds:

- `TmpDir` — RAII temp-dir wrapper (std-only, no `tempfile` dep).
- `tmp_chaindb()` — produces a `TmpDir` with `immutable/`,
  `volatile/`, `ledger/` subdirs preset for `ChainDb` fixtures.
- `make_test_peer(index: u8) -> SocketAddr` — deterministic bogus
  peer addresses for routing / topology test fixtures.
- `assert_bytes_eq(label, actual, expected)` — uniform byte-equivalence
  assertion with hex-dump diagnostic on failure.

## Rules — Non-Negotiable

- **`publish = false`.** Inherited from `[workspace.package]`; do
  not override.
- **No production-code consumers.** The strict-mirror gate excludes
  helpers via the dev-deps-only contract. If something here turns
  out to be needed in production, it gets re-homed to the consuming
  crate's `src/` and the dev-deps copy is deleted.
- **Std-only deps.** Keeping it std-only means adding this as a
  dev-dep in every Wave 5 sub-crate doesn't pull in fresh
  transitive crates. Wave 6 may relax this for `tracing`-test
  utilities when those land.

## Naming parity

Synthesis crate (no upstream mirror). Upstream's equivalents
scatter across `Test.ThreadNet.*`, `Test.Util.*`, and per-package
`test/` trees. Declared via `## Naming parity` block in `src/lib.rs`;
allowlisted in `docs/strict-mirror-audit.tsv` — Wave 2 PR 4 scaffold.

## R-arc tracking

Wave 2 PR 4 (scaffold) → Wave 5 sub-crate extractions consume this
via `[dev-dependencies] yggdrasil-test-fixtures.workspace = true`.
