# Phase D — Living-doc parity language sweep

**Date:** 2026-05-09
**Phase:** D (parallel with Phase C)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Update living docs to:
1. Cite the new strict 1:1 file-mirror policy from R274 across all
   per-crate `AGENTS.md` files.
2. Register `python3 scripts/check-strict-mirror.py` and
   `python3 scripts/audit-strict-mirror.py` as parity-flow gates in
   the root `AGENTS.md`.
3. Strip "pragmatic mirror" / "not 1:1" / "best-effort mirror" hedges
   from the docs body (none found — the language was already clean).

Phase D ships only docs; zero code changes; zero test impact. It
runs in parallel with Phase C.

## Investigation

`grep -rin "pragmatic mirror\|not 1:1\|best.effort.mirror\|pragmatic-mirror"`
across `CLAUDE.md`, `AGENTS.md`, `docs/*.md`, and per-crate
`AGENTS.md` returned **zero hits** in living docs. Historical
operational-runs are intentionally excluded per the round-doc
immutability rule.

The living-doc language was already clean — no hedge purge needed.
The work-product of Phase D is therefore positive additions only:
verification-gate registration + strict-mirror policy citations.

## Resolution

### `AGENTS.md` (root) — verification-gate updates

Updated the **Verification Expectations** section's Parity-flow gates
list to include the two new Python checkers:

- `python3 scripts/check-strict-mirror.py` — strict 1:1 file-mirror
  drift-guard (warn-only since R275; fail-build at R288).
- `python3 scripts/audit-strict-mirror.py` — rebuilds
  `docs/strict-mirror-audit.tsv` after Phase B graduates rows.

Added a paragraph **Strict 1:1 file-mirror policy (R274 onward)**
stating the policy in one place, with cross-references to:
- the authoring-time skill at
  `.claude/skills/round-extraction/SKILL.md`,
- the CI counterpart `python3 scripts/check-strict-mirror.py`,
- the allowlist source-of-truth `docs/strict-mirror-audit.tsv`.

Updated the Parity-flow surfaces list to register
`docs/strict-mirror-audit.tsv` and `docs/upstream-haskell-files.txt`
as documented surfaces.

### Per-crate `AGENTS.md` × 13

Inserted a new `## Strict 1:1 file-mirror policy (R274+)` section near
the top of each per-crate `AGENTS.md` file, immediately after the
file's title heading and before the first `##` heading:

```markdown
## Strict 1:1 file-mirror policy (R274+)

Every production `.rs` here either mirrors a single canonical upstream
`.hs` file by snake_case basename (with directory-prefix fallback for
sibling collisions) OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces. CI gate: `python3 scripts/check-strict-mirror.py`
(warn-only since R275; fail-build at R288). Allowlist source-of-truth:
[`docs/strict-mirror-audit.tsv`](<rel-link>).
```

The relative link to the audit TSV is depth-adjusted per file
location:
- `crates/AGENTS.md` → `../docs/strict-mirror-audit.tsv`
- `crates/<crate>/AGENTS.md` → `../../docs/strict-mirror-audit.tsv`
- `crates/consensus/src/mempool/AGENTS.md` → `../../../../docs/strict-mirror-audit.tsv`
- etc.

Files updated:

| Path |
|---|
| `crates/AGENTS.md` |
| `crates/consensus/AGENTS.md` |
| `crates/consensus/src/mempool/AGENTS.md` |
| `crates/crypto/AGENTS.md` |
| `crates/ledger/AGENTS.md` |
| `crates/network/AGENTS.md` |
| `crates/plutus/AGENTS.md` |
| `crates/storage/AGENTS.md` |
| `docs/AGENTS.md` |
| `node/AGENTS.md` |
| `node/configuration/AGENTS.md` |
| `node/src/AGENTS.md` |
| `specs/AGENTS.md` |

The `CLAUDE.md` Commands section was already updated in R275 to
register `check-strict-mirror.py` and `audit-strict-mirror.py`; no
change needed here.

### Other living docs (read-through)

Reviewed:
- `docs/PARITY_PLAN.md` — no hedged-mirror language.
- `docs/PARITY_SUMMARY.md` — no hedged-mirror language.
- `docs/PARITY_PROOF.md` — no hedged-mirror language.
- `docs/UPSTREAM_PARITY.md` — no hedged-mirror language.
- `docs/ARCHITECTURE.md` — no hedged-mirror language.
- `docs/manual/*.md` × 11 — operator-facing; reviewed; no hedged-
  mirror language.

## Critical: docs/operational-runs/ NOT edited

Per the doc-immutability rule, historical round-docs under
`docs/operational-runs/` are NOT edited. They record what was true at
their authoring date. Adjusting them retrospectively destroys round
archeology.

## Verification gates

```text
cargo fmt --all -- --check          clean (no Rust changes)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

This is a docs-only round; `cargo check` / `cargo lint` are
unaffected.

## Diff stat

```text
AGENTS.md                                   +24 lines (gates section)
crates/AGENTS.md                            +12 lines (strict-mirror section)
crates/consensus/AGENTS.md                  +12 lines
crates/consensus/src/mempool/AGENTS.md      +12 lines
crates/crypto/AGENTS.md                     +12 lines
crates/ledger/AGENTS.md                     +12 lines
crates/network/AGENTS.md                    +12 lines
crates/plutus/AGENTS.md                     +12 lines
crates/storage/AGENTS.md                    +12 lines
docs/AGENTS.md                              +12 lines
node/AGENTS.md                              +12 lines
node/configuration/AGENTS.md                +12 lines
node/src/AGENTS.md                          +12 lines
specs/AGENTS.md                             +12 lines
docs/operational-runs/2026-05-09-phase-d-... (new)
```

## Stop point — Phase D complete

Phase D shipped its two work products:
1. ✅ Root `AGENTS.md` Verification Expectations updated for R275+
   parity-flow gates and the strict 1:1 file-mirror policy paragraph.
2. ✅ Per-crate `AGENTS.md` × 13 carry the strict-mirror citation +
   pointer at the audit TSV.

| Phase | Rounds | Status |
|---|---|---|
| A — Discovery & guardrail bootstrap | R274, R275 | ✅ closed |
| B — Targeted renames + docstrings | R276 … R281 | ✅ closed |
| C — Tech-debt purge | R282 … R287 | ✅ closed |
| **D — Living-doc parity language sweep** | (parallel to C) | ✅ **closed** |
| E — Drift-guard fail-build flip | R288 | next |
| F — cardano-cli surface expansion | R289 … R295 | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Phase A: R274 (`docs/operational-runs/2026-05-09-round-274-strict-mirror-discovery.md`),
  R275 (`docs/operational-runs/2026-05-09-round-275-strict-mirror-drift-guard.md`)
- Phase B: R276–R281
- Phase C: R282–R287
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
