# Round 289 — Phase F bootstrap (cardano-cli crate skeleton)

**Date:** 2026-05-09
**Phase:** F (cardano-cli surface expansion)
**Predecessor:** R288 (`docs/operational-runs/2026-05-09-round-288-drift-guard-fail-build.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Bootstrap the new workspace member `crates/cardano-cli/`
(`yggdrasil-cardano-cli`). Lands the crate skeleton + 8 strict-mirror
files (one per top-level upstream `Cardano.CLI.*` namespace module).
The Byron / Compatible / Shelley / Alonzo / Babbage / Conway clusters
land in R290–R295.

The crate exists as a separate workspace member rather than growing
inside `node/src/` because the cardano-cli surface is large
(~150 upstream files), has its own dependency graph (cardano-api
types, transaction-construction helpers, key derivation,
serialise-text-envelope codec), and shipping it independently keeps
`node/` an integration layer per `CLAUDE.md`'s topology rule.

## Files added

| File | Strict-mirror upstream `.hs` |
|---|---|
| `crates/cardano-cli/Cargo.toml` | `cardano-cli/cardano-cli/cardano-cli.cabal` (library component) |
| `crates/cardano-cli/src/lib.rs` | Rust convention; documents the sub-module tree |
| `crates/cardano-cli/src/command.rs` | `Cardano/CLI/Command.hs` (top-level dispatch enum) |
| `crates/cardano-cli/src/run.rs` | `Cardano/CLI/Run.hs` (dispatcher) |
| `crates/cardano-cli/src/parser.rs` | `Cardano/CLI/Parser.hs` (clap-based parser shell) |
| `crates/cardano-cli/src/render.rs` | `Cardano/CLI/Render.hs` (output formatters) |
| `crates/cardano-cli/src/option.rs` | `Cardano/CLI/Option.hs` (shared parsers) |
| `crates/cardano-cli/src/helper.rs` | `Cardano/CLI/Helper.hs` (version_info + utilities) |
| `crates/cardano-cli/src/environment.rs` | `Cardano/CLI/Environment.hs` (env/path resolution) |
| `crates/cardano-cli/src/orphan.rs` | `Cardano/CLI/Orphan.hs` (Rust coherence rules eliminate the upstream need; empty file kept for strict-mirror parity) |

Total: 1 manifest + 9 source files. All 8 source files auto-graded as
`(a) DIRECT_MIRROR` by the audit script because each carries an
explicit `**Strict mirror:** <upstream-path>` declaration in its
`## Naming parity` block.

## R289 bootstrap behavior

The crate compiles + ships the API skeleton. Implementation is
deferred:

- `command::Command` carries 3 variants (Version, ShowUpstreamConfig,
  QueryTip) matching what `node/src/cli.rs::CardanoCliCommand`
  exposes today. R290–R295 grow the variant set.
- `run::run_command` dispatches on `Command`; the `Version` arm has
  a working stub, `ShowUpstreamConfig` and `QueryTip` arms `bail!`
  with a deferral message pointing at the existing
  `node/src/commands/cardano_cli.rs` implementation.
- `parser::parse_command` is a stub that returns `ParseError::NotYetMigrated`;
  callers should continue using the node binary's existing clap
  parser (`node/src/cli.rs::CardanoCliCommand`) until the per-cluster
  parsers land.
- `render::render_*` ships working JSON + plain-text rendering helpers.
- `option::parse_socket_path` and `parse_network_magic` ship working
  parsers used by the upcoming runners.
- `helper::version_info` ships the version-string helper.
- `environment::resolve_upstream_config_root` and
  `resolve_socket_path` ship working env-var fallback chains matching
  upstream's `Cardano.CLI.Environment` resolution order.

`node/src/commands/cardano_cli.rs` (163 lines) is **NOT removed in
R289**. Migration of its three command implementations into
`yggdrasil-cardano-cli` happens in R290 (Byron) and R291 (Compatible).
The existing node binary's `yggdrasil-node cardano-cli` subcommand
continues to work unchanged through R289.

## Workspace integration

```toml
# Cargo.toml
[workspace]
members = [
    "crates/cardano-cli",   # +
    "crates/crypto",
    ...
]
```

The crate is published-false (matches workspace policy) and uses
shared workspace deps (`clap`, `eyre`, `serde`, `serde_json`,
`thiserror`).

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (yggdrasil-cardano-cli compiles
                                            in 0.52s; full workspace 12.01s)
cargo lint                          clean
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Strict-mirror audit table

```text
$ python3 scripts/audit-strict-mirror.py
  audit complete: 217 rust files; candidate_match=168, no_candidate_match=49
  auto-grading bucket counts:
    (a): 60   (+8 cardano-cli skeleton files)
    (c): 157
```

All 8 new cardano-cli source files graded `(a) DIRECT_MIRROR (auto:
docstring declares strict mirror)`. Total file count: 209 → 217.

## Diff stat

```text
Cargo.toml                          +1 line  (add crates/cardano-cli member)
crates/cardano-cli/Cargo.toml       (new, 24 lines)
crates/cardano-cli/src/lib.rs       (new, 47 lines)
crates/cardano-cli/src/command.rs   (new, 49 lines)
crates/cardano-cli/src/run.rs       (new, 54 lines)
crates/cardano-cli/src/parser.rs    (new, 54 lines)
crates/cardano-cli/src/render.rs    (new, 38 lines)
crates/cardano-cli/src/option.rs    (new, 38 lines)
crates/cardano-cli/src/helper.rs    (new, 23 lines)
crates/cardano-cli/src/environment.rs (new, 46 lines)
crates/cardano-cli/src/orphan.rs    (new, 16 lines)
docs/strict-mirror-audit.tsv        rebuilt (+8 rows for cardano-cli)
docs/operational-runs/2026-05-09-round-289-... (new)
```

## Stop point — Phase F R289 closed

| Round | Cluster | Status |
|---|---|---|
| **R289** | **Phase F bootstrap (cardano-cli skeleton)** | ✅ **closed** |
| R290 | Byron cluster (~20 files) | next |
| R291 | Compatible cluster (~12 files) | pending |
| R292 | Shelley + governance (~30 files) | pending |
| R293 | Alonzo + Babbage (~25 files) | pending |
| R294 | Conway (~20 files) | pending |
| R295 | sweeper + integration tests | pending |

R290 starts populating the Byron sub-tree at
`crates/cardano-cli/src/byron/{command, delegation, genesis, key,
tx, vote, ...}.rs` mirroring upstream
`cardano-cli/cardano-cli/src/Cardano/CLI/Byron/*.hs`.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R288 (`docs/operational-runs/2026-05-09-round-288-drift-guard-fail-build.md`)
- Upstream cardano-cli library tree:
  `.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI/`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
