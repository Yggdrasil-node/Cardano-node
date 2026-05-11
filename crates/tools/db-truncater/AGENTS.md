# Guidance for the pure-Rust port of upstream `db-truncater`.

**Status:** `partial` (functionally complete; awaiting operator
soak gate for verified_11_0_1 promotion). The R326-R459 Phase B.1
mini-arc shipped: R335 skeleton → R347 storage extension
(`ImmutableStore::trim_after_slot`) → R348 typed-config surface
→ R349 `Run.hs` equivalent (open ChainDB → resolve target →
trim → report). Scope band: **SMALL**.

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at: `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBTruncater/` (4 `.hs` files).

## Mini-arc scope (R386-R390 Phase B.1)

ChainDB rollback utility. Yggdrasil ships full operational parity
with upstream's binary at the truncation-procedure level: open
ChainDB, resolve `TruncateAfter::TruncateAfterSlot|TruncateAfterBlock`
to a concrete `SlotNo`, call `ImmutableStore::trim_after_slot`,
return a `TruncateOutcome` carrying `(resolved_slot,
blocks_removed)`. On-disk byte parity is *not* an acceptance goal
(yggdrasil's `FileImmutable` uses a different on-disk layout than
upstream's ChainDB chunked-log format); semantic parity at the
operator-procedure level is sufficient.

## Current functional surface

- ✅ `<binary> --help` byte-equivalent to upstream (golden test
  pinned in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::Args` with `--db PATH`,
  `--truncate-after-slot SLOT_NUMBER`,
  `--truncate-after-block BLOCK_NUM` mutually-exclusive flags
  (R348).
- ✅ `crates/storage/src/immutable_db.rs::ImmutableStore::trim_after_slot`
  trait method + `InMemoryImmutable` + `FileImmutable` impls +
  `ChainDb::truncate_immutable_after_slot` wrapper (R347).
- ✅ `crates/tools/db-truncater/src/run.rs::run()` opens a
  `FileImmutable::open(&config.db_dir)` → calls
  `resolve_target()` → calls `store.trim_after_slot()` → returns
  `TruncateOutcome { resolved_slot, blocks_removed }` (R349).
- ✅ End-to-end integration tests cover both
  `TruncateAfterSlot` (slot passes through verbatim) and
  `TruncateAfterBlock` (looked up via `suffix_after`).

## Carve-outs surviving the mini-arc

- **Operator soak vs. upstream binary** — the only remaining
  gate before parity-matrix `partial → verified_11_0_1`
  promotion. Operator runs
  `node/scripts/compare_db_truncater_to_upstream.sh` against a
  synthesized upstream + yggdrasil ChainDB pair; the script
  asserts the post-truncate tip slot + block count match. This
  is an operator-side task — the crate code itself is complete.

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-db-truncater

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh db-truncater --help
node/scripts/run-tools.sh db-truncater --version

# Run a truncation:
node/scripts/run-tools.sh db-truncater \
  --db /path/to/chaindb \
  --truncate-after-slot 12345

# Or invoke the binary directly:
target/release/db-truncater --help
```

The binary is named `db-truncater` (matching upstream exactly) —
operators can swap upstream's binary for the yggdrasil one in
their automation now that R349 ships full procedure parity.
**Caveat:** the on-disk ChainDB layout differs; the yggdrasil
binary truncates yggdrasil-format chains, not upstream-format
chains.

## Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Procedure-level byte-equivalence with upstream `db-truncater`
  (truncation semantics, error-reporting shape) is the
  acceptance gate. On-disk format byte-equivalence is *not*
  required.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `node/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Post-R349 status:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at
  R335-R336).
- ✅ Storage extension (R347 `ImmutableStore::trim_after_slot`).
- ✅ Typed config surface (R348).
- ✅ `Run.hs` equivalent (R349).
- 🟡 **Closeout** (operator side): run
  `node/scripts/compare_db_truncater_to_upstream.sh` and report.
  When the soak passes, parity-matrix advances `partial →
  verified_11_0_1`.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte
on `--help` / `--version`:

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

For the truncation-procedure soak (the gate for
verified_11_0_1):

```bash
# Synthesize an upstream + yggdrasil ChainDB pair, truncate both
# to the same slot, diff the resulting tips:
bash node/scripts/compare_db_truncater_to_upstream.sh
```

## Maintenance Guidance

- Update this AGENTS.md when the operator soak completes
  successfully (mark the parity-matrix promotion as shipped).
- Keep the per-tool round numbers in sync with the authoritative
  plan file at `/home/daniel/.claude/plans/playful-tickling-plum.md`
  + `CHANGELOG.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
