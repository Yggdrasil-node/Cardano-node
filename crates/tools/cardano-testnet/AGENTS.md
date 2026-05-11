# Guidance for the pure-Rust port of upstream `cardano-testnet`.

**Status:** `partial` (post-R335-pattern skeleton). Concrete
subcommand dispatch lands at **R417+** per the R326-R459
sister-tools port arc plan. Scope band: **LARGE**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/cardano-testnet/` (82 `.hs` files).

## Mini-arc scope

Local multi-node testnet harness. Phase C.2 mini-arc R416-R433 (18 rounds, LARGE). Hedgehog Process/Property carve-out approved at plan time — Rust uses tokio::process + proptest instead. R424 drives CLI-MVS from the parallel C-arc cardano-cli completion. Hard-gated on CLI-MVS (keys/tx/query/genesis/governance) being verified in C-arc.

## Current functional surface (post-R445)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Command` dispatch — 3 subcommands recognized
  (`cardano`, `create-env`, `version`).
- ❌ Per-subcommand era-aware dispatch — returns
  `RunError::SubcommandEraDispatchDeferred { subcommand: status::Subcommand }`
  (R445 structured deferral). See **Carve-out inventory** below.
- ❌ End-to-end behavioral tests against upstream binary — pending
  the cardano-testnet mini-arc (R416-R433) + yggdrasil-ledger era
  surface being exposed at crate boundaries.

## Carve-out inventory (R445 structured deferral surface)

`crates/tools/cardano-testnet/src/status.rs` ships a typed
`Subcommand` enum (3 verbs: `cardano`, `create-env`, `version`) +
`era_dispatch_status()` helper.

| Carve-out                            | Status helper                       | Deferral rationale (one-liner)                                            |
|--------------------------------------|-------------------------------------|---------------------------------------------------------------------------|
| Per-subcommand era-aware dispatch    | `status::era_dispatch_status()`     | Gated on cardano-testnet mini-arc (R416-R433 — LARGE; 32 upstream `.hs` files; Hedgehog Process/Property modules approved as Rust-idiomatic carve-out using `tokio::process` + `proptest`) AND on yggdrasil-ledger's era surface being exposed at crate boundaries. |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-cardano-testnet

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh cardano-testnet --help
node/scripts/run-tools.sh cardano-testnet --version

# Or invoke the binary directly:
target/release/cardano-testnet --help
```

The binary is named `cardano-testnet` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch lands at `R417+`.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `cardano-testnet` is the
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
- 🟡 Next: **R417** — first concrete-impl round of the mini-arc.
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
cargo test -p yggdrasil-cardano-testnet

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/cardano-testnet --help) \
     <(target/debug/cardano-testnet --help)
diff <(.reference-haskell-cardano-node/install/bin/cardano-testnet --version) \
     <(target/debug/cardano-testnet --version)
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
