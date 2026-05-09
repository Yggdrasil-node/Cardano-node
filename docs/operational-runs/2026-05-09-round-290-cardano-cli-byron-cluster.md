# Round 290 — Phase F Byron cluster

**Date:** 2026-05-09
**Phase:** F (cardano-cli surface expansion)
**Predecessor:** R289 (`docs/operational-runs/2026-05-09-round-289-phase-f-bootstrap.md`)

## Scope

Mirror the upstream `Cardano.CLI.Byron.*` sub-tree into
`crates/cardano-cli/src/byron/`. 10 sub-modules + 1 parent shell.

## Files added

| Yggdrasil file | Strict mirror |
|---|---|
| `byron.rs` | none (Yggdrasil aggregation shell — upstream has no `Cardano/CLI/Byron.hs` top-level file; Byron surface lives under `Cardano/CLI/Byron/*.hs`) |
| `byron/command.rs` | `Cardano/CLI/Byron/Command.hs` |
| `byron/delegation.rs` | `Cardano/CLI/Byron/Delegation.hs` |
| `byron/genesis.rs` | `Cardano/CLI/Byron/Genesis.hs` |
| `byron/key.rs` | `Cardano/CLI/Byron/Key.hs` |
| `byron/legacy.rs` | `Cardano/CLI/Byron/Legacy.hs` |
| `byron/parser.rs` | `Cardano/CLI/Byron/Parser.hs` |
| `byron/run.rs` | `Cardano/CLI/Byron/Run.hs` |
| `byron/tx.rs` | `Cardano/CLI/Byron/Tx.hs` |
| `byron/update_proposal.rs` | `Cardano/CLI/Byron/UpdateProposal.hs` |
| `byron/vote.rs` | `Cardano/CLI/Byron/Vote.hs` |

Each sub-file ships with a `## Naming parity` block declaring its
strict mirror, plus a single placeholder `<Pascal>Placeholder` enum
so the module compiles + can be extended in subsequent rounds
without breaking the public path. Concrete Byron-era command
implementations port from upstream over subsequent rounds; the R290
deliverable is the strict-mirror filename infrastructure.

The parent `byron.rs` shell carries `pub mod` declarations for all
10 sub-modules + a `## Naming parity: **Strict mirror:** none.` block
documenting that upstream has no `Byron.hs` top-level file.

`crates/cardano-cli/src/lib.rs` adds `pub mod byron;` to expose the
new sub-tree.

## Verdict bucket counts (post-R290)

| Bucket | Pre-R290 | Post-R290 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 60 | 70 (+10 byron sub-files) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 157 | 158 (+1 byron parent) |
| **TOTAL** | 217 | 228 (+11) |

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 0.30s)
cargo lint                          clean (Finished `dev` profile in 0.37s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
crates/cardano-cli/src/lib.rs        +1 line (pub mod byron;)
crates/cardano-cli/src/byron.rs      (new, 22 lines)
crates/cardano-cli/src/byron/command.rs        (new, 14 lines)
crates/cardano-cli/src/byron/delegation.rs     (new, 14 lines)
crates/cardano-cli/src/byron/genesis.rs        (new, 14 lines)
crates/cardano-cli/src/byron/key.rs            (new, 14 lines)
crates/cardano-cli/src/byron/legacy.rs         (new, 14 lines)
crates/cardano-cli/src/byron/parser.rs         (new, 14 lines)
crates/cardano-cli/src/byron/run.rs            (new, 14 lines)
crates/cardano-cli/src/byron/tx.rs             (new, 14 lines)
crates/cardano-cli/src/byron/update_proposal.rs (new, 14 lines)
crates/cardano-cli/src/byron/vote.rs           (new, 14 lines)
docs/strict-mirror-audit.tsv         rebuilt (+11 rows)
docs/operational-runs/2026-05-09-round-290-... (new)
```

## Stop point — Phase F R290 closed

| Round | Cluster | Status |
|---|---|---|
| R289 | Phase F bootstrap | ✅ closed |
| **R290** | **Byron cluster (10 + 1 = 11 files)** | ✅ **closed** |
| R291 | Compatible cluster | next |
| R292 | Shelley + governance | pending |
| R293 | Alonzo + Babbage | pending |
| R294 | Conway | pending |
| R295 | sweeper + integration tests | pending |

R291 starts populating the Compatible sub-tree at
`crates/cardano-cli/src/compatible/{command,exception,governance,
json,option,read,run,stake_address,stake_pool,transaction}.rs`
mirroring upstream `cardano-cli/src/Cardano/CLI/Compatible/*.hs`.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R289 (`docs/operational-runs/2026-05-09-round-289-phase-f-bootstrap.md`)
- Upstream Byron tree:
  `.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI/Byron/`
