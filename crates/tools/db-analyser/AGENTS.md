# Guidance for the pure-Rust port of upstream `db-analyser`.

**Status:** `partial` (post-R493 dispatch-coverage matrix
complete). **12/13** of upstream's `AnalysisName` variants ship
end-to-end; **1/13** permanent carve-out (`CheckNoThunksEvery`
— Rust has no GHC-side laziness/thunks). **13/13 final
verdicts** — zero remaining `RequiresLedgerStateApplyLoop`
deferrals. Scope band: **MEDIUM** (R475-R493 arc + follow-ons
shipped; remaining work is forensic-fidelity hardening, not
dispatch-coverage gaps).

## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate:
`python3 scripts/check-strict-mirror.py --fail-on-violation`.

## Upstream source

Vendored at:
`.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/unstable-cardano-tools/Cardano/Tools/DBAnalyser/`
(13 `.hs` files).

The Byron `knownEBBs` registry consumed by `ShowEBBs` is at
`.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/byron/Ouroboros/Consensus/Byron/EBBs.hs`.

## Mini-arc scope

ChainDB forensic analyser. Phase B.2 mini-arc R391-R400 was rolled
into the R475-R491 post-R459 follow-on arc which shipped the full
HasAnalysis surface + analysis dispatch core + 11-of-13 handlers
(R479-R480: 7 block-iteration-only; R488-R491: 4 via
LedgerState::apply_block seam) + 1 permanent carve-out
(CheckNoThunksEvery, R485) + end-to-end `FileImmutable` wire-up.
Operates on Yggdrasil's ChainDB format (semantic parity with
upstream binary, not on-disk-format byte parity since the storage
layer diverges).

## Current functional surface (post-R491)

- ✅ `<binary> --help` byte-equivalent to upstream (golden test pinned
  in `tests/cli_help_golden.rs`).
- ✅ `<binary> --version` byte-equivalent to upstream.
- ✅ Typed `parser::DBAnalyserConfig` dispatch — db path, analysis
  name, ledger-DB backend, conf-limit parsed + validated.
- ✅ Per-era `Tx::output_count(era)` dispatcher in
  `crates/ledger/src/tx.rs` + per-era `decode_output_count`
  helpers under `crates/ledger/src/eras/*` (R475).
- ✅ `impl HasAnalysis for yggdrasil_ledger::Block` in
  `src/has_analysis.rs` (R476) — collapses upstream's per-era
  typeclass instances into a single match-on-Era dispatcher.
- ✅ Byron known-EBB registry at `src/byron_ebbs.rs` (R476;
  325-entry strict-mirror of upstream `EBBs.hs`).
- ✅ `analysis::runner::run_analysis` dispatch core
  (`src/analysis/runner.rs`) ports upstream `Analysis.hs::runAnalysis`
  (R479).
- ✅ End-to-end run path via `lib.rs::run`: opens
  `FileImmutable::open(&config.db_dir)` → walks
  `ImmutableStore::iter_after(&Point::Origin)` (R482 streaming
  iter) → dispatches → renders to stdout (R481+R482).
- ✅ 6 integration tests at `tests/end_to_end_chain_walk.rs`
  exercise the production call path against a temp ChainDB.

### Dispatch coverage matrix

| AnalysisName | Verdict | Shipping round |
|--------------|---------|----------------|
| `ShowSlotBlockNo` | ✅ shipped | R479 |
| `CountBlocks` | ✅ shipped | R479 |
| `CountTxOutputs` | ✅ shipped | R479 |
| `ShowBlockHeaderSize` | ✅ shipped | R479 |
| `ShowBlockTxsSize` | ✅ shipped | R480 |
| `ShowEBBs` | ✅ shipped | R480 |
| `OnlyValidation` | ✅ shipped | R480 |
| `StoreLedgerStateAt` | ✅ shipped (LedgerStateCheckpoint CBOR snapshot) | R491 |
| `CheckNoThunksEvery` | ⛔ `NotApplicableToRust` | R485 (permanent carve-out) |
| `TraceLedgerProcessing` | ✅ shipped (forensic Ok/Err trace) | R488 |
| `BenchmarkLedgerOps` | ✅ shipped (Instant timing into SlotDataPoint) | R489 |
| `ReproMempoolAndForge` | ✅ shipped (yggdrasil-consensus Mempool insert + pop_best timing) | R493 |
| `GetBlockApplicationMetrics` | ✅ shipped (R476 column closures + every-N sampling) | R490 |

