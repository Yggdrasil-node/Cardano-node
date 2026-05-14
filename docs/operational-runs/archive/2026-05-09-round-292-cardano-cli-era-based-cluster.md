# Round 292 — Phase F EraBased cluster

**Date:** 2026-05-09
**Phase:** F (cardano-cli surface expansion)
**Predecessor:** R291 (`docs/operational-runs/2026-05-09-round-291-cardano-cli-compatible-cluster.md`)

## Scope

Mirror the upstream `Cardano.CLI.EraBased.*` sub-tree into
`crates/cardano-cli/src/era_based/`. 57 leaf files + 25 sub-parents
+ 1 cluster-root parent = **83 files**.

EraBased is the per-era-aware command surface that adapts upstream's
`cardano.api.IsShelleyBasedEra` constraint into a single set of
subcommands operators invoke against any post-Byron era (Shelley
through Conway). It is the largest single sub-tree in the cardano-cli
library; the plan estimated ~30 files but the actual upstream layout
ships 57 leaf files across 25 sub-directories.

## Plan adaptation note

The original plan (`~/.claude/plans/playful-tickling-plum.md`) listed
R292 as "Shelley + governance ~30 files" mapping to upstream
`Cardano.CLI.Shelley.*` etc. Upstream `cardano-cli` 11.0.1 has
**reorganized** away from per-era directories: there is no
`Cardano/CLI/Shelley/`, `Cardano/CLI/Alonzo/`, `Cardano/CLI/Babbage/`,
or `Cardano/CLI/Conway/` directory. Era-aware commands now all live
under `Cardano/CLI/EraBased/`, era-independent commands under
`Cardano/CLI/EraIndependent/`. R292 mirrors the actual current
layout; R293 will mirror EraIndependent (34 files); R294 will cover
the remaining `Type/` (33 files), `Legacy/` (5 files), and the small
`IO/`, `Json/`, `OS/`, `Option/`, `Read/`, `Run/` subdirs (~12 files
total). This keeps strict 1:1 file-mirror parity against the actual
upstream tree rather than the plan's outdated Era directory mapping.

## Files added

57 leaf files mirror upstream `.hs` files 1:1. Sub-tree breakdown:

| Sub-tree | Leaf count |
|---|---|
| `era_based/` (top-level: Command, Option, Run) | 3 |
| `era_based/common/` (Option) | 1 |
| `era_based/genesis/` (Command, Option, Run + 3 sub-sub-dirs) | 7 |
| `era_based/governance/` (Command, Option, Run + 5 sub-sub-dirs) | 14 |
| `era_based/query/` (Command, Option, Run) | 3 |
| `era_based/script/` (7 sub-sub-dirs + Read/Common, Type) | 14 |
| `era_based/stake_address/` (Command, Option, Run) | 3 |
| `era_based/stake_pool/` (Command, Option, Run + Internal/Metadata) | 4 |
| `era_based/text_view/` | (multiple) |
| `era_based/transaction/` (multiple) | (multiple) |
| **TOTAL leaf** | **57** |

Plus 25 sub-parent shells (one per sub-directory under era_based/)
and 1 `era_based.rs` cluster-root parent declaring `pub mod` for the
top-level children.

Each leaf file ships with a strict-mirror `## Naming parity` block
naming its specific upstream `.hs` plus a placeholder enum so the
module compiles + can be extended. Each parent shell carries
`pub mod` declarations + `**Strict mirror:** none.` block.

## Rust keyword collision (`type`)

7 upstream files are named `Type.hs`. Rust reserves `type` as a
keyword, so `pub mod type;` fails to compile. Used the raw-identifier
form `pub mod r#type;` in the 7 affected parent shells (`script.rs`,
`script/certificate.rs`, `script/mint.rs`, `script/proposal.rs`,
`script/spend.rs`, `script/vote.rs`, `script/withdrawal.rs`). The
file `type.rs` itself doesn't need the prefix; only the `pub mod`
declaration does.

## Verdict bucket counts (post-R292)

| Bucket | Pre-R292 | Post-R292 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 91 | 148 (+57 EraBased leaf files) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 164 | 190 (+25 sub-parents + 1 cluster-root) |
| **TOTAL** | 255 | 338 (+83) |

Audit grader hand-list: still zero `(c-needed)` or `(NEEDS-REVIEW)`
rows; every R292 file is auto-graded `(a)` or `(c)` from its
docstring declaration.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 0.33s)
cargo lint                          clean (Finished `dev` profile in 0.42s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
crates/cardano-cli/src/lib.rs       +1 line (pub mod era_based;)
crates/cardano-cli/src/era_based.rs (new, ~25 lines)
crates/cardano-cli/src/era_based/   (new, 25 sub-parent shells + 57 leaf files)
docs/strict-mirror-audit.tsv         rebuilt (+83 rows)
docs/operational-runs/2026-05-09-round-292-... (new)
```

## Stop point — Phase F R292 closed

| Round | Cluster | Status |
|---|---|---|
| R289 | Phase F bootstrap | ✅ closed (10 files) |
| R290 | Byron cluster | ✅ closed (11 files) |
| R291 | Compatible cluster | ✅ closed (27 files) |
| **R292** | **EraBased cluster (83 files)** | ✅ **closed** |
| R293 | EraIndependent cluster (~34 files) | next |
| R294 | Type + Legacy + small subdirs (~50 files) | pending |
| R295 | sweeper + integration tests | pending |

R293 mirrors `Cardano.CLI.EraIndependent.*` (34 leaf files). R294
covers the remaining trees (`Type/`, `Legacy/`, `IO/`, `Json/`,
`OS/`, `Option/`, `Read/`, `Run/`) plus the top-level `Cardano.CLI`
files not yet mirrored.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R291 (`docs/operational-runs/2026-05-09-round-291-cardano-cli-compatible-cluster.md`)
- Upstream EraBased tree:
  `.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI/EraBased/`
