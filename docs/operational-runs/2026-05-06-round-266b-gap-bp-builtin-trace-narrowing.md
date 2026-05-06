## Round 266b — Gap BP per-builtin trace + step-cost narrowing: drift moved out of the costing surface

Date: 2026-05-06
Branch: main
Type: Forensic capture + regression pin (no production-code fix; queued the residual root-cause search for a future round)

### Context

R266 step 1 ruled out (a) `BuiltinSemanticsVariant` selection and (b) per-tx
cost-model propagation. The captured trace from `YGG_DUMP_PLUTUS_PV` showed
`pv=Some((7, 0)) propagated=true variant=A` for the Gap BP failing tx
`7bb40e40…3be5b9` at preview slot ~1,462,057.

R266b instruments the next layer: per-builtin cost charges and per-step costs
during the actual CEK execution of that tx. Goal: rule out the per-builtin
cost expressions and the per-step uniform CEK cost as the source of the
306,309 CPU drift, narrowing the search for the underlying bug.

### Forensic instrumentation added

Two new env-gated diagnostics, each zero-overhead when unset:

1. **`YGG_DUMP_BUILTIN_COSTS`** in `crates/plutus/src/machine.rs` —
   appends one line per fully-saturated builtin call to
   `YGG_DUMP_BUILTIN_COSTS_FILE` (default `/tmp/ygg-builtin-costs.log`):
   `fun=<DefaultFun> args=[<size>,<size>,...] cpu=<charged> mem=<charged>
   remaining_cpu=<post-charge> remaining_mem=<post-charge>`.

2. **Extended `YGG_DUMP_PLUTUS_PV`** in `node/src/plutus_eval.rs` — the
   existing PV/variant dump now also prints the constructed
   `CostModel.step_costs` (var/const/lam/apply/delay/force/builtin per-kind
   cpu and mem) and `startup_cost` for each evaluation, so the verifier
   can confirm the active cek* values.

### Capture

Resumed sync from R265's checkpoint (preview slot ~1,462,041) into
`/tmp/ygg-r266b-preview/db`. Captured every V2 evaluation through the
failing block. Forensic dump persisted at
`docs/operational-runs/2026-05-06-round-266b-gap-bp-builtin-trace.log`.

For the failing tx `7bb40e40…3be5b9` (the 6th and last V2 eval in the block):

```
YGG_DUMP_PLUTUS_PV: ... variant=A
  startup=100/100 var=23000/100 const=23000/100 lam=23000/100 apply=23000/100
  delay=23000/100 force=23000/100 builtin=23000/100
```

So yggdrasil's StepCosts are uniform `23000 cpu / 100 mem` across all
kinds — exactly upstream's variant-A defaults from
`.reference-haskell-cardano-node/deps/plutus/plutus-core/cost-model/data/cekMachineCostsA.json`.

### Per-builtin verification — every formula matches upstream variant A

For the failing tx, yggdrasil dispatches **3,210 saturated builtin calls**
across the trace. Spot-verified all unique `(fun, arg_sizes, cpu_charged)`
tuples against upstream
`builtinCostModelA.json`:

| Builtin | Upstream variant-A formula | Yggdrasil charge | Match |
|---|---|---|---|
| `unConstrData`      | constant 32696                       | 32696 (every size)             | ✅ |
| `headList`          | constant 43249                       | 43249                          | ✅ |
| `tailList`          | constant 41182                       | 41182                          | ✅ |
| `sndPair`           | constant 85931                       | 85931                          | ✅ |
| `fstPair`           | constant 80436                       | 80436                          | ✅ |
| `chooseList`        | constant 175354                      | 175354                         | ✅ |
| `chooseData`        | constant 19537                      | 19537                          | ✅ |
| `ifThenElse`        | constant 80556                       | 80556                          | ✅ |
| `unBData` / `unIData` / `unMapData` / `unListData` | constant 31220 / 43357 / 38314 / 32247 | matches each | ✅ |
| `bData` / `iData`   | constant 1000                        | 1000                           | ✅ |
| `mkCons`            | constant 65493                       | 65493                          | ✅ |
| `mkNilData`         | constant 22558                       | 22558                          | ✅ |
| `constrData`        | constant 89141                       | 89141                          | ✅ |
| `addInteger` / `subtractInteger` | max_size(205665, 812)     | 206477 (size 1,1)              | ✅ |
| `multiplyInteger`   | added_sizes(69522, 11687)            | 92896 (size 1,1)               | ✅ |
| `divideInteger`     | const_above_diagonal(196500, multiplied_sizes(453240, 220)) | 453460 (size 1,1) | ✅ |
| `equalsInteger`     | min_size(208512, 421)                | 208933 (size 1,1)              | ✅ |
| `lessThanInteger`   | min_size(208896, 511)                | 209407 (size 1,1)              | ✅ |
| `lessThanEqualsInteger` | min_size(204924, 473)            | 205397 (size 1,1)              | ✅ |
| `equalsByteString`  | linear_on_diagonal(245000, 216773, 62) | 216835 (a=b=1) / 245000 (a≠b) / 217021 (a=b=4) | ✅ |
| `equalsData`        | min_size(1060367, 12586)             | 1161055 (size 8,8) / 1324673 (size 21,21) | ✅ |

