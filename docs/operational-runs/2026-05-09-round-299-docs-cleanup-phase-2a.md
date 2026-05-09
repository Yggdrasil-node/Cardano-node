# Round 299 — `docs/` cleanup phase 2A (archive parity-plan + upstream-research)

**Date:** 2026-05-09
**Phase:** Documentation hygiene (operator-requested cleanup, continuation)
**Predecessor:** R298 (`docs/operational-runs/2026-05-09-round-298-docs-cleanup-phase-1.md`)

## Scope

Phase 2A of the operator-requested `docs/` cleanup. Archives the two
historical parity docs identified by the consolidation survey:

- `docs/PARITY_PLAN.md` (1325 lines, 2026-03-26 pre-execution
  planning doc; the roadmap, dependency graph, and risk matrix are
  superseded by R269–R298 execution).
- `docs/UPSTREAM_RESEARCH.md` (1211 lines, 2026-05-01 pre-plan
  Haskell research; superseded by `docs/ARCHITECTURE.md` +
  `docs/PARITY_PROOF.md` + `docs/strict-mirror-audit.tsv`).

Phase 2B (trimming `PARITY_SUMMARY.md` to ~200 lines by removing
the 2026-03-26 planning artifacts in §3–7) is deferred to a separate
round because the section-by-section review needs operator input
on what to drop vs preserve.

## Survey-driven decisions

The consolidation survey (an Explore agent walked all 5 parity docs)
produced the following recommendations:

| File | Verdict | Action |
|---|---|---|
| `PARITY_PROOF.md` | KEEP — canonical operational verification reference | unchanged |
| `UPSTREAM_PARITY.md` | KEEP — drift-guard pin matrix (atomic CI artifact) | unchanged |
| `PARITY_SUMMARY.md` | TRIM — keep §1, §2, §8; drop §3–7 (stale planning artifacts) | **deferred to Phase 2B** |
| `PARITY_PLAN.md` | ARCHIVE — original 2026-03 planning doc, superseded | **archived in R299** |
| `UPSTREAM_RESEARCH.md` | ARCHIVE — pre-plan research notes, superseded | **archived in R299** |

R299 ships ARCHIVE actions (low-risk, reversible). Phase 2B's TRIM
action requires line-by-line content review; deferred.

## Actions

### 1. `git mv` archive moves

```text
docs/PARITY_PLAN.md         -> docs/archive/PARITY_PLAN.md
docs/UPSTREAM_RESEARCH.md   -> docs/archive/UPSTREAM_RESEARCH.md
```

### 2. Jekyll front-matter restructure

Each archived doc's front-matter updated to:
- `parent: Reference` -> `parent: Archive` + `grand_parent: Reference`
- explicit `permalink: /<file-stem>/` to preserve the published URL
- a leading blockquote pointing readers at the live successors

The R298 `docs/archive/README.md` index is updated with new entries
for both archived files, naming the closing round (R299) and the
live-successor docs.

### 3. Cross-reference updates (8 files)

| File | Updates |
|---|---|
| `AGENTS.md` | `docs/PARITY_PLAN.md` -> `docs/archive/PARITY_PLAN.md` |
| `CLAUDE.md` | `docs/PARITY_PLAN.md` reference (one site) |
| `crates/consensus/src/genesis_density.rs` | inline Rust comment ref |
| `crates/network/src/blockfetch_pool.rs` | inline Rust comment ref |
| `node/src/sync.rs` | inline Rust comment ref |
| `docs/AGENTS.md` | local relative ref |
| `docs/PARITY_PROOF.md` | local relative ref |
| `docs/PARITY_SUMMARY.md` | local relative ref |
| `docs/UPSTREAM_PARITY.md` | absolute ref `docs/PARITY_PLAN.md` -> `archive/PARITY_PLAN.md` |
| `docs/MANUAL_TEST_RUNBOOK.md` | absolute ref `docs/PARITY_PLAN.md` -> `archive/PARITY_PLAN.md` |

`docs/operational-runs/*.md` are NOT updated per the round-doc
immutability rule.

## Final `docs/` inventory

