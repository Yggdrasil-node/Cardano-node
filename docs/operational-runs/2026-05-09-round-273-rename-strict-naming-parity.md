# Round 273-rename — strict naming-parity fix-up of R273b–i sub-modules

**Date:** 2026-05-09
**Phase:** γ filename parity — strict mirror correction
**Predecessor:** R273i (`docs/operational-runs/2026-05-09-round-273i-plutus-flat-split.md`)
**Trigger:** Operator review of R273b–i found sub-module filenames that
did not match upstream Haskell `.hs` leaf filenames (Yggdrasil-invented
descriptors like `term.rs`, `default_fun.rs`, `runtime.rs`,
`decoder.rs`, `universe.rs`, `expr.rs`, `memory.rs`, `cert.rs`,
`counter.rs`). Operator directive: "fix them properly. i expext
nothing else then 100% perfection and qode quality" against
CLAUDE.md's "100% protocol parity, 100% naming parity, 100%
functionality parity, 100% filename parity" mandate.

## Scope

Non-destructive corrective pass over R273b–i (eight already-shipped
filename-parity rounds). All work is preserved (no commits reverted)
via `git mv` renames + parent `pub mod` / `pub use` adjustments +
docstring annotations on the residual non-strict-mirror cases.

| Yggdrasil before | Yggdrasil after | Upstream `.hs` leaf the new name mirrors |
|---|---|---|
| `crates/consensus/src/opcert/cert.rs` | `crates/consensus/src/opcert/ocert.rs` | `Cardano/Protocol/TPraos/OCert.hs` |
| `crates/consensus/src/opcert/counter.rs` | `crates/consensus/src/opcert/rules_ocert.rs` | `Cardano/Protocol/TPraos/Rules/OCert.hs` (dir-prefixed to disambiguate from sibling) |
| `crates/plutus/src/types/term.rs` | `crates/plutus/src/types/core_type.rs` | `UntypedPlutusCore/Core/Type.hs` (dir-prefixed) |
| `crates/plutus/src/types/default_fun.rs` | `crates/plutus/src/types/default_builtins.rs` | `PlutusCore/Default/Builtins.hs` (dir-prefixed) |
| `crates/plutus/src/types/runtime.rs` | `crates/plutus/src/types/cek_internal.rs` | `UntypedPlutusCore/Evaluation/Machine/Cek/Internal.hs` (dir-prefixed) |
| `crates/plutus/src/cost_model/expr.rs` | `crates/plutus/src/cost_model/costing_fun.rs` | `PlutusCore/Evaluation/Machine/CostingFun/Core.hs` (dir-prefixed) |
| `crates/plutus/src/cost_model/memory.rs` | `crates/plutus/src/cost_model/ex_memory_usage.rs` | `PlutusCore/Evaluation/Machine/ExMemoryUsage.hs` |
| `crates/plutus/src/flat/decoder.rs` | `crates/plutus/src/flat/instance_flat.rs` | `UntypedPlutusCore/Core/Instance/Flat.hs` (dir-prefixed) |
| `crates/plutus/src/flat/universe.rs` | `crates/plutus/src/flat/default_universe.rs` | `PlutusCore/Default/Universe.hs` (dir-prefixed) |

The dir-prefixing rule (`Default/Builtins.hs` →
`default_builtins.rs`, `Cek/Internal.hs` → `cek_internal.rs`,
`Rules/OCert.hs` → `rules_ocert.rs`) is the canonical Yggdrasil
flattening when the upstream leaf basename collides with a sibling or
is too generic on its own. It preserves traceability (the prefix
recovers the upstream directory context) without introducing made-up
names.

## Files without strict 1:1 upstream mirror

The R273b–i splits also produced sub-modules where no single upstream
`.hs` file is the "right" mirror. These files were NOT renamed; instead
each received an explicit `## Naming parity` block in the module
docstring stating `**Strict mirror:** none.` and naming the upstream
symbol(s) the file surfaces. This honesty signal is what the
strengthened `round-extraction` skill now requires going forward.