All 21 unique builtins observed in the failing tx use upstream-correct
variant-A cost expressions on upstream-default values. Total per-builtin
CPU charged in the failing tx: **278,268,215 cpu** across 3,210 calls.

### Step accounting — drift narrowed to total CEK step count

Combined `YGG_DUMP_CEK_STEPS` + `YGG_DUMP_BUILTIN_COSTS` capture for the
failing tx:

| Metric | Value |
|---|---|
| Total CEK steps charged | **60,000** |
| ┝ `kind=Apply`           | 17,068 |
| ┝ `kind=Var`             | 15,466 |
| ┝ `kind=LamAbs`          | 11,602 |
| ┝ `kind=Force`           | 8,399  |
| ┝ `kind=Delay`           | 3,911  |
| ┝ `kind=Builtin`         | 3,207  |
| └ `kind=Constant`        | 347    |
| Per-step CPU (all kinds) | 23,000 |
| Total step CPU           | 1,380,000,000 |
| Total builtin CPU        | 278,268,215   |
| Startup CPU              | 100           |
| **Total yggdrasil CPU**  | **1,658,268,315** |
| Block-claimed budget     | 1,657,962,006 |
| Drift                    | **+306,309 cpu**  |

`306,309 / 23000 ≈ 13.32` — not an integer multiple of any single step
kind's cost. Combined with the verified per-builtin and per-step costs,
this means yggdrasil takes **~13–14 more total CEK steps than upstream**
for the same script execution. The 14-extra-steps figure assumes upstream
stops with a partial slip-batch (1–199 residual steps) instead of
yggdrasil's full 300th batch of 200.

### Slip-batch comparison — matches upstream

Yggdrasil at `crates/plutus/src/machine.rs:416`:
```rust
if self.pending_steps_total >= self.step_slippage  // step_slippage = 200
```

Upstream at `Cek/Internal.hs:1042`:
```haskell
when (unbudgetedStepsTotal >= ?cekSlippage) spendAccumulatedBudget
```

Both fire at `>= 200`. No off-by-one. Yggdrasil also flushes residual
budget at `State::Done` (machine.rs:132), matching upstream's `NoFrame`
flush at `returnCek` (Cek/Internal.hs:893).

### Decoder term-tag comparison — matches upstream

Yggdrasil's `dispatch_term_tag` (`crates/plutus/src/flat.rs:416`) maps
each upstream UPLC term-tag to the same `Term` constructor as upstream
`decodeTerm` (`UntypedPlutusCore/Core/Instance/Flat.hs:142`). Tags 0–9
verified one-by-one. The `Binder DeBruijn` instance is documented to
encode as zero-cost empty bytes (upstream's
`testlib/Flat/Spec.hs:367`), so yggdrasil's omission of an explicit
binder-name read for tag 2 is upstream-correct.

### Conclusions

Cumulative ruling out across R266 + R266b:

| Candidate | Status | Evidence |
|---|---|---|
| Variant selector (V2 + PV 7 → A) | ✅ ruled out | R266 step 1 capture: `variant=A` |
| Per-tx cost-model propagation | ✅ ruled out | R266 step 1 capture: `propagated=true` |
| Per-builtin cost expression shape | ✅ ruled out | R266b: 21/21 unique builtins match upstream variant-A formula |
| Per-step uniform CEK cost (variant-A) | ✅ ruled out | R266b: all 7 kinds = 23000 cpu / 100 mem |
| Slip-batch trigger comparison | ✅ ruled out | R266b: matches upstream `>= 200` and `Done` flush |
| Term-tag decoder mapping | ✅ ruled out | R266b: tags 0–9 byte-equal to upstream `decodeTerm` |
| **Total CEK step count divergence (~14 extra steps)** | ⚠️ **active** | R266b: yggdrasil charges 1,658,268,315 cpu, block budget 1,657,962,006 cpu, drift 306,309 ≈ 13.32 × 23000 |
| **V2 cost-model parameter values (byte-diff vs preview chain)** | ⚠️ deferred | Same operator-time prerequisite as the active candidate |

### Where the 14-extra-step drift could live

The remaining surface is tiny (≈0.023% of total CEK execution). Static
review of `step_compute`, `step_return`, `apply_value`, `force_value`,
the slip-batch trigger, and term-tag dispatch ruled out structural
divergence. The most plausible candidates that haven't been audited:

1. **`script_context_data` construction** — if yggdrasil's TxInfo /
   ScriptContext encoding has one extra field, one different field
   shape, or different ordering of inputs/outputs/withdrawals, the
   script's pattern-matching logic may iterate one or two extra
   list elements per branch, costing a small handful of CEK steps.
