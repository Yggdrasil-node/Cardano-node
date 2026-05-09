# Round 291 — Phase F Compatible cluster

**Date:** 2026-05-09
**Phase:** F (cardano-cli surface expansion)
**Predecessor:** R290 (`docs/operational-runs/2026-05-09-round-290-cardano-cli-byron-cluster.md`)

## Scope

Mirror the upstream `Cardano.CLI.Compatible.*` sub-tree into
`crates/cardano-cli/src/compatible/`. 21 leaf files + 6 parent
shells = 27 files total.

Compatible is the era-shared command surface that adapts upstream's
per-era types into a single set of subcommands the operator invokes
the same way across Byron / Shelley / Allegra / Mary / Alonzo /
Babbage / Conway.

## Files added

### Top-level (5 leaf + 1 cluster parent)

| Yggdrasil file | Strict mirror |
|---|---|
| `compatible.rs` | none (Yggdrasil cluster parent shell) |
| `compatible/command.rs` | `Cardano/CLI/Compatible/Command.hs` |
| `compatible/exception.rs` | `Cardano/CLI/Compatible/Exception.hs` |
| `compatible/option.rs` | `Cardano/CLI/Compatible/Option.hs` |
| `compatible/read.rs` | `Cardano/CLI/Compatible/Read.hs` |
| `compatible/run.rs` | `Cardano/CLI/Compatible/Run.hs` |

### Governance sub-tree (4 leaf + 1 sub-parent)

| Yggdrasil file | Strict mirror |
|---|---|
| `compatible/governance.rs` | none (sub-parent) |
| `compatible/governance/command.rs` | `Cardano/CLI/Compatible/Governance/Command.hs` |
| `compatible/governance/option.rs` | `Cardano/CLI/Compatible/Governance/Option.hs` |
| `compatible/governance/run.rs` | `Cardano/CLI/Compatible/Governance/Run.hs` |
| `compatible/governance/types.rs` | `Cardano/CLI/Compatible/Governance/Types.hs` |

### Json sub-tree (1 leaf + 1 sub-parent)

| Yggdrasil file | Strict mirror |
|---|---|
| `compatible/json.rs` | none (sub-parent) |
| `compatible/json/friendly.rs` | `Cardano/CLI/Compatible/Json/Friendly.hs` |

### StakeAddress sub-tree (3 leaf + 1 sub-parent)

| Yggdrasil file | Strict mirror |
|---|---|
| `compatible/stake_address.rs` | none (sub-parent) |
| `compatible/stake_address/command.rs` | `Cardano/CLI/Compatible/StakeAddress/Command.hs` |
| `compatible/stake_address/option.rs` | `Cardano/CLI/Compatible/StakeAddress/Option.hs` |
| `compatible/stake_address/run.rs` | `Cardano/CLI/Compatible/StakeAddress/Run.hs` |

### StakePool sub-tree (3 leaf + 1 sub-parent)

| Yggdrasil file | Strict mirror |
|---|---|
| `compatible/stake_pool.rs` | none (sub-parent) |
| `compatible/stake_pool/command.rs` | `Cardano/CLI/Compatible/StakePool/Command.hs` |
| `compatible/stake_pool/option.rs` | `Cardano/CLI/Compatible/StakePool/Option.hs` |
| `compatible/stake_pool/run.rs` | `Cardano/CLI/Compatible/StakePool/Run.hs` |

### Transaction sub-tree (5 leaf + 1 sub-parent)

| Yggdrasil file | Strict mirror |
|---|---|
| `compatible/transaction.rs` | none (sub-parent) |
| `compatible/transaction/command.rs` | `Cardano/CLI/Compatible/Transaction/Command.hs` |
| `compatible/transaction/option.rs` | `Cardano/CLI/Compatible/Transaction/Option.hs` |
| `compatible/transaction/run.rs` | `Cardano/CLI/Compatible/Transaction/Run.hs` |
| `compatible/transaction/script_witness.rs` | `Cardano/CLI/Compatible/Transaction/ScriptWitness.hs` |
| `compatible/transaction/tx_out.rs` | `Cardano/CLI/Compatible/Transaction/TxOut.hs` |

Each leaf file ships with a strict-mirror `## Naming parity` block +
a placeholder enum so the module compiles + can be extended in
subsequent rounds. Each parent shell carries `pub mod` declarations +
`**Strict mirror:** none.` documenting the sub-tree-aggregation role.

`crates/cardano-cli/src/lib.rs` adds `pub mod compatible;`.

## Verdict bucket counts (post-R291)

| Bucket | Pre-R291 | Post-R291 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 70 | 91 (+21 leaf files) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 158 | 164 (+6 parent shells) |
| **TOTAL** | 228 | 255 (+27) |

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 0.48s)
cargo lint                          clean (Finished `dev` profile in 0.32s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py --fail-on-violation
                                    strict-mirror: 0 violations (clean), exit 0
python3 scripts/check-parity-matrix.py
                                    parity matrix clean: 8 entries validated
```

## Diff stat

```text
crates/cardano-cli/src/lib.rs       +1 line (pub mod compatible;)
crates/cardano-cli/src/compatible/  (new, 6 sub-parent shells + 21 leaf files)
docs/strict-mirror-audit.tsv         rebuilt (+27 rows)
docs/operational-runs/2026-05-09-round-291-... (new)
```

## Stop point — Phase F R291 closed

| Round | Cluster | Status |
|---|---|---|
| R289 | Phase F bootstrap | ✅ closed |
| R290 | Byron cluster | ✅ closed (11 files) |
| **R291** | **Compatible cluster (27 files)** | ✅ **closed** |
| R292 | Shelley + governance | next |
| R293 | Alonzo + Babbage | pending |
| R294 | Conway | pending |
| R295 | sweeper + integration tests | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R290 (`docs/operational-runs/2026-05-09-round-290-cardano-cli-byron-cluster.md`)
- Upstream Compatible tree:
  `.reference-haskell-cardano-node/deps/cardano-cli/cardano-cli/src/Cardano/CLI/Compatible/`
