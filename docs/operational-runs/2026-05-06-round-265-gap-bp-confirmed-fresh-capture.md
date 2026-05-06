## Round 265 — Gap BP confirmed against R263+R264 build (preview Plutus V2 budget overrun)

Date: 2026-05-06
Branch: main
Type: Forensic capture (no code change this round; queue Gap BP for focused future round)

### Context

R263+R264 closed the Byron-prefix epoch-math drift bug class (preprod
TPraos VRF + ledger PPUP/MIR/blocks_made first-slot computations).
Preview was unaffected by R263/R264 (Shelley-only, no Byron prefix).
This round confirms whether the orthogonal **Gap BP** (preview
Plutus V2 CEK budget overrun) still fires after the parity fixes.

### Fresh sync

```
target/release/yggdrasil-node run --network preview \
    --database-path /tmp/ygg-r265-preview/db \
    --metrics-port 12466
```

Sustained progress through Byron + early Shelley + Allegra + Mary
+ Alonzo. Reached slot 1,400,745 cleanly, resumed sync past prior
checkpoint, hit Gap BP at slot ~1,462,057 with the **same forensic
shape as the original session-plan capture**:

```
[Error] Node.Sync error=ledger decode error: Phase-2 validation tag mismatch:
        block says true, re-evaluation says false:
        script 86f081bd6de5712f1bd1d8fe8a25fdb8782830522db18550c365d1df failed:
        purpose Spending { tx_id: 202f45d627131b8c4d6659a62cd9f92e7ac6183dfe165a8eda8795e0e2b2ed68, index: 0 },
        version V2,
        tx 7bb40e40c3e6010ead628fd9ea62ae4f8acab340ccca26efacad826caa3be5b9,
        ex_units(mem=6121408, steps=1657962006):
        out of budget: 200 accumulated steps:
            cost cpu=4600000, mem=20000,
            before cpu=4293691, mem=67426,
            after  cpu=-306309, mem=47426
```

Decoded:

- **Slot**: ~1,462,057 (same as original)
- **Tx**: `7bb40e40c3e6010ead628fd9ea62ae4f8acab340ccca26efacad826caa3be5b9`
- **Spending input**: `tx_id=202f45d627131b8c4d6659a62cd9f92e7ac6183dfe165a8eda8795e0e2b2ed68, index=0`
- **Script hash**: `86f081bd6de5712f1bd1d8fe8a25fdb8782830522db18550c365d1df`
- **Plutus version**: V2
- **Block-claimed budget**: `mem=6,121,408 / cpu=1,657,962,006`
- **Yggdrasil's actual usage**: cpu=1,658,268,315 (= claimed + 306,309)
- **Memory**: stays in budget (47,426 remaining at fail point)
- **Failure mode**: Phase-2 validation tag mismatch — the block claims
  the script returned `true`, but yggdrasil's CEK runs out of budget
  before reaching `true`.

### Static analysis (this round)

Compared yggdrasil's `step_compute` per-StepKind charging against
upstream `Cardano.UPlc.Machine.Cek.Internal::computeCek`:

| Term type | Upstream charge | Yggdrasil charge | Match? |
|---|---|---|---|
| `Var`        | `BVar`     | `StepKind::Var`     | ✅ |
| `Constant`   | `BConst`   | `StepKind::Constant`| ✅ |
| `LamAbs`     | `BLamAbs`  | `StepKind::LamAbs`  | ✅ |
| `Apply`      | `BApply`   | `StepKind::Apply`   | ✅ |
| `Delay`      | `BDelay`   | `StepKind::Delay`   | ✅ |
| `Force`      | `BForce`   | `StepKind::Force`   | ✅ |
| `Builtin`    | `BBuiltin` | `StepKind::Builtin` | ✅ |
| `Constr`     | `BConstr`  | `StepKind::Constr`  | ✅ (V3+) |
| `Case`       | `BCase`    | `StepKind::Case`    | ✅ (V3+) |

Compared `returnCek` upstream vs `step_return` yggdrasil — neither
charges per-StepKind. Compared `Frame::ApplyArg` charging — neither
side charges. Compared step-charging *placement*: same (charge
once on encountering the term, before pushing the frame).

So the per-StepKind step-charging path is byte-identical between
yggdrasil and upstream. The 306,309 CPU shortfall is NOT in the
step-cost path itself.

### Remaining surface for Gap BP

