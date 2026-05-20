# Guidance for the pure-Rust port of upstream `tx-generator`.

**Status:** `partial` (post-R533 Command parser slice). The old
cardano-cli CLI-MVS prerequisite is closed; concrete work here is now
the tx-generator Setup / Script / GeneratorTx / Submission
implementation arc plus upstream comparison evidence. Scope band:
**LARGE**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/bench/tx-generator/`
(46 `.hs` files).

## Mini-arc scope

Transaction-stream load generator for benchmarking. The active arc
starts from the vendored `Command.hs`, `Setup/*`, and
`GeneratorTx/Submission.hs` surfaces, then finishes with an end-to-end
soak against a yggdrasil node on preview. The Calibrate sub-tree
carve-out (Compiler.hs, Benchmarking/Script/*, PureExample) remains an
approved synthesis area from the sister-tools plan.

## Current Functional Surface

- Shipped: `<binary> --help` byte-equivalent to upstream (golden test
  pinned in `tests/cli_help_golden.rs`).
- Shipped: `<binary> --version` byte-equivalent to upstream.
- Shipped R533: `Command.hs` parser surface. `command.rs` mirrors the
  upstream `Command` sum type and `commandParser` grammar for `json`,
  `json_highlevel`, `compile`, `selftest`, and `version`.
- Shipped R533: `parser::Args` now carries typed `command::Command`
  instead of raw passthrough.
- Pending: concrete command execution. Dispatch returns a
  command-specific "not yet implemented" sentinel until the Setup /
  Script / GeneratorTx / Submission slices land.
- Pending: end-to-end behavioral tests against the upstream binary.

## Build + Run

```bash
# Build (release).
cargo build --release -p yggdrasil-tx-generator

# Run via the universal launcher (recommended).
scripts/run-tools.sh tx-generator --help
scripts/run-tools.sh tx-generator --version

# Or invoke the binary directly:
target/release/tx-generator --help
```

The binary is named `tx-generator` (matching upstream exactly).
Operators can swap upstream's binary for the yggdrasil one in their
automation once concrete dispatch and upstream comparison evidence land.

## Rules

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `tx-generator` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies from
  crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream ships a
  new release with different help output, refresh the fixtures + bump
  the relevant SHA pin in `crates/node/config/src/upstream_pins.rs` as
  a coordinated round.

## Round Roadmap

This crate's full implementation remains an A4 sister-tool build-out:

- Shipped: skeleton (R327 + R335-pattern bulk skeleton at R335-R336).
- Shipped: Command parser (R533): `Command.hs` `Command`,
  `TestnetConfig`, and command-parser grammar.
- Next: port upstream setup discovery, generator transaction
  construction, and submission client in strict-mirror-sized slices.
- Closeout: when all subcommands are functional, parity-matrix entry
  advances `partial -> verified_11_0_1`. Operators can then swap
  upstream binary for the yggdrasil binary without script changes.

## Comparison With Upstream

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-tx-generator

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/tx-generator --help) \
     <(target/debug/tx-generator --help)
diff <(.reference-haskell-cardano-node/install/bin/tx-generator --version) \
     <(target/debug/tx-generator --version)
# (empty diffs expected; byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete command implementations land.
- Keep the per-tool migration status in sync with
  `docs/COMPLETION_ROADMAP.md` and `docs/parity-matrix.json`.
- If upstream ships a new release: refresh the help/version fixtures,
  advance the relevant SHA pin in `upstream_pins.rs`, and re-run the
  full cargo gate.
