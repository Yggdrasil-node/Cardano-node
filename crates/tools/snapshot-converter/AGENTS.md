# Guidance for the pure-Rust port of upstream `snapshot-converter`.

**Status:** `partial` (post-R335-pattern skeleton). Concrete
subcommand dispatch lands at **R402+** per the R326-R459
sister-tools port arc plan. Scope band: **MEDIUM**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/app/snapshot-converter.hs` (1 `.hs` files).

## Mini-arc scope

Ledger snapshot format converter (mem ↔ lmdb ↔ lsm). Phase B.3 mini-arc R401-R407 (7 rounds, MEDIUM). Single dense file (~245 lines); R405 covers all 9 (3×3) input/output combinations with golden tests.

## Current functional surface (post-R446)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Config` dispatch — Daemon / Oneshot modes parsed
  + validated.
- ✅ R446 `LedgerSnapshotVersion` scaffolding in
  `crates/storage/src/ledger_db.rs` — version-tag newtype + `MAGIC`
  + `V1` / `LATEST` constants + `detect_version()` helper +
  `LedgerStore::latest_snapshot_version()` trait method. Gates the
  future V1↔V2 migration body.
- ❌ Concrete conversion dispatch — returns
  `RunError::ConvertSnapshotDeferred { mode }` (R439 structured
  deferral; the prior raw `eyre!` stub was replaced at R439). See
  the **Carve-out inventory** section below for the deferral
  rationale.
- ❌ End-to-end behavioral tests against upstream binary — pending
  the V2 snapshot format being defined (R446 gates this).

## Carve-out inventory (R439 / R446 structured deferral surface)

The deferred surfaces are surfaced programmatically via
`crates/tools/snapshot-converter/src/status.rs`. Callers can
match on `RunError` variants for programmatic dispatch + grep
`fn .*_status()` across the workspace to enumerate deferrals.

| Carve-out                            | Status helper                                    | Deferral rationale (one-liner)                                            |
|--------------------------------------|--------------------------------------------------|---------------------------------------------------------------------------|
| mem↔lsm `convertSnapshot` logic      | `status::convert_snapshot_status()`              | Gated on yggdrasil-format LedgerStore reader/writer (R446 scaffolds the version-tag scheme; V1↔V2 migration body lands when format evolves). |
| filesystem-watcher daemon            | `status::daemon_watcher_status()`                | Depends on `convert_snapshot` logic itself; Yggdrasil uses the `notify` crate when ready. |

Honest re-scoping vs upstream: the upstream 3×3
`mem↔lmdb↔lsm` matrix collapses for Yggdrasil (single backend);
real scope is format-version migration over time. See
[R446 operational-runs doc](../../../docs/operational-runs/2026-05-11-round-446-snapshot-converter-format-design.md)
for the design rationale.

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-snapshot-converter

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh snapshot-converter --help
node/scripts/run-tools.sh snapshot-converter --version

# Or invoke the binary directly:
target/release/snapshot-converter --help
```

The binary is named `snapshot-converter` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one in their automation
once concrete dispatch lands at `R402+`.

##  Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format byte-equivalence with upstream `snapshot-converter` is the
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
- 🟡 Next: **R402** — first concrete-impl round of the mini-arc.
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
cargo test -p yggdrasil-snapshot-converter

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/snapshot-converter --help) \
     <(target/debug/snapshot-converter --help)
diff <(.reference-haskell-cardano-node/install/bin/snapshot-converter --version) \
     <(target/debug/snapshot-converter --version)
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