| File | Upstream coverage |
|---|---|
| `crates/consensus/src/nonce/derivation.rs` | `Cardano.Ledger.BaseTypes::hashVerifiedVRF` (TPraos) + `Ouroboros.Consensus.Protocol.Praos.VRF::vrfNonceValue` (Praos) — both helpers live inside upstream kitchen-sink modules; no separate `Derivation.hs` exists upstream |
| `crates/consensus/src/nonce/evolution.rs` | `Cardano.Protocol.TPraos.Rules.Updn.hs` (UPDN per-block) + `Cardano.Protocol.TPraos.Rules.Tickn.hs` (TICKN epoch-boundary) — combined here because both rules mutate the same `NonceEvolutionState`. Splitting deferred to R268 naming-parity sweep |
| `crates/consensus/src/mempool/queue/inner.rs` | `Mempool/Impl/Common.hs` + `Mempool/Impl/Update.hs` — combined |
| `crates/consensus/src/mempool/queue/shared.rs` | none — Yggdrasil-side `Arc<RwLock<…>>` wrapper that has no upstream parallel (upstream uses STM `TVar`s embedded inside `Mempool.hs` directly) |
| `crates/consensus/src/mempool/tx_state/state.rs` | `Ouroboros.Network.TxSubmission.Inbound.V2.State.hs` (partial — combines `PeerTxState` + `SharedTxState`) |
| `crates/consensus/src/mempool/tx_state/shared.rs` | none — Yggdrasil-side concurrency wrapper |
| `crates/consensus/src/diffusion_pipelining/identity.rs` | `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining.hs` (partial — `HotIdentity` + per-pool tentative-header tracking) |
| `crates/consensus/src/diffusion_pipelining/state.rs` | `Ouroboros.Consensus.Block.SupportsDiffusionPipelining.hs` + `Ouroboros.Consensus.HardFork.Combinator.Node.DiffusionPipelining.hs` — combined (two upstream files of the same name in different directories) |
| `crates/plutus/src/cost_model/step.rs` | `StepKind` + `StepCosts` live inline in upstream `UntypedPlutusCore/Evaluation/Machine/Cek/Internal.hs` — no separate file. Yggdrasil isolates per-step cost wiring for cohesion |

## Operations performed

### Phase 1: Renames via `git mv` (history-preserving)

```bash
git mv crates/consensus/src/opcert/cert.rs    crates/consensus/src/opcert/ocert.rs
git mv crates/consensus/src/opcert/counter.rs crates/consensus/src/opcert/rules_ocert.rs
git mv crates/plutus/src/types/term.rs        crates/plutus/src/types/core_type.rs
git mv crates/plutus/src/types/default_fun.rs crates/plutus/src/types/default_builtins.rs
git mv crates/plutus/src/types/runtime.rs     crates/plutus/src/types/cek_internal.rs
git mv crates/plutus/src/cost_model/expr.rs   crates/plutus/src/cost_model/costing_fun.rs
git mv crates/plutus/src/cost_model/memory.rs crates/plutus/src/cost_model/ex_memory_usage.rs
git mv crates/plutus/src/flat/decoder.rs      crates/plutus/src/flat/instance_flat.rs
git mv crates/plutus/src/flat/universe.rs     crates/plutus/src/flat/default_universe.rs
```

### Phase 2: Parent-file `pub mod` + `pub use` updates

| Parent | Old | New |
|---|---|---|
| `crates/consensus/src/opcert.rs` | `pub mod cert;` `pub mod counter;` | `pub mod ocert;` `pub mod rules_ocert;` |
| `crates/plutus/src/types.rs` | `pub mod term/default_fun/runtime;` | `pub mod core_type/default_builtins/cek_internal;` |
| `crates/plutus/src/cost_model.rs` | `pub mod expr/memory;` + `use memory::…` | `pub mod costing_fun/ex_memory_usage;` + `use ex_memory_usage::…` |
| `crates/plutus/src/flat.rs` | `pub mod decoder/universe;` + `use decoder::…` | `pub mod instance_flat/default_universe;` + `use instance_flat::…` |

### Phase 3: Sub-module cross-reference updates

| File | Change |
|---|---|
| `crates/plutus/src/types/core_type.rs` | `use super::default_fun::…` → `use super::default_builtins::…` |
| `crates/plutus/src/types/cek_internal.rs` | Two `use super::…` paths updated for renamed siblings |
| `crates/plutus/src/cost_model/tests.rs` | `use crate::cost_model::memory::…` → `use crate::cost_model::ex_memory_usage::…` |
| `crates/plutus/src/flat/instance_flat.rs` | `use super::universe::…` → `use super::default_universe::…` |

### Phase 4: `## Naming parity` docstring blocks

Every sub-module touched by R273b–i (18 files total — 9 strict-mirror
renames + 9 non-strict-mirror cases) now carries a `## Naming parity`
block in its module docstring. For renamed strict-mirror files the
block names the upstream `.hs` file the new name matches and notes the
rename. For non-strict-mirror files the block leads with `**Strict
mirror:** none.` and names the upstream symbol/file(s) the helper
surfaces.

### Phase 5: Skill update

`.claude/skills/round-extraction/SKILL.md` strengthened with explicit
filename-mirror rules (snake_case of upstream leaf basename;
dir-prefix when sibling collisions; mandatory `## Naming parity`
docstring block when no strict mirror exists). New constraint added
to the Stop-conditions block. Future R-arc rounds invoking this skill
will not repeat the R273b–i naming violations.

## Verification gates

```text
cargo fmt --all -- --check        clean
cargo check-all                   clean (Finished `dev` profile in 8.46s)
cargo lint                        clean
cargo test-all                    4855 passed; 0 failed
```

Test count preserved exactly (4,855) — the round is by construction
behavior-preserving (renames + docstrings only).

## Diff stat

