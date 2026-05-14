# yggdrasil-node-config — node configuration + path resolution + upstream pins

## Scope

Leaf-of-the-build-graph crate extracted from `yggdrasil-node` in
Wave 5 PR 7 so the configuration surface is consumable by sister
tools and downstream embedders without linking the whole runtime.

Three modules:

- `lib.rs` (the former `node/src/config.rs`): `NodeConfigFile`,
  `NetworkPreset`, `TraceNamespaceConfig`, the JSON-first deserializer
  with PascalCase upstream-key aliases, peer-snapshot loaders, etc.
- `path_resolve`: pure-`std::path` helpers for resolving operator
  config / topology / database / socket paths. No I/O.
- `upstream_pins`: `UPSTREAM_*_COMMIT` constants pinned at the
  policy tag (`docs/parity-matrix.json::reference.tag`, currently
  `11.0.1`). Verified by `scripts/check-fixture-manifest.py`.

## Rules — Non-Negotiable

- **Tier 1 stability.** Every public type / const in this crate is
  part of the operator-facing stability contract declared at
  `docs/COMPATIBILITY.md`. Breaking changes require semver-major.
- **Leaf dependency.** This crate depends only on
  `yggdrasil-{ledger,network,plutus}` — it MUST NOT pull in
  `yggdrasil-consensus`, `yggdrasil-storage`, or any other node
  sub-crate. Adding a sibling-node-crate dep here would re-introduce
  the monolithic coupling that Wave 5 broke.
- **Strict mirror.** The synthesis docstring at the top of `lib.rs`
  declares this crate as a Yggdrasil-side unification (upstream's
  config surface is split across multiple Haskell modules); the
  `## Naming parity` block is the parity-matrix evidence and MUST
  stay accurate after each Wave-N expansion.

## Naming parity

Synthesis crate. The lib.rs docstring carries the `## Naming parity`
stanza covering all three modules.

## R-arc tracking

Wave 5 PR 7 (extracted from yggdrasil-node binary). Future bumps
(e.g. new fields on `NodeConfigFile` to support the Wave 6 tracer
config) land as R-arc rounds against this crate.
