# Guidance for the pure-Rust port of upstream `db-synthesizer`.

**Status:** `partial` (post-R335-pattern skeleton). Concrete
subcommand dispatch lands at **R409+** per the R326-R459
sister-tools port arc plan. Scope band: **MEDIUM**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBSynthesizer/` (6 `.hs` files).

## Mini-arc scope

Synthetic chain generator for stress tests. Phase C.1 mini-arc R408-R415 (8 rounds, MEDIUM). R411 leverages `node/src/block_producer.rs` Forging logic.

## Current functional surface (post-R441)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Args` dispatch — forge-limit (slot/block/epoch) +
  open-mode (create/create-force/append) parsed + validated.
- ❌ Forge loop — returns `RunError::ForgeLoopDeferred { config,
  chain_db, limit, mode }` (R441 structured deferral). See **Carve-out
  inventory** below.
- ❌ End-to-end behavioral tests against upstream binary — pending
  Phase C authorization checkpoint (cardano-cli MVS in the parallel
  C-arc must complete first).

## Carve-out inventory (R441 structured deferral surface)

`crates/tools/db-synthesizer/src/status.rs` ships
`forge_loop_status()` returning a `ForgeLoopStatus` descriptor.

| Carve-out                            | Status helper                       | Deferral rationale (one-liner)                                            |
|--------------------------------------|-------------------------------------|---------------------------------------------------------------------------|
| Forge loop + Run.hs supervisor       | `status::forge_loop_status()`       | Gated on Phase C authorization checkpoint (cardano-cli MVS C-arc); once unlocked, leverages `node/src/block_producer.rs` for actual block-construction logic. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-db-synthesizer

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh db-synthesizer --help
node/scripts/run-tools.sh db-synthesizer --version

# Or invoke the binary directly:
target/release/db-synthesizer --help
```

The binary is named `db-synthesizer` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch lands at `R409+`.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `db-synthesizer` is the
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
- 🟡 Next: **R409** — first concrete-impl round of the mini-arc.
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
cargo test -p yggdrasil-db-synthesizer

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/db-synthesizer --help) \
     <(target/debug/db-synthesizer --help)
diff <(.reference-haskell-cardano-node/install/bin/db-synthesizer --version) \
     <(target/debug/db-synthesizer --version)
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