```text
docs/                                   docs/archive/
├── AGENTS.md                            ├── AUDIT_VERIFICATION_2026Q2.md (R298)
├── ARCHITECTURE.md                      ├── PARITY_PLAN.md            (R299)
├── CHANGELOG.md                         ├── README.md
├── CONTRIBUTING.md                      ├── REFACTOR_BLUEPRINT.md     (R298)
├── DEPENDENCIES.md                      ├── UPSTREAM_RESEARCH.md      (R299)
├── MANUAL_TEST_RUNBOOK.md               └── code-audit.md             (R298)
├── PARITY_PROOF.md                      
├── PARITY_SUMMARY.md                    
├── REAL_PREPROD_POOL_VERIFICATION.md    
├── SPECS.md                             
├── UPSTREAM_PARITY.md                   
├── index.md                             
├── reference.md                         
├── parity-matrix.json                   
├── strict-mirror-audit.tsv              
├── upstream-haskell-files.txt           
├── archive/                             
├── operational-runs/                    
├── manual/                              
├── assets/, _includes/, _sass/          
└── Gemfile                              
```

`docs/` top-level markdown count: 23 → 11 (over R298 + R299; -52%).
The 11 remaining files split into clear roles:

| Role | Files |
|---|---|
| Living parity surface | `PARITY_PROOF.md`, `PARITY_SUMMARY.md`, `UPSTREAM_PARITY.md` (3) |
| Architecture / specs | `ARCHITECTURE.md`, `SPECS.md`, `DEPENDENCIES.md` (3) |
| Operator + contributor | `MANUAL_TEST_RUNBOOK.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `REAL_PREPROD_POOL_VERIFICATION.md` (4) |
| Workspace + Jekyll | `AGENTS.md`, `index.md`, `reference.md` + Jekyll site files (3) |

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 4.76s)
cargo lint                          clean (Finished `dev` profile in 8.78s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

Reference grep confirms zero unfixed paths to PARITY_PLAN.md or
UPSTREAM_RESEARCH.md across non-archive non-operational-runs files.

## Diff stat

```text
docs/PARITY_PLAN.md                 -> docs/archive/PARITY_PLAN.md
                                       (rename + Jekyll front-matter,
                                        permalink: /PARITY_PLAN/,
                                        leading archive notice)
docs/UPSTREAM_RESEARCH.md           -> docs/archive/UPSTREAM_RESEARCH.md
                                       (rename + Jekyll front-matter,
                                        permalink: /UPSTREAM_RESEARCH/,
                                        leading archive notice)
docs/archive/README.md              +2 rows (PARITY_PLAN, UPSTREAM_RESEARCH)
10 files updated for cross-reference paths
docs/operational-runs/2026-05-09-round-299-... (new)
```

## Phase 2B (still deferred)

**`PARITY_SUMMARY.md` trim** — drop §3 Implementation Dependencies,
§4 Key Risks & Mitigations, §5 Deliverables by Phase, §6 Success
Criteria, §7 Why This Plan (5 sections, ~280 lines combined). Keep:
- §1 Current Implementation Status table (live, update per round)
- §2 Quick Function Inventory (genuinely useful)
- §8 Parity Audit History (cumulative record)

The trim drops planning-doc artifacts that duplicate `PARITY_PLAN.md`
(now archived) and the per-section content review needs operator
input on what to absorb into other live docs (e.g. risks could move
into `UPSTREAM_PARITY.md::Open Gaps`).

## Stop point — Phase 2A complete

| Phase | Scope | Status |
|---|---|---|
| R298 | Phase 1 — archive R287-closed docs + relocate poolMetaData.json | ✅ |
| **R299** | **Phase 2A — archive PARITY_PLAN + UPSTREAM_RESEARCH** | ✅ |
| Phase 2B | Trim `PARITY_SUMMARY.md` (drop §3–7) | future |

Phase 2B awaits operator scheduling — the section trim removes
~280 lines of stale planning content but each removal is a
content-review decision better made with operator review than
auto-applied.

## References

- Predecessor: R298 (`docs/operational-runs/2026-05-09-round-298-docs-cleanup-phase-1.md`)
- Survey result: Explore-agent consolidation report (recommended ARCHIVE
  for PARITY_PLAN + UPSTREAM_RESEARCH; KEEP for PARITY_PROOF + UPSTREAM_PARITY;
  TRIM for PARITY_SUMMARY)
- Archive index: [`docs/archive/README.md`](../archive/README.md)