## Carve-out inventory (post-R491)

`crates/tools/db-analyser/src/status.rs` ships
`analysis_dispatch_status()` returning an `AnalysisDispatchStatus`
descriptor. **Post-R481 status:** `block-only-shipped`.

| Carve-out | Status helper | Deferral rationale |
|-----------|---------------|--------------------|
| 0 ledger-state-dependent analyses (post-R493) | n/a | Dispatch matrix fully covered. After R493, no `AnalysisName` returns `RequiresLedgerStateApplyLoop`. The only remaining error variant operators can hit is `NotApplicableToRust` for `CheckNoThunksEvery` (R485 permanent carve-out). |
| Forensic-fidelity hardening (post-R493) | n/a | R488 `TraceLedgerProcessing` emits Ok/Err per block but `Block::emit_traces` is the R476 placeholder. R493 `ReproMempoolAndForge` uses a simplified `MempoolEntry` (fee=0, ttl=u64::MAX, empty inputs) — no fee-decoding, ttl-derivation, conflict-detection, or revalidation-against-ledger-state. Closing these gaps needs richer per-era helpers + genesis-bootstrap CLI flags. Not blocking the dispatch matrix. |
| `CheckNoThunksEvery` (permanent) | `status::analysis_dispatch_status()` (R485) | Fundamentally not portable to Rust. Upstream `checkNoThunks` uses `NoThunks.unsafeNoThunks` to walk GHC's lazy heap for unevaluated thunks; Rust is eagerly evaluated and has no runtime thunks. Returns `AnalysisError::NotApplicableToRust` with the explanation in the error message. |
| `TraceLedgerProcessing` trace content | `analysis::runner::analysis_trace_ledger_processing` (R488) | Yggdrasil's R488 handler captures per-block apply Ok/Err outcomes. Upstream's `traceLedgerProcessing` calls `emit_traces` per block, which returns ledger-state-derived traces (epoch boundary, stake delta, etc.). Yggdrasil's `Block::emit_traces` returns empty (R476 placeholder); closing this trace-content gap needs genesis-bootstrap CLI flags + a richer `emit_traces` body — separate future arc. |
| On-disk-streaming `FileImmutable` | (no helper — operational concern) | R482's `iter_after` saves the intermediate `Vec` allocation but the `FileImmutable` impl still loads every block into `self.index: HashMap<HeaderHash, Block>` at open time. A revision that lazy-loads CBOR records from disk on-demand would close the multi-terabyte memory gap fully; gated on a chunked-log on-disk format design (separate arc). |
| Per-analysis byte-equivalent stdout vs upstream binary | (operational soak — no helper) | `lib.rs::render_outcome` emits an upstream-compatible-shape stdout (e.g. `slot=N block_no=M hash=...; total_blocks=K`). A formal byte-by-byte soak against `.reference-haskell-cardano-node/install/bin/db-analyser` is a follow-on integration round (not blocking — `AnalysisOutcome` is the canonical Yggdrasil-side contract). |

## Build + run

```bash
# Build (release).
cargo build --release -p yggdrasil-db-analyser

# Run via the universal launcher (recommended).
node/scripts/run-tools.sh db-analyser --help
node/scripts/run-tools.sh db-analyser --version

# Run an analysis (R481+R482 wire-up):
node/scripts/run-tools.sh db-analyser \
  --db /path/to/chaindb \
  --analysis count-blocks

# Or invoke the binary directly:
target/release/db-analyser --help
```