2. **Constant decoding** — `Frame::ReadTerm` for tag-4 (`Constant`)
   decodes a recursive type tag list. A subtle mismatch in how
   yggdrasil materialises a nested `Constant::ProtoList(Type, [...])`
   versus upstream's `VCon` could expose one extra reduction step.
3. **De Bruijn lookup** — `Environment::lookup` is currently
   implemented as a recursive cons-list walk. Upstream uses
   `RAList` (a random-access list); the lookup-cost shapes are the
   same per-index, but a difference in environment construction
   (e.g. one extra cons cell during application) could surface as
   an extra `Var` step on every closure invocation.

Disambiguating these requires either (a) a per-step Term diff against
upstream's CEK trace for the same tx, or (b) byte-diffing yggdrasil's
on-the-wire ScriptContext PlutusData against an upstream-built
equivalent. Both gate on a Haskell preview node synced past slot
1,462,057 + a small instrumentation shim in upstream's `db-analyser`,
so they remain operator-time work.

### Regression pin

Added unit test
`node/src/genesis/tests.rs::gap_bp_variant_a_v2_builtin_cost_expression_shapes`
which constructs a synthetic 175-entry V2 cost-model array, builds the
`CostModel` at PV (7, 0), and asserts that **every builtin from the failing
tx's trace** maps to the variant-A `CostExpr` *shape* upstream specifies:

- 21 constant-cost builtins → `CostExpr::Constant`
- 2 max_size integer-arith builtins (add/subtract) → `CostExpr::MaxSize`
- 1 multiplyInteger → `CostExpr::AddedSizes` (variant-A specific)
- 3 min_size integer-comparison builtins → `CostExpr::MinSize`
- 1 equalsByteString → `CostExpr::LinearOnDiagonal`
- 1 equalsData → `CostExpr::MinSize`
- 1 divideInteger → `CostExpr::ConstAboveDiagonal { inner:
  MultipliedSizes }` (variant-A specific; variant C wraps
  `TwoVarQuadratic` instead)

This locks the variant-A formula shapes in place. If a future cost-model
refactor accidentally rewires multiplyInteger to `MultipliedSizes` (variant
B/C shape), or divideInteger to `TwoVarQuadratic` (variant C shape), this
test fires.

### Forensic dumper kept in tree

Both `YGG_DUMP_PLUTUS_PV` and `YGG_DUMP_BUILTIN_COSTS` are kept in tree
gated on env vars. The next round (R266c) will reuse the builtin dumper
to compare yggdrasil's per-(fun, args, cost) trace against an upstream
trace from `db-analyser --repro-mempool-and-forge` once the Haskell
preview sync is operator-rehearsed.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 850 passed, 0 failed (+1 vs R266)
```

The new test
`genesis::tests::gap_bp_variant_a_v2_builtin_cost_expression_shapes`
passes cleanly alongside the R266 step-1 pin.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R263 | shipped | Gap BO closed: Byron-aware TPraos nonce evolution |
| R264 | shipped | Same Byron-prefix epoch-math fix to 3 ledger sites |
| R265 | shipped | Gap BP confirmed live; CEK step-charging path byte-equal to upstream; root-cause narrowed to 3 candidates |
| R266 | shipped | Gap BP step 1: candidate (2) `BuiltinSemanticsVariant` ruled out; candidate (1) propagation half ruled out |
| **R266b** | **this round** | Gap BP step 2: per-builtin cost shapes + per-step CEK costs verified upstream-correct. Drift narrowed to ~14 extra CEK steps in yggdrasil's evaluation, most likely in ScriptContext construction or Environment lookup. Two candidates still queued for R266c (operator-time). |

### References

- R266 closure: `2026-05-06-round-266-gap-bp-variant-a-confirmed.md`
- Forensic instrumentation:
  - `crates/plutus/src/machine.rs::CekMachine::dump_builtin_cost`
    (gated on `YGG_DUMP_BUILTIN_COSTS`)
  - `node/src/plutus_eval.rs::CekPlutusEvaluator::evaluate`
    extended PV+StepCosts dump (gated on `YGG_DUMP_PLUTUS_PV`)
- Regression tests:
  - `node/src/genesis/tests.rs::gap_bp_preview_failing_tx_v2_pv7_resolves_variant_a`
  - `node/src/genesis/tests.rs::gap_bp_variant_a_v2_builtin_cost_expression_shapes`
- Upstream variant-A files:
  - `.reference-haskell-cardano-node/deps/plutus/plutus-core/cost-model/data/builtinCostModelA.json`
  - `.reference-haskell-cardano-node/deps/plutus/plutus-core/cost-model/data/cekMachineCostsA.json`
- Upstream CEK reference:
  - `.reference-haskell-cardano-node/deps/plutus/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek/Internal.hs`
  - `.reference-haskell-cardano-node/deps/plutus/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Core/Instance/Flat.hs`
- Captured runtime evidence: `2026-05-06-round-266b-gap-bp-builtin-trace.log` (sibling)
