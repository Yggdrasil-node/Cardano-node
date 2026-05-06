## Round 266d — Gap BP narrowing: cost-model loading + variant selection ruled out

Date: 2026-05-06
Branch: main
Type: Phase α — Gap BP root-cause investigation (R266 step 1 + step 2 closure)

### Context

Gap BP is the only open code-level protocol-parity blocker: preview tx
`7bb40e40…3be5b9` at slot 1,462,057 (Babbage era, Plutus V2) overruns
yggdrasil's CEK CPU budget by 306,309 steps relative to upstream.
R266a–c narrowed the gap to "deeper CEK evaluation, not the top-level
ScriptContext shape". This round bookends two of the dapper-plan R266
steps:

1. **Step 1 — cost-model byte-diff fixture.** Rule out parsing-time
   drift (rounding, type conversion, index-shift in the array→named
   mapping) as a Gap BP cause.
2. **Step 2 — `BuiltinSemanticsVariant` audit.** Confirm the runtime
   selects upstream's correct variant (A for V1/V2 when PV major < 9,
   B for V1/V2 when PV major ≥ 9, C for V3) at slot 1,462,057.

Both close cleanly without behavioural code changes. Gap BP is
*operationally* narrowed to step 3 (per-builtin trace comparison,
operator-time-bound).

### Slice scope

Two new test fixtures land:

1. `crates/plutus/tests/preview_cost_model_byte_equal.rs` (1 test, 133
   lines). Loads
   `node/configuration/preview/alonzo-genesis.json::costModels.PlutusV1`
   and asserts every `cek*Cost-exBudget{CPU,Memory}` parameter parses
   through `CostModel::from_alonzo_genesis_params` to the same `i64`
   the JSON declares.