The binary is named `db-analyser` (matching upstream exactly) — operators
can swap upstream's binary for the yggdrasil one for the 7 shipped
analyses. The 6 ledger-state-dependent analyses return a clear
operator-readable error naming the dependency.

## Rules *Non-Negotiable*

- Every new sub-module file MUST mirror an upstream `.hs` file by
  snake_case basename or carry a `## Naming parity` block.
- Wire-format / stdout byte-equivalence with upstream `db-analyser`
  is the acceptance gate for any concrete handler — see the
  carve-out inventory above for the documented stdout-shape soak.
- No FFI; no Haskell wrapping. Pure-Rust ecosystem dependencies
  from crates.io are allowed if license-compatible (see
  `docs/DEPENDENCIES.md`).
- Help-text fixtures (`tests/fixtures/upstream-{help,version}.txt`)
  are the source of truth for `--help`/`--version`. If upstream
  ships a new release with different help output, refresh the
  fixtures + bump the relevant SHA pin in
  `node/src/upstream_pins.rs` as a coordinated round.

## Round roadmap

Post-R482 status:

- ✅ Skeleton shipped (R327 + R335-pattern bulk skeleton at R335-R336).
- ✅ Typed config + parser dispatch (R351 + R365).
- ✅ CSV writers + HasAnalysis trait + BenchmarkLedgerOps leaf trio
  (R372-R376).
- ✅ Per-era `Tx::output_count` + `HasAnalysis for Block` impl +
  Byron EBB registry (R475-R476).
- ✅ Per-era dispatch coverage (R477-R478).
- ✅ `analysis::runner::run_analysis` core + 7/13 handlers (R479-R480).
- ✅ End-to-end `lib.rs::run` wire-up + `iter_after` streaming
  (R481-R482).
- 🟡 **Next:** ledger-state apply-loop arc (multi-round, future) →
  unblocks the 6 remaining analyses.
- 🟡 **Optional:** on-disk-streaming `FileImmutable` redesign
  (multi-terabyte memory profile improvement).
- 🟡 **Closeout:** parity-matrix entry advances `partial →
  verified_11_0_1` after the ledger-state apply-loop arc + the
  stdout-shape soak.

## Comparison-with-upstream procedure

To verify the yggdrasil binary still tracks upstream byte-for-byte
on `--help` / `--version`:

```bash
# 1. Refresh vendored upstream tree (only when bumping the upstream version).
bash scripts/setup-reference.sh

# 2. Run cargo test for the crate.
cargo test -p yggdrasil-db-analyser

# 3. Compare --help / --version byte-for-byte.
diff <(.reference-haskell-cardano-node/install/bin/db-analyser --help) \
     <(target/debug/db-analyser --help)
diff <(.reference-haskell-cardano-node/install/bin/db-analyser --version) \
     <(target/debug/db-analyser --version)
# (empty diffs expected — byte-equivalent)
```

For the 7 shipped per-analysis stdout shapes, the operational soak
(documented in the carve-out inventory above) would diff the same
way on a fixture ChainDB. Acceptance criteria: stdout shape matches
upstream OR is documented as semantically-equivalent here.

## Maintenance Guidance

- Update this AGENTS.md when ledger-state apply-loop arc ships
  (replace `🚧 RequiresLedgerStateApplyLoop` rows in the dispatch
  matrix with `✅ shipped` + round number).
- Keep the per-tool round numbers in sync with the authoritative
  plan file at `/home/daniel/.claude/plans/playful-tickling-plum.md`
  + `CHANGELOG.md`.
- If upstream ships a new release: refresh the help/version
  fixtures, advance the relevant SHA pin in `upstream_pins.rs`,
  re-run the full cargo gate.
