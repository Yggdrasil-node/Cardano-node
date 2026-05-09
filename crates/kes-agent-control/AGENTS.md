# Guidance for the pure-Rust port of upstream `kes-agent-control`.

**Status:** `partial` (post-R335-pattern skeleton). Concrete
subcommand dispatch lands at **R356+** per the R326-R459
sister-tools port arc plan. Scope band: **SMALL**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/kes-agent/kes-agent/` (0 `.hs` files).

## Mini-arc scope

Companion CLI for kes-agent. Phase A.4 mini-arc R355-R359 (5 rounds, SMALL). R357 implements gen-staged-key, install-key, export-staged-vkey subcommands; R358 round-trip test against R344-R354 yggdrasil-kes-agent + upstream.

## Current functional surface (R335-pattern skeleton)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Arg passthrough captured into `parser::Args.passthrough` for
  later-round typed dispatch.
- ❌ Concrete subcommand dispatch — returns "not yet implemented"
  sentinel. Lands at `R356+`.
- ❌ End-to-end behavioral tests against upstream binary — pending
  concrete dispatch.

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-kes-agent-control

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh kes-agent-control --help
node/scripts/run-tools.sh kes-agent-control --version

# Or invoke the binary directly:
target/release/kes-agent-control --help
```

The binary is named `kes-agent-control` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch lands at `R356+`.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `kes-agent-control` is the
  acceptance gate for any concrete implementation.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `node/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Per the R326-R459 plan, this crate's full implementation lands across
the named mini-arc rounds:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- 🟡 Next: **R356** — first concrete-impl round of the mini-arc.
- 🟡 Closeout — when all subcommands are functional, parity-matrix
  entry advances `partial → verified_11_0_1`. Operators can then
  swap upstream binary for the yggdrasil binary without script
  changes.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-kes-agent-control

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/kes-agent-control --help) \
     <(target/debug/kes-agent-control --help)
diff <(.reference-haskell-cardano-node/install/bin/kes-agent-control --version) \
     <(target/debug/kes-agent-control --version)
# (empty diffs expected — byte-equivalent)
```

## Maintenance Guidance

- Update this AGENTS.md when concrete subcommand implementations
  land (replace `❌ not yet implemented` rows with `✅ shipped` +
  round number).
- Keep the per-tool migration round numbers in sync with the
  authoritative plan file at `/home/daniel/.claude/plans/playful-tickling-plum.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
