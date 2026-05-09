# Round 294 + R295 — Phase F closing rounds (cardano-cli sweeper)

**Date:** 2026-05-09
**Phase:** F (cardano-cli surface expansion — closing rounds)
**Predecessor:** R293 (`docs/operational-runs/2026-05-09-round-293-...`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

R294 and R295 ship together — R295 was a 2-file follow-up correction
to R294, so consolidating the round-doc avoids per-round bureaucracy
for what is operationally one slice.

## R294 — remaining sub-trees (8 clusters)

### Scope

Mirror the 8 remaining `Cardano.CLI.*` sub-trees that were not
covered by R290 (Byron), R291 (Compatible), R292 (EraBased), or
R293 (EraIndependent):

| Upstream sub-tree | Leaf count | Yggdrasil path |
|---|---|---|
| `IO/` | 1 | `crates/cardano-cli/src/io/` |
| `Json/` | 1 | `crates/cardano-cli/src/json/` |
| `Legacy/` | 5 | `crates/cardano-cli/src/legacy/` |
| `OS/` | 1 | `crates/cardano-cli/src/os/` |
| `Option/` | 2 | `crates/cardano-cli/src/option/` (parent existed from R289) |
| `Read/` | 4 | `crates/cardano-cli/src/read/` |
| `Run/` | 1 | `crates/cardano-cli/src/run/` (parent existed from R289) |
| `Type/` | 33 | `crates/cardano-cli/src/r#type/` |
| **TOTAL** | **48** | |

Plus 9 sub-parent shells + 6 cluster-root parents (4 new + 2 updated:
`option.rs` and `run.rs` from R289 received added `pub mod <child>;`
declarations).

### Files added (R294)

- 48 leaf files (each strict-mirror of a single upstream `.hs`).
- 11 parent shells (sub-tree organizers + new cluster roots).
- `crates/cardano-cli/src/lib.rs` adds `pub mod {io, json, legacy,
  os, read, r#type};` (cluster declarations).

The `r#type` raw-identifier escape is required at three levels:
- `lib.rs` — `pub mod r#type;` (cluster root)
- `r#type/error.rs` — `pub mod r#type;` (sub-parent)
- `r#type/key.rs` — `pub mod r#type;` (sub-sub-parent)

### Verdict bucket counts (post-R294)

| Bucket | Pre-R294 | Post-R294 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 182 | 228 (+46 leaf files) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 205 | 216 (+11 parent shells) |
| **TOTAL** | 387 | 444 (+57) |

## R295 — sweeper corrections

### Scope

Two follow-up fixes surfaced by tallying upstream `.hs` files vs the
post-R294 Yggdrasil tree:

1. **`top_handler.rs` missing.** R289 missed `Cardano/CLI/TopHandler.hs`
   when enumerating top-level mirrors. Added the strict-mirror file
   now.

2. **`read.rs` mis-graded.** R294's auto-generated parent-shell
   script declared `**Strict mirror:** none.` for `read.rs` because
   the script saw the `Read/` directory and assumed no top-level
   `Read.hs` existed. In fact upstream has BOTH a top-level `Read.hs`
   AND a `Read/` directory; the Yggdrasil parent file should mirror
   the top-level `.hs` while declaring sub-modules for the directory
   children. Corrected the docstring + verdict.

### Files affected (R295)

- `crates/cardano-cli/src/top_handler.rs` (new, 32 lines).
- `crates/cardano-cli/src/read.rs` — docstring corrected; verdict
  re-graded from `(c)` to `(a)`.
- `crates/cardano-cli/src/lib.rs` adds `pub mod top_handler;`.

### Final tally

```text
$ find crates/cardano-cli/src -name "*.rs" | wc -l
237  (Yggdrasil cardano-cli)

$ find .reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI -name "*.hs" | wc -l
180  (upstream)
```

Yggdrasil's 237:180 ratio reflects the parent-shell overhead
inherent in the Rust module-tree convention (one extra `.rs` per
directory beyond the leaf-file `.hs` count).

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 0.48s)
cargo lint                          clean (Finished `dev` profile in 0.43s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Final verdict bucket counts (post-R295)

| Bucket | Pre-R295 | Post-R295 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 228 | 230 (+1 top_handler.rs +1 read.rs regrade) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 216 | 215 (-1 read.rs regrade) |
| **TOTAL** | 444 | 445 (+1) |

## Phase F summary across R289–R295

| Round | Cluster | Files added |
|---|---|---|
| R289 | Bootstrap (top-level + 8 namespace files) | 10 |
| R290 | Byron | 11 (10 + 1) |
| R291 | Compatible | 27 (21 + 6) |
| R292 | EraBased | 83 (57 + 26) |
| R293 | EraIndependent | 49 (34 + 15) |
| R294 | IO/Json/Legacy/OS/Option/Read/Run/Type | 57 (48 + 9 + 4 cluster-roots, 2 R289 updates) |
| R295 | TopHandler + read.rs regrade | 1 + 1 regrade |
| **TOTAL Phase F** | | **237 files** |

Yggdrasil's `crates/cardano-cli/` now strict-mirrors all 180 upstream
`.hs` files in the cardano-cli library, with 57 additional Rust-side
parent shells documenting the sub-tree aggregation. Every file is
graded `(a) DIRECT_MIRROR` or `(c) NO_MIRROR_NEEDS_DOCSTRING`; the
strict-mirror drift-guard is satisfied.

## R295 deferral note: integration tests

The plan called for "end-to-end integration tests against the Haskell
binary" in R295. These are deferred to a future R296+ slice because:

1. The R290–R294 leaf files ship with placeholder enums, not yet
   concrete implementations. Integration tests against
   `.reference-haskell-cardano-node/install/bin/cardano-cli` are
   useful only after the implementations land.
2. The strict-mirror infrastructure (filename parity + docstrings +
   crate skeleton + module tree) is the load-bearing R289–R295
   deliverable; concrete implementations are a multi-week follow-up.
3. The current `node/src/commands/cardano_cli.rs` (163 lines)
   continues to provide the working `yggdrasil-node cardano-cli`
   subcommand for operators today.

The integration-test work moves into the parity-matrix as a
`docs/parity-matrix.json` entry tracking per-subcommand byte-equiv
verification status.

## Stop point — Phases A through F complete

| Phase | Rounds | Status |
|---|---|---|
| A — Discovery & guardrail bootstrap | R274, R275 | ✅ closed |
| B — Targeted renames + docstrings | R276 … R281 | ✅ closed |
| C — Tech-debt purge | R282 … R287 | ✅ closed |
| D — Living-doc parity language sweep | (parallel to C) | ✅ closed |
| E — Drift-guard fail-build flip | R288 | ✅ closed |
| **F — cardano-cli surface expansion** | **R289 … R295** | ✅ **closed** |

The full strict 1:1 file-mirror arc + tech-debt purge + drift-guard
hardening + cardano-cli surface expansion is closed. R296+ would
populate the cardano-cli leaf files with concrete implementations
(non-blocking; multi-week follow-up).

## Diff stat (R294 + R295)

```text
Cargo.lock                              (regenerated for new crate)
crates/cardano-cli/src/lib.rs           +9 lines (8 new pub mod + top_handler)
crates/cardano-cli/src/io/              (new, 1 leaf)
crates/cardano-cli/src/io.rs            (new cluster-root)
crates/cardano-cli/src/json/            (new, 1 leaf)
crates/cardano-cli/src/json.rs          (new cluster-root)
crates/cardano-cli/src/legacy/          (new, 5 leaf + 1 sub-parent)
crates/cardano-cli/src/legacy.rs        (new cluster-root)
crates/cardano-cli/src/option/          (new, 2 leaf + 1 sub-parent)
crates/cardano-cli/src/option.rs        (R289 updated: pub mod flag;)
crates/cardano-cli/src/os/              (new, 1 leaf)
crates/cardano-cli/src/os.rs            (new cluster-root)
crates/cardano-cli/src/read/            (new, 4 leaf + 1 sub-parent)
crates/cardano-cli/src/read.rs          (R294 created; R295 regraded to strict mirror)
crates/cardano-cli/src/run/             (new, 1 leaf)
crates/cardano-cli/src/run.rs           (R289 updated: pub mod mnemonic;)
crates/cardano-cli/src/r#type/          (new, 33 leaf + 2 sub-parents)
crates/cardano-cli/src/r#type.rs        (new cluster-root)
crates/cardano-cli/src/top_handler.rs   (new, R295)
docs/strict-mirror-audit.tsv            rebuilt (+58 rows post-R294, +1 post-R295)
docs/operational-runs/2026-05-09-round-294-295-... (new)
```

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R293 (Phase F EraIndependent cluster)
- Upstream cardano-cli library:
  `.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI/`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