```text
.claude/skills/round-extraction/SKILL.md             | 35 ++++++++++++++++++++
crates/consensus/src/diffusion_pipelining/identity.rs|  5 +++
crates/consensus/src/diffusion_pipelining/state.rs   |  7 +++
crates/consensus/src/mempool/queue/inner.rs          |  8 +++
crates/consensus/src/mempool/queue/shared.rs         |  7 +++
crates/consensus/src/mempool/tx_state/shared.rs      |  7 +++
crates/consensus/src/mempool/tx_state/state.rs       |  7 +++
crates/consensus/src/nonce/derivation.rs             |  8 +++
crates/consensus/src/nonce/evolution.rs              | 10 ++++++
crates/consensus/src/opcert.rs                       |  8 +-
crates/consensus/src/opcert/{cert.rs => ocert.rs}    |  6 ++
crates/consensus/src/opcert/{counter.rs => rules_ocert.rs} | 8 +
crates/plutus/src/cost_model.rs                      |  8 +-
crates/plutus/src/cost_model/{expr.rs => costing_fun.rs} | 7 +
crates/plutus/src/cost_model/{memory.rs => ex_memory_usage.rs} | 7 +
crates/plutus/src/cost_model/step.rs                 |  7 +++
crates/plutus/src/cost_model/tests.rs                |  2 +-
crates/plutus/src/flat.rs                            |  6 +-
crates/plutus/src/flat/{universe.rs => default_universe.rs} | 6 +
crates/plutus/src/flat/{decoder.rs => instance_flat.rs} | 10 +-
crates/plutus/src/types.rs                           | 12 +++---
crates/plutus/src/types/{runtime.rs => cek_internal.rs} | 12 ++++-
crates/plutus/src/types/{term.rs => core_type.rs}    |  9 ++-
crates/plutus/src/types/{default_fun.rs => default_builtins.rs} | 6 +
24 files changed, 186 insertions(+), 22 deletions(-)
```

## Cumulative R273 arc state (post-rename)

| Slice | Round | Status | Strict-mirror filenames? |
|---|---|---|---|
| `praos.rs` split | R273a | ✅ shipped | yes (no rename needed) |
| `nonce.rs` split | R273b | ✅ shipped + non-strict cases annotated | partial (`derivation.rs`/`evolution.rs` annotated as Yggdrasil-side) |
| `opcert.rs` split | R273c | ✅ shipped + renamed | yes (`ocert.rs`, `rules_ocert.rs`) |
| `mempool/queue` split | R273d | ✅ shipped + non-strict cases annotated | partial (`inner.rs`/`shared.rs` annotated) |
| `mempool/tx_state` split | R273e | ✅ shipped + non-strict cases annotated | partial (`state.rs`/`shared.rs` annotated) |
| `diffusion_pipelining` split | R273f | ✅ shipped + non-strict cases annotated | partial (`identity.rs`/`state.rs` annotated) |
| `plutus/types` split | R273g | ✅ shipped + renamed | yes (`core_type.rs`, `default_builtins.rs`, `cek_internal.rs`) |
| `plutus/cost_model` split | R273h | ✅ shipped + renamed + non-strict case annotated | partial (`costing_fun.rs`/`ex_memory_usage.rs` strict, `step.rs` annotated) |
| `plutus/flat` split | R273i | ✅ shipped + renamed | yes (`instance_flat.rs`, `default_universe.rs`) |

R273 arc is complete. 9 sub-modules now strictly mirror upstream
`.hs` leaf filenames; 9 retain Yggdrasil-side names with explicit
`## Naming parity` annotations documenting the upstream symbol(s)
they surface. Future drift-guard tests can rely on the docstring
block as the contract.

## Stop point

R273-rename closes the R273 arc cleanly. The next agent-side round is
**R271l — extract `run_governor_loop` → `runtime/governor_loop.rs`**
per the active plan (`~/.claude/plans/playful-tickling-plum.md`). The
strengthened `round-extraction` skill now applies to R271l onwards:
every new sub-module either uses upstream-leaf-snake_case naming or
ships with a `## Naming parity` docstring block.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Strategic envelope: `~/.claude/plans/dapper-giggling-haven.md`
  (R266 → R275)
- Predecessor rounds: R273a `2026-05-07-round-273a-praos-split.md`,
  R273b `2026-05-07-round-273b-nonce-split.md`, R273c
  `2026-05-07-round-273c-opcert-split.md`, R273d
  `2026-05-08-round-273d-mempool-queue-split.md`, R273e
  `2026-05-08-round-273e-mempool-tx-state-split.md`, R273f
  `2026-05-08-round-273f-diffusion-pipelining-split.md`, R273g
  `2026-05-08-round-273g-plutus-types-split.md`, R273h
  `2026-05-08-round-273h-plutus-cost-model-split.md`, R273i
  `2026-05-09-round-273i-plutus-flat-split.md`
- Upstream `.hs` paths cited above all live under
  `.reference-haskell-cardano-node/deps/{plutus,ouroboros-consensus,ouroboros-network,cardano-base,cardano-ledger}/`