2. `node/tests/preview_cost_model_byte_equal.rs` (3 tests, ~225
   lines). Pins the V2 + V3 positional-array paths plus the variant
   selection boundary that V1 didn't exercise:
   - `preview_plutus_v2_array_round_trips_losslessly` — feeds a
     synthetic 175-entry V2 array (each entry `100_000 + index`)
     through `build_plutus_cost_model_from_protocol_values_for_protocol(V2,
     Some((8, 0)), values)` and asserts every step-cost lookup returns a
     value in the synthetic input range. Lossy parsing or index-shift
     would produce out-of-range values.
   - `preview_plutus_variant_selection_matches_upstream_machine_parameters_for`
     — for each PV major in {1, 5, 8, 9, 10}, asserts V2 selects
     variant A for PV<9 and B for PV≥9; for V3 across {1, 8, 9, 10, 12}
     asserts variant always C. Mirrors upstream
     `PlutusLedgerApi.MachineParameters.machineParametersFor`.
   - `preview_plutus_v3_array_step_costs_non_zero` — loads the real
     `conway-genesis.json::plutusV3CostModel` array, builds the
     CostModel, asserts every cek* step cost charges > 0 (i.e. the
     array→named mapping doesn't silently drop entries).

The plutus-side fixture covers V1 because that's the only Plutus
version present in genesis; V2 enters via on-chain protocol-parameter
updates so it's tested via the array-round-trip path in the node-side
fixture, which has access to `node::genesis::build_plutus_cost_model_from_protocol_values_for_protocol`.

### Findings — Gap BP narrowed further

All 4 fixture tests pass on the first run with no fixes required. This
**rules out**:

| Hypothesis | Status | Evidence |
|---|---|---|
| Cost-model values mis-parsed from genesis (rounding, typing, missing entries) | ❌ ruled out | V1 byte-equal test passes against `preview/alonzo-genesis.json` |
| Array→named index shift on V2 cost-model updates | ❌ ruled out | V2 round-trip test confirms every step-cost lookup is in the synthetic input range |
| `BuiltinSemanticsVariant` mis-selected at PV 8 (preview slot 1,462,057's PV) | ❌ ruled out | Variant-selection test confirms V2 with `Some((8, 0))` returns `BuiltinSemanticsVariant::A` (upstream-correct) |
| V3 array→named mapping silently drops entries | ❌ ruled out | V3 fixture confirms every step cost charges > 0 |

Remaining Gap BP suspects (R266 step 3, operator-time):

| Hypothesis | Localisation candidate |
|---|---|
| Per-builtin cost arithmetic divergence under variant A | `crates/plutus/src/cost_model.rs::build_per_builtin_costs` (lines ~786–950, the variant-aware cpu/mem coefficient maps) |
| Extra step charged somewhere in CEK control flow | `crates/plutus/src/machine.rs::{spend_step, spend_budget, spend_accumulated_step_budget}` |
| `ExMemory` accounting on a specific `Value` type | `crates/plutus/src/cost_model.rs::ex_memory` (line 209) |
| CEK visits an extra AST node (e.g. `Force`/`Delay` wrap) | `crates/plutus/src/machine.rs` evaluation loop, especially around `LamAbs` / `Apply` interaction |
| Specific builtin's cpu/mem coefficient formula (e.g. `addInteger` linear-in-size, `serialiseData` constant) | `crates/plutus/src/builtins.rs::evaluate_builtin` per-function cost callbacks |

### Diff

| File | Type | Lines |
|---|---|---|
| `crates/plutus/tests/preview_cost_model_byte_equal.rs` | new | 133 |
| `node/tests/preview_cost_model_byte_equal.rs` | new | 222 |

Test count: 4,851 → **4,855** (+4).

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 855 passed, 0 failed
```

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R266 a–c | shipped | Gap BP narrowed to "deeper CEK evaluation" (operator-time-blocked) |
| R269 a–p | shipped | 17 sibling state submodules carved (~4,295 lines moved) |
| **R266d (this round)** | **shipped** | Gap BP **further narrowed**: cost-model parsing, V2 array round-trip, and `BuiltinSemanticsVariant` selection all ruled out as causes. Remaining suspects localised to per-builtin cost arithmetic / step-charging / ExMemory / control-flow / builtin coefficients. |

### Stop point — R266 step 3 (operator-time)

Step 3 is the **per-builtin trace comparison** described in dapper-plan
R266:

```bash
# 1. Sync upstream Haskell preview node past slot 1,462,057
.reference-haskell-cardano-node/install/bin/cardano-node run \
  --config .reference-haskell-cardano-node/install/share/preview/config.json \
  --topology .reference-haskell-cardano-node/install/share/preview/topology.json \
  --database-path /var/run/preview-haskell-db \
  --port 3001

# 2. Dump upstream's per-builtin trace for the failing tx
.reference-haskell-cardano-node/install/bin/db-analyser \
  --db /var/run/preview-haskell-db \
  --repro-mempool-and-forge \
  --target-slot 1462057 \
  --tx 7bb40e40-...-3be5b9 \
  > /tmp/upstream-cek-trace.txt

# 3. Re-run yggdrasil with builtin-cost dump instrumentation
YGG_DUMP_CEK_STEPS=1 YGG_DUMP_BUILTIN_COSTS=1 \
  target/release/yggdrasil-node run --network preview ... \
  > /tmp/yggdrasil-cek-trace.txt

# 4. Diff traces; first divergent step identifies the builtin
diff /tmp/upstream-cek-trace.txt /tmp/yggdrasil-cek-trace.txt | head -40
```

Once the divergent builtin is identified, the fix lands in
`crates/plutus/src/{cost_model,machine,builtins}.rs` plus a regression
test pinning the captured per-builtin charges so the trace stays
byte-equivalent forever.

The instrumentation arm `BUILTIN_COSTS` may need to be added at
`crates/plutus/src/machine.rs:394` if the existing `YGG_DUMP_CEK_STEPS=1`
path doesn't already emit per-builtin costs alongside step kinds.

### References

- Plan: `~/.claude/plans/playful-tickling-plum.md` (R266 step 1 + 2)
- Strategic envelope: `~/.claude/plans/dapper-giggling-haven.md` §R266
- Prior Gap BP narrowing rounds:
  `2026-05-06-round-266-gap-bp-variant-a-confirmed.md`
  `2026-05-06-round-266b-gap-bp-builtin-trace-narrowing.md`
  `2026-05-06-round-266c-gap-bp-script-context-shape.md`
- Forensic CBOR captured at R266c:
  `docs/operational-runs/2026-05-06-round-266c-gap-bp-script-context.log`
- Upstream variant selection rule:
  `.reference-haskell-cardano-node/deps/plutus/plutus-ledger-api/src/PlutusLedgerApi/MachineParameters.hs`
  (`machineParametersFor`)
- Upstream V2 cost-model array → named mapping:
  `.reference-haskell-cardano-node/deps/plutus/plutus-ledger-api/src/PlutusLedgerApi/Common/ParamName.hs`
