# Guidance for the pure-Rust port of upstream `db-truncater`.

**Status:** `partial` (post-R335-pattern skeleton). Concrete
subcommand dispatch lands at **R387+** per the R326-R459
sister-tools port arc plan. Scope band: **SMALL**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBTruncater/` (4 `.hs` files).

## Mini-arc scope

ChainDB rollback utility. Phase B.1 mini-arc R386-R390 (5 rounds, SMALL). R389 lands Run.hs port leveraging `crates/storage/src/{immutable,volatile}_db.rs` — note: Yggdrasil storage currently exposes `trim_before_slot`; a `trim_after_slot` extension is a prerequisite for the R389 implementation.

## Current functional surface (R335-pattern skeleton)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Arg passthrough captured into `parser::Args.passthrough` for
  later-round typed dispatch.
- ❌ Concrete subcommand dispatch — returns "not yet implemented"
  sentinel. Lands at `R387+`.
- ❌ End-to-end behavioral tests against upstream binary — pending
  concrete dispatch.

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-db-truncater

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh db-truncater --help
node/scripts/run-tools.sh db-truncater --version

# Or invoke the binary directly:
target/release/db-truncater --help
```

The binary is named `db-truncater` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch lands at `R387+`.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `db-truncater` is the
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
- 🟡 Next: **R387** — first concrete-impl round of the mini-arc.
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
cargo test -p yggdrasil-db-truncater

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/db-truncater --help) \
     <(target/debug/db-truncater --help)
diff <(.reference-haskell-cardano-node/install/bin/db-truncater --version) \
     <(target/debug/db-truncater --version)
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
