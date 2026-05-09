# Round 287 — `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade

**Date:** 2026-05-09
**Phase:** C (tech-debt purge — closing round)
**Predecessor:** R286 (`docs/operational-runs/2026-05-09-round-286-marker-and-helper-cleanup.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Re-grade the two living planning docs that pre-date the R269–R281
work but still describe state as if pending:

1. `docs/code-audit.md` (896 lines, dated 2026-04-27) — every
   Critical / High / Medium / Low finding is closed per
   `docs/ARCHITECTURE.md:58` ("The 2026-Q3 operational pass on `main`
   then closed the [audit C-1/H-1/H-2/M-1..M-8/L-1..L-9](code-audit.md)
   findings"). Annotate each finding heading inline; the original
   audit body is preserved as historical evidence.

2. `docs/REFACTOR_BLUEPRINT.md` (269 lines, dated 2026-05-06) — Phase
   C–G refactors all shipped via R269–R281. Mark each phase header
   with its closing-round annotation; replace the "What remains
   (R257-R262 candidates)" planning table with a "What's already done
   (R269 — R281 closure)" status table.

Docs-only round; zero code changes; zero test impact.

## Resolution

### `code-audit.md`

Inserted a **Section 0 — Status — closure update (2026-05-09)**
banner at the top stating that all 20 findings (1 Critical, 2 High,
8 Medium, 9 Low) closed. Body preserved verbatim per the historical-
evidence rule.

Each Critical / High / Medium / Low finding heading received an inline
`[CLOSED in 2026-Q3]` annotation:

- `#### C-1 — Pre-auth remote process abort ... [CLOSED in 2026-Q3]`
- `#### H-1 — Same unbounded-allocation pattern ... [CLOSED in 2026-Q3]`
- `#### H-2 — Inbound accept loop runs handshake ... [CLOSED in 2026-Q3]`
- `#### M-1` through `#### M-8` `[CLOSED in 2026-Q3]`
- `#### L-1` through `#### L-9` `[CLOSED in 2026-Q3]`

Informational findings I-1 through I-15 are positive observations, not
issues to close, so they keep their original headings.

### `REFACTOR_BLUEPRINT.md`

Updated the document header date + status banner:

> **Status — all R256 phases (A through G) shipped.** This document
> was authored to plan the R256 Phase C–G monolith splits. Those
> phases have all landed via R269 (state.rs split, Phase C), R270
> (governor.rs split, Phase E), R271 (runtime.rs split, Phase D-runtime),
> R272 (epoch_boundary.rs split, Phase G), R273 (subsystem submodule
> splits), and R274–R281 (strict-mirror naming-parity sweeps).

Annotated each phase header:

- `## Phase D — runtime.rs + sync.rs split [DONE in R271 + R269]`
- `## Phase E — governor.rs split [DONE in R270]`
- `## Phase F — local_server.rs split [DONE in R270 / partially R273]`
- `## Phase G — epoch_boundary.rs split [DONE in R272]`
- `## Phase C — state.rs split (the big one) [DONE in R269 a–w + R276]`

Replaced the "What remains (R257-R262 candidates)" planning table
with a "What's already done (R269 — R281 closure)" status table that
maps each Phase to its closing round and short summary, plus pointers
at:
- `docs/strict-mirror-audit.tsv` for per-file verdicts.
- `docs/operational-runs/` for per-round operational records.

The remaining-work section is gone; only historical context + closed
status remain.

## Verification gates

```text
cargo fmt --all -- --check          clean (no Rust changes)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

This is a docs-only round; `cargo check` / `cargo lint` are
unaffected.

## Diff stat

```text
docs/code-audit.md           +25 lines (status banner + 20 inline
                                          [CLOSED in 2026-Q3] annotations)
docs/REFACTOR_BLUEPRINT.md   +35 lines (status banner + 5 phase-header
                                          annotations + replaced planning
                                          table with closure status table)
docs/operational-runs/2026-05-09-round-287-... (new)
```

## Stop point — Phase C closed

| Round | Site | Status |
|---|---|---|
| R282 | `block_producer.rs::description` | ✅ closed |
| R283 | `sync.rs era_tag` + `local_server.rs lsq_era_index` | ✅ closed |
| R284 | `local_server.rs:713` LSQ TODO | ✅ closed |
| R285 | `peer_management.rs` Phase-6 allows | ✅ closed |
| R286 | `reconnecting.rs` marker + `shelley.rs` test helper | ✅ closed |
| **R287** | `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade | ✅ closed |

**Phase C is complete.** All 6 rounds shipped. Production-side
`#[allow(dead_code)]` count is zero; production-side TODO/FIXME count
is zero; both legacy planning docs carry accurate closure status
banners.

## Phase A–C cumulative state

After R287:
- Production-side `#[allow(dead_code)]`: **0** (down from 9 at R281)
- Production-side `TODO/FIXME`: **0** (down from 1 at R283)
- Strict-mirror audit-table verdicts: 52 (a) + 157 (c) = **209
  files, all graded** (down from 24 (c-needed) + 73 NEEDS-REVIEW at
  R275)
- Living planning docs with stale "as-pending" descriptions: **0**
  (code-audit.md + REFACTOR_BLUEPRINT.md re-graded in R287)
- Cardano-node policy tag: **11.0.1** (refreshed in R274)
- 4 cargo gates: green at every round (4,855 tests preserved)
- CI strict-mirror drift-guard: warn-only, zero violations

Phase D (living-doc parity language sweep) and R288 (drift-guard
fail-build flip) close out the strict-1:1-file-mirror arc.

## Next round

R288 — flip `scripts/check-strict-mirror.py` from warn-only to
fail-build. Per the plan, this is the closure of Phase E and the
strict-1:1-file-mirror policy goes from "informational warning" to
"required CI gate".

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R286 (`docs/operational-runs/2026-05-09-round-286-marker-and-helper-cleanup.md`)
- Closure evidence: `docs/ARCHITECTURE.md:58` (2026-Q3 audit closure)
