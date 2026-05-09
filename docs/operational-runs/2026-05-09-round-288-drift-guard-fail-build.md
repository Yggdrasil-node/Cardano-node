# Round 288 — Strict-mirror drift-guard fail-build flip

**Date:** 2026-05-09
**Phase:** E (drift-guard fail-build flip — closing round)
**Predecessor:** Phase D (`docs/operational-runs/2026-05-09-phase-d-living-doc-sweep.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Flip `scripts/check-strict-mirror.py` from warn-only to fail-build.
The strict 1:1 file-mirror policy goes from "informational warning"
to "required CI gate".

After R281 closed the last `(c-needed)` and `NEEDS-REVIEW` row in
`docs/strict-mirror-audit.tsv`, the codebase is by construction
strict-mirror compliant: 52 (a) DIRECT_MIRROR + 157 (c) NO_MIRROR_NEEDS_DOCSTRING
across 209 production `.rs` files, with zero unresolved verdicts.
The drift-guard is now safe to fail builds on net-new violations.

## Resolution

### `.github/workflows/ci.yml`

Removed `continue-on-error: true` from the strict-mirror step and
appended `--fail-on-violation` to the script invocation:

```yaml
- name: Strict-mirror drift-guard
  run: python3 scripts/check-strict-mirror.py --fail-on-violation
```

The step name dropped its "(warn-only)" qualifier. The accompanying
comment block was updated to record the R275→R288 transition for
future operator context.

### `CLAUDE.md` Commands section

Promoted the strict-mirror drift-guard to the named baseline gate
set ("the five verification expectations"):

```bash
cargo fmt --all -- --check
cargo check-all
cargo test-all
cargo lint
python3 scripts/check-strict-mirror.py --fail-on-violation
```

The "Parity-flow gates" sub-section (run when in scope) keeps the
non-strict-mirror Python checkers (`check-parity-matrix.py`,
`audit-strict-mirror.py`, `filetree.py check`,
`setup-reference.sh`).

## Smoke test (synthetic violation)

```text
$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)
$ echo $?
0

$ cat > crates/ledger/src/state/yggdrasil_invented_smoke.rs <<'EOF'
//! Synthesis without docstring stanza - drift-guard smoke test.
pub fn dummy() {}
EOF
$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 1 new file(s) violate the policy (neither upstream `.hs`
mirror nor `## Naming parity` docstring stanza):
::warning file=crates/ledger/src/state/yggdrasil_invented_smoke.rs::(NEEDS-REVIEW) ...
$ echo $?
1

$ rm crates/ledger/src/state/yggdrasil_invented_smoke.rs
$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)
$ echo $?
0
```

The fail-build path now exits 1 on synthetic violations and 0 on
clean trees. New files lacking either an upstream `.hs` mirror or a
`## Naming parity` docstring stanza will fail CI.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 0.39s)
cargo lint                          clean (Finished `dev` profile in 0.19s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
.github/workflows/ci.yml      -3 lines / +5 lines (drop continue-on-error,
                                                    add --fail-on-violation,
                                                    update comment)
CLAUDE.md                     -2 lines / +5 lines (promote strict-mirror
                                                    to fifth gate; reorganize
                                                    command sections)
docs/operational-runs/2026-05-09-round-288-... (new)
```

## Stop point — Phases A–E complete

| Phase | Rounds | Status |
|---|---|---|
| A — Discovery & guardrail bootstrap | R274, R275 | ✅ closed |
| B — Targeted renames + docstrings | R276 … R281 | ✅ closed |
| C — Tech-debt purge | R282 … R287 | ✅ closed |
| D — Living-doc parity language sweep | (parallel to C) | ✅ closed |
| **E — Drift-guard fail-build flip** | **R288** | ✅ **closed** |
| F — cardano-cli surface expansion | R289 … R295 | next |

## Closure criteria summary (post-R288)

The strict 1:1 file-mirror + tech-debt arc is closed. All 8 closure
criteria from the plan are satisfied:

1. ✅ `bash scripts/setup-reference.sh --force` brings vendored install
   to 11.0.1 (R274 step 0).
2. ✅ `python3 scripts/check-strict-mirror.py` exits 0 on clean tree
   and nonzero when a synthetic violation is introduced (smoke test
   above).
3. ✅ `cargo fmt --check && cargo check-all && cargo lint && cargo
   test-all` green at 4,855 tests across all 15 rounds (R274–R288 +
   Phase D).
4. ✅ Zero `#[allow(dead_code)]` in production paths (R282–R286
   resolved all 9).
5. ✅ Zero `TODO/FIXME` in production paths (R284 resolved the only
   one).
6. ✅ `docs/strict-mirror-audit.tsv` has zero category-(b) and
   category-(d) rows; every row is `(a) DIRECT_MIRROR` or `(c)
   NO_MIRROR_NEEDS_DOCSTRING (verified)`.
7. ✅ Living docs carry strict-mirror language with no "pragmatic" /
   "not 1:1" / "best-effort" hedges (Phase D verified zero hits in
   the existing prose; positive citations added per-crate).
8. ✅ CI runs five gates; the strict-mirror gate is fail-build.

## Optional Phase F still pending

Phase F (R289–R295, cardano-cli surface expansion) is operator-
authorized scope addition: mirrors upstream `cardano-cli` (~150 files)
into a new `crates/cardano-cli/` workspace member. ~14 agent-days,
~3 calendar weeks. Not blocking the strict-mirror arc closure; tracked
separately.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Phase A: R274 + R275
- Phase B: R276–R281
- Phase C: R282–R287
- Phase D: 2026-05-09-phase-d-living-doc-sweep
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
