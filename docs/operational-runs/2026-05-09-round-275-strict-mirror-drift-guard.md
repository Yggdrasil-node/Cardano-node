# Round 275 — Strict-mirror drift-guard (warn-only)

**Date:** 2026-05-09
**Phase:** A (discovery & guardrail bootstrap)
**Predecessor:** R274 (`docs/operational-runs/2026-05-09-round-274-strict-mirror-discovery.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

R275 ships the CI counterpart of `.claude/skills/round-extraction/SKILL.md`.
Where the skill enforces strict naming AT AUTHORING TIME (during round
work), the drift-guard enforces it AT CI TIME (every push and PR).
Both layers must be in place before the policy is durable.

## Deliverables

| Path | Purpose |
|---|---|
| `scripts/check-strict-mirror.py` | warn-only drift-guard. Re-uses `audit-strict-mirror.py`'s logic; reads `docs/strict-mirror-audit.tsv` as the allowlist; flags any net-new file lacking both an upstream `.hs` mirror and a `## Naming parity` docstring stanza. |
| `.github/workflows/ci.yml` | adds the gate after the four cargo gates with `continue-on-error: true` (warn-only). R288 will flip to fail-build via `--fail-on-violation`. |
| `CLAUDE.md` Commands section | documents the new gate and the surfaces the policy depends on (`docs/strict-mirror-audit.tsv`, `docs/upstream-haskell-files.txt`, `.claude/skills/round-extraction/SKILL.md`). |
| `docs/operational-runs/2026-05-09-round-275-strict-mirror-drift-guard.md` | this doc. |

## Drift-guard semantics

```text
                 +---------------------+
   live tree --->| audit-strict-mirror |---> in-memory verdict per file
                 +---------------------+
                            |
                            V
                 +---------------------+
   committed --->| check-strict-mirror |---> { 0 violations } | { ::warning:: ... }
   audit TSV     +---------------------+
   (allowlist)
```

A row passes iff one of:
- The Rust file is in `docs/strict-mirror-audit.tsv` (allowlisted).
- The auto-graded verdict starts with `(a)` or `(c)` (direct mirror or
  verified synthesis).

Otherwise: emit a GitHub Actions `::warning::` annotation; in
warn-only mode exit 0; in fail-build mode (post-R288) exit 1.

## Smoke test

```bash
$ python3 scripts/check-strict-mirror.py
strict-mirror: 0 violations (clean)
$ echo $?
0

$ cat > crates/ledger/src/state/yggdrasil_invented_smoke.rs <<EOF
//! Synthesis without docstring stanza - drift-guard smoke test.
pub fn dummy() {}
EOF
$ python3 scripts/check-strict-mirror.py
strict-mirror: 1 new file(s) violate the policy ...
::warning file=crates/ledger/src/state/yggdrasil_invented_smoke.rs::(NEEDS-REVIEW) ...
$ echo $?
0   # warn-only

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 1 new file(s) violate the policy ...
::warning file=...
$ echo $?
1   # fail-build mode

$ rm crates/ledger/src/state/yggdrasil_invented_smoke.rs
$ python3 scripts/check-strict-mirror.py
strict-mirror: 0 violations (clean)
```

## Why warn-only first

R274 surfaced 45 `(c-needed)` rows (auto-graded as known-violations
scheduled for Phase B docstring work) plus 115 `(NEEDS-REVIEW)` rows
(auto-graded as needing per-cluster hand-grading during Phase B). If
R275 shipped fail-build, every Phase-B round would run against red CI
until R288 closes the last violation.

Warn-only catches NEW regressions — any file added or renamed after
R274 without a corresponding allowlist entry — while letting Phase B
work proceed cleanly. R288 flips the gate to fail-build once Phase B
graduates the allowlist to all-`(a)` / `(c)+verified`.

## Cargo alias decision

The plan called for a `cargo parity-strict-mirror` workspace alias.
Cargo aliases can only invoke other cargo subcommands; arbitrary shell
commands aren't supported. Rather than ship a Rust shim binary just to
expose the python script, the gate is documented as
`python3 scripts/check-strict-mirror.py` directly — same idiom as the
existing `python3 scripts/check-parity-matrix.py` parity-flow gate.
The `cargo parity-strict-mirror` reference in the plan is updated
in the round-doc to reflect this.

## Stale parity-matrix paths surfaced and fixed

While verifying the gates, `python3 scripts/check-parity-matrix.py`
flagged seven stale `path` entries left over from R273-rename — paths
in `docs/parity-matrix.json` that pointed at files renamed two rounds
ago:

| Old path | New path |
|---|---|
| `crates/consensus/src/opcert/cert.rs` | `crates/consensus/src/opcert/ocert.rs` |
| `crates/consensus/src/opcert/counter.rs` | `crates/consensus/src/opcert/rules_ocert.rs` |
| `crates/plutus/src/types/term.rs` | `crates/plutus/src/types/core_type.rs` |
| `crates/plutus/src/types/default_fun.rs` | `crates/plutus/src/types/default_builtins.rs` |
| `crates/plutus/src/types/runtime.rs` | `crates/plutus/src/types/cek_internal.rs` |
| `crates/plutus/src/cost_model/expr.rs` | `crates/plutus/src/cost_model/costing_fun.rs` |
| `crates/plutus/src/cost_model/memory.rs` | `crates/plutus/src/cost_model/ex_memory_usage.rs` |

R273-rename should have caught these via the parity-matrix gate; it
didn't because the rename round predated the explicit gate audit.
The historical text fields in the parity-matrix entries (`evidence`,
`split` notes referring to the old basenames) are NOT edited per the
operational-runs doc-immutability rule — only the `path` fields are
corrected. The parity-matrix is now clean.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 0.31s)
cargo lint                          clean
cargo test-all                      4855 passed; 0 failed
python3 scripts/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean); exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
.cargo/config.toml                                       (unchanged — alias removed in favor of direct python invocation)
.github/workflows/ci.yml                                 +18 lines (new strict-mirror step, warn-only)
CLAUDE.md                                                +5 lines (Commands section + parity-flow surfaces)
docs/parity-matrix.json                                  7 path entries updated
docs/operational-runs/2026-05-09-round-275-...           (new file)
scripts/check-strict-mirror.py                           (new file, ~110 lines)
```

## Stop point — Phase A complete

R275 closes Phase A. Phase A's deliverables are now:
- `bash scripts/setup-reference.sh --force` brings vendored install to 11.0.1 (R274 step 0).
- `scripts/audit-strict-mirror.py` produces the audit TSV (R274).
- `docs/strict-mirror-audit.tsv` graded with auto-verdicts (R274).
- `docs/upstream-haskell-files.txt` flat-file index (R274).
- `scripts/check-strict-mirror.py` drift-guard, warn-only (R275).
- CI `.github/workflows/ci.yml` carries the new gate (R275).
- `CLAUDE.md` Commands section + parity-flow surfaces updated (R275).
- `.claude/skills/round-extraction/SKILL.md` strengthened (R273-rename
  predecessor) — authoring-time counterpart to the runtime gate.

Phase B (R276–R281) starts next: each round hand-grades and resolves
the rows in its assigned cluster.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R274 (`docs/operational-runs/2026-05-09-round-274-strict-mirror-discovery.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