The 306,309 CPU drift between yggdrasil and upstream for tx
`7bb40e40…3be5b9`'s CEK execution must come from one of:

1. **Cost-model parameter loading** — if any of the
   `cek{Var,Const,LamAbs,Apply,Delay,Force,Builtin,Constr,Case}Cost-exBudget{CPU,Memory}`
   genesis parameters lands at a slightly wrong value in yggdrasil's
   `StepCosts`, every step of that kind charges with the wrong
   cost. With ~72K total step charges, a 4 CPU/step error would
   produce the observed drift.
2. **Builtin cost charging** — `cost_model.builtin_cost(fun, args)`
   may compute a different cost for one specific `(fun, args)` pair
   than upstream `defaultCostModelParamsForTesting`. ~13 builtin
   invocations with average ~23K CPU each ≈ 306K shortfall.
3. **Builtin semantics variant mismatch** — yggdrasil's
   `BuiltinSemanticsVariant::B` flag may not match the
   tx-protocol-version-correct upstream value, causing a builtin
   to evaluate to a different shape (different cost dispatch).

### Why this is queued, not fixed in this round

Each candidate above requires one of:

- **Per-builtin trace comparison** against upstream's
  `db-analyser --repro-mempool-and-forge` output for the same
  tx — captures every builtin's `(fun, args, cost)` triple in
  upstream's CEK and compares against yggdrasil's
  `YGG_DUMP_CEK_STEPS`-instrumented trace. This needs a Haskell
  preview node synced past slot 1,462,057, which takes hours.
- **Cost-model parameter byte-diff** — load the same
  `conway-genesis.json` cost model JSON in both yggdrasil and
  upstream; assert the resulting in-memory cost map is byte-equal.
  Bounded but tedious; needs a fixture test against the actual
  preview cost model (not the synthetic fixtures already pinned).

Both are hours-to-days of focused work, not in scope for this
turn given the broader user directive to "proceed parity work and
development." This round confirms Gap BP is still live, captures
the failure shape with the latest builds (R263+R264 active), and
documents the 3 remaining root-cause candidates for the next
focused R-round.

### Cumulative parity arc (this session)

| Round | Status | Effect |
|---|---|---|
| R258 | shipped | Multi-peer fetch default `1→2` (67% throughput delta on rich-topology operators) |
| R259 | shipped | TPraos active-overlay VRF diagnostic enrichment (slot/era/nonce in trace) |
| R260 | shipped | `tools/cddl-codegen` removed; CDDL is documentation only |
| R261 | shipped | R253 sub-candidate narrowing (3→2): VRF crypto + JSON-parse ruled out |
| R262 | shipped | R253 forensic capture (preprod slot 432000 fail) → narrowed to nonce evolution |
| R263 | shipped | R253 closed: Byron-aware TPraos nonce evolution; preprod sync past slot 432000 |
| R264 | shipped | Same Byron-prefix epoch-math fix audit: 3 ledger sites (PPUP/MIR/blocks_made) fixed |
| **R265** | **this round** | Gap BP confirmed live; CEK step-charging path byte-equal to upstream; root-cause search narrowed to cost-model loading or builtin cost charging |

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 848 passed, 0 failed (unchanged from R264)
```

### What's functional after this session

Yggdrasil syncs:

- **Preview**: cleanly to slot ~1,462,057 (= where Gap BP blocks).
- **Preprod**: cleanly past slot ~607,000 (= where R262 wall used
  to stop sync; verified post-R263). Multi-day sustained sync
  remains to be verified by an operator-time rehearsal.
- **Mainnet**: same R263+R264 fixes apply uniformly; mainnet has
  Byron→Shelley at slot 4,492,800 / epoch 208 with the same bug
  surface. No mainnet sync has been run this session; verification
  pending operator-time rehearsal.

### Next bounded targets

In rough priority order:

- **Gap BP root-cause** (R266 candidate): the three candidates above,
  best attacked with `YGG_DUMP_CEK_STEPS` + a parallel Haskell
  preview sync.
- **Mainnet rehearsal** (operator-time): R263+R264 are uniform fixes;
  validate against mainnet from genesis.
- **Long-tail naming/filename parity** (per-`AGENTS.md` directive):
  align yggdrasil module structure with upstream `Cardano.*` /
  `Ouroboros.*` Haskell module hierarchy. Bounded refactor work
  per-area.
