# Round 298 — `docs/` cleanup phase 1 (archive closed docs)

**Date:** 2026-05-09
**Phase:** Documentation hygiene (operator-requested cleanup)
**Predecessor:** R297 (`docs/operational-runs/2026-05-09-round-297-cardano-cli-show-upstream-config.md`)

## Scope

Phase 1 of the operator-requested `docs/` cleanup ("there is so many
.md files in root some can be deleted and merged together"). Moves
closed/historical documents into `docs/archive/` and the
mis-categorised `poolMetaData.json` operator artifact out of `docs/`
entirely.

Phase 1 is the low-risk archive move; Phase 2 (consolidating the 5
parity docs into a single living `PARITY.md`) is deferred to a
separate round because it requires careful content-merge review.

## Survey before move

```text
docs/ top-level (pre-R298): 23 markdown files + 4 data/config files
  Closed by R287:
    code-audit.md            (917 lines, 3 cross-refs)
    REFACTOR_BLUEPRINT.md    (292 lines, 0 cross-refs)
  Historical (R287 confirmed findings closed):
    AUDIT_VERIFICATION_2026Q2.md  (128 lines, 11 cross-refs)
  Mis-categorised (operator artifact, not a doc):
    poolMetaData.json        (6 lines: stake-pool metadata sample)
```

## Actions

### 1. Created `docs/archive/`

New subdirectory for historical/closed documents preserved as audit
trail. `docs/archive/README.md` is the index, declares
`has_children: true` + `permalink: /archive/` so Jekyll picks it up
as a sub-section under Reference.

Each archived file's front-matter updated:
- `parent: Reference` → `parent: Archive` + `grand_parent: Reference`
- explicit `permalink: /<old-name>/` to preserve the published Jekyll
  URLs (`docs/index.md` and `docs/reference.md` link to
  `/AUDIT_VERIFICATION_2026Q2/` etc.; the explicit permalinks keep
  those links live after the move).

### 2. Moved files via `git mv`

| From | To |
|---|---|
| `docs/code-audit.md` | `docs/archive/code-audit.md` |
| `docs/REFACTOR_BLUEPRINT.md` | `docs/archive/REFACTOR_BLUEPRINT.md` |
| `docs/AUDIT_VERIFICATION_2026Q2.md` | `docs/archive/AUDIT_VERIFICATION_2026Q2.md` |
| `docs/poolMetaData.json` | `node/configuration/poolMetaData.json` |

The `poolMetaData.json` move corrects a long-standing
mis-categorisation — it's a sample stake-pool metadata file
(`name: "WORLDS FIRST RUST FULLNODE"`, `ticker: "RUST"`), not
documentation. Operator artifacts belong under `node/configuration/`.

### 3. Cross-reference updates (10 files)

| File | Updates |
|---|---|
| `AGENTS.md` | 3 `docs/AUDIT_VERIFICATION_2026Q2.md` → `docs/archive/AUDIT_VERIFICATION_2026Q2.md` |
| `CHANGELOG.md` | 2 `docs/code-audit.md` → `docs/archive/code-audit.md` |
| `README.md` | 1 `docs/AUDIT_VERIFICATION_2026Q2.md` |
| `crates/network/AGENTS.md` | 1 `docs/AUDIT_VERIFICATION_2026Q2.md` |
| `docs/ARCHITECTURE.md` | 1 `code-audit.md` |
| `docs/MANUAL_TEST_RUNBOOK.md` | 2 `docs/AUDIT_VERIFICATION_2026Q2.md` (intra-doc) → `archive/AUDIT_VERIFICATION_2026Q2.md` |
| `docs/REAL_PREPROD_POOL_VERIFICATION.md` | 1 `code-audit.md` |
| `node/src/genesis/tests.rs` | 2 `docs/AUDIT_VERIFICATION_2026Q2.md` (Rust comments) |
| `node/src/sync.rs` | 1 `docs/AUDIT_VERIFICATION_2026Q2.md` |
| `node/src/upstream_pins.rs` | 2 `docs/AUDIT_VERIFICATION_2026Q2.md` |

`docs/operational-runs/*.md` are NOT updated per the round-doc
immutability rule. Historical round-docs that referenced the old
paths continue to point at the pre-move locations; readers following
those links will hit a 404 in the rendered site, which is acceptable
because operational-runs are audit-trail records, not navigable
hand-written prose.

## Final state

```text
docs/                                   docs/archive/
├── AGENTS.md                            ├── AUDIT_VERIFICATION_2026Q2.md
├── ARCHITECTURE.md                      ├── README.md (archive index)
├── CHANGELOG.md                         ├── REFACTOR_BLUEPRINT.md
├── CONTRIBUTING.md                      └── code-audit.md
├── DEPENDENCIES.md
├── MANUAL_TEST_RUNBOOK.md               node/configuration/
├── PARITY_PLAN.md                       └── poolMetaData.json (moved here)
├── PARITY_PROOF.md
├── PARITY_SUMMARY.md
├── REAL_PREPROD_POOL_VERIFICATION.md
├── SPECS.md
├── UPSTREAM_PARITY.md
├── UPSTREAM_RESEARCH.md
├── index.md
├── parity-matrix.json
├── reference.md
├── strict-mirror-audit.tsv
├── upstream-haskell-files.txt
├── archive/
├── operational-runs/
├── manual/
├── assets/
├── _includes/
└── _sass/
```

`docs/` top-level shrunk from 23 → 20 markdown files (3 archived).
The Jekyll site continues to publish all archived docs at their
original URLs via explicit `permalink:` front-matter; navigation
restructured under a new "Archive" sub-section of Reference.

## Phase 2 (deferred)

The 5 parity docs still have heavy overlap and are candidates for
consolidation in a separate round:

| File | Lines | Role |
|---|---|---|
| `PARITY_PLAN.md` | 1325 | Original 2026-03-26 planning doc, mostly historical |
| `PARITY_PROOF.md` | 951 | R248 proof report (2026-05-02) |
| `PARITY_SUMMARY.md` | 485 | Management-facing summary (2026-05-05) |
| `UPSTREAM_PARITY.md` | 128 | Upstream parity matrix (table form) |
| `UPSTREAM_RESEARCH.md` | 1211 | Research notes (2026-05-01) |

Consolidating these into a single `PARITY.md` (with sections for
Plan / Summary / Proof / Upstream-table) would cut ~3000 lines of
duplication. **Deferred to a future round** because the merge
requires line-by-line content review to avoid losing unique facts;
not safe to bulk-merge.

`UPSTREAM_RESEARCH.md` (1211 lines of pre-plan research notes) is
also a candidate for archive once the consolidation lands.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 4.49s)
cargo lint                          clean (Finished `dev` profile in 6.65s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

Reference grep across non-archive non-operational-runs paths returns
zero unfixed `code-audit.md` / `AUDIT_VERIFICATION_2026Q2.md` /
`REFACTOR_BLUEPRINT.md` references — every live reference now routes
through `docs/archive/<file>.md`.

## Diff stat

```text
docs/code-audit.md                  -> docs/archive/code-audit.md
                                       (rename + Jekyll front-matter added,
                                        permalink: /code-audit/)
docs/REFACTOR_BLUEPRINT.md          -> docs/archive/REFACTOR_BLUEPRINT.md
                                       (rename + parent: Archive,
                                        permalink: /REFACTOR_BLUEPRINT/)
docs/AUDIT_VERIFICATION_2026Q2.md   -> docs/archive/AUDIT_VERIFICATION_2026Q2.md
                                       (rename + parent: Archive,
                                        permalink: /AUDIT_VERIFICATION_2026Q2/)
docs/poolMetaData.json              -> node/configuration/poolMetaData.json
                                       (operator artifact relocation)
docs/archive/README.md              (new, archive index)

10 living-doc files updated for cross-reference paths.
docs/operational-runs/2026-05-09-round-298-... (new)
```

## Stop point — Phase 1 cleanup complete

| Phase | Scope | Status |
|---|---|---|
| **R298** | **Phase 1 — archive closed docs + relocate poolMetaData.json** | ✅ |
| Phase 2 | Consolidate PARITY_PLAN/PROOF/SUMMARY/UPSTREAM_PARITY/UPSTREAM_RESEARCH | future |

Phase 2 (parity-doc consolidation) deferred until operator
schedules — it requires careful content review to avoid losing
unique facts during the merge.

## References

- Predecessor: R297 (`docs/operational-runs/2026-05-09-round-297-cardano-cli-show-upstream-config.md`)
- Archive index: [`docs/archive/README.md`](../archive/README.md)
- Closed via: R287 closure round (the audit-doc + refactor-blueprint
  re-grade that originally annotated the moved files as `[CLOSED in
  2026-Q3]`).
