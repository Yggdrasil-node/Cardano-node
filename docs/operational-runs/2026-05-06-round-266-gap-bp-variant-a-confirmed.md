## Round 266 — Gap BP step 1: candidate (2) ruled out, candidate (1) partially ruled out

Date: 2026-05-06
Branch: main
Type: Forensic capture + regression pin (no production-code fix this round; queue per-builtin trace comparison for a follow-on round)

### Context

R265 confirmed Gap BP (preview Plutus V2 CEK 306,309 CPU drift on tx
`7bb40e40…3be5b9` at slot ~1,462,057) is still live with R263+R264
applied, and narrowed the remaining root-cause surface to three
candidates:

1. Cost-model parameter loading.
2. `BuiltinSemanticsVariant` mismatch.
3. Per-builtin cost charging.

This round closes candidate **(2)** entirely and the *propagation*
half of candidate **(1)** with runtime evidence. The *parameter-value*
half of candidate (1) — confirming the V2 cost-model array's individual
entries are byte-equal to upstream — is queued together with candidate
**(3)** for a future round that requires an upstream Haskell preview
node synced past slot 1,462,057. Both deferred subjects share the
same operator-time prerequisite (Haskell preview sync), so they
naturally bundle.

### Forensic instrumentation

Added one-line, env-gated diagnostic in `node/src/plutus_eval.rs`
inside `CekPlutusEvaluator::evaluate`, immediately after the cost-model
is built and before the CEK runs:

```rust
if std::env::var_os("YGG_DUMP_PLUTUS_PV").is_some() {
    eprintln!(
        "YGG_DUMP_PLUTUS_PV: tx_hash={} script_hash={} version={:?} \
         pv={:?} propagated={} variant={:?}",
        hex::encode(tx_ctx.tx_hash),
        hex::encode(eval.script_hash),
        eval.version,
        tx_ctx.protocol_version,
        eval.cost_model.is_some(),
        cost_model.builtin_semantics_variant,
    );
}
```

The log captures three diagnostic axes per evaluation:

| Axis | Question answered |
|---|---|
| `propagated` | Was the per-tx cost-model array carried through `PlutusScriptEval.cost_model` (`true`), or did the evaluator fall back to its startup-default cost model (`false`)? |
| `pv` | What protocol version reached `builtin_semantics_variant`? |
| `variant` | What `BuiltinSemanticsVariant` did the constructed `CostModel` actually carry into the CEK? |

The variable is zero-overhead when unset; production sync paths are
unaffected. Kept in tree as forensic infrastructure for the deferred
candidate (3).

### Capture

Resumed sync from R265's checkpoint (preview slot 1,462,041) into a
copy at `/tmp/ygg-r266-preview/db`:

```
YGG_DUMP_PLUTUS_PV=1 target/release/yggdrasil-node run \
    --network preview \
    --database-path /tmp/ygg-r266-preview/db \
    --metrics-port 12468 \
    --socket-path /tmp/ygg-r266-preview/ygg2.sock
```

Captured the Gap BP block and every V2 evaluation up to and including
the failing tx:

```
YGG_DUMP_PLUTUS_PV: tx_hash=7bb40e40c3e6010ead628fd9ea62ae4f8acab340ccca26efacad826caa3be5b9
                   script_hash=86f081bd6de5712f1bd1d8fe8a25fdb8782830522db18550c365d1df
                   version=V2  pv=Some((7, 0))  propagated=true  variant=A
```

The five preceding V2 evaluations in the same block all dispatched
identically (`pv=Some((7, 0)) propagated=true variant=A`), confirming
the dispatch is uniform — the failing tx is not taking a different
branch through the variant selector.

### Conclusions

- **Candidate (1) — cost-model parameter loading.** *Partially* ruled
  out. The **propagation** path is verified: `propagated=true` means
  `PlutusScriptEval.cost_model` was populated by
  `validate_plutus_scripts` at `crates/ledger/src/plutus_validation.rs:1583`
  from the active ledger `protocol_params.cost_models[1]` (the V2 array),
  and `build_plutus_cost_model_from_protocol_values_for_protocol`
  succeeded — no `Err` return, no fallback to the startup default
  whose builtin coverage is sparser. The 175-entry V2 array parses
  cleanly into the named `BTreeMap<String, i64>` via
  `ordered_plutus_v2_param_names()` and lands in
  `CostModel::from_alonzo_genesis_params_with_variant`.

  The **parameter-value-correctness** path is *not* yet verified. If a
  prior on-chain protocol-update transaction was decoded with one
  swapped or off-by-one parameter, every downstream V2 evaluation
  would carry the bad value with `propagated=true` and `variant=A` —
  exactly what the dump shows. Confirming or refuting this requires
  byte-diffing yggdrasil's active V2 array at slot 1,462,057 against
  the upstream Haskell node's same array, which gates on a Haskell
  preview sync past that slot. Queued for the same follow-on round
  as candidate (3).

- **Candidate (2) — `BuiltinSemanticsVariant` mismatch.** Ruled out.
  The active protocol version reaching `builtin_semantics_variant` is
  `Some((7, 0))` (Babbage Vasil — preview at slot 1,462,057 had not yet
  hard-forked to Babbage Valentine PV (8, 0) or Conway PV (9, 0)).
  Variant `A` is the upstream-correct selection for V1/V2 + PV < 9 per
  `PlutusLedgerApi.MachineParameters.machineParametersFor`. The
  constructed `CostModel.builtin_semantics_variant` matches the
  selector's output. The dispatch site is `node/src/genesis.rs:1119`.

- **Candidate (3) — per-builtin cost charging.** Active. The 306,309
  CPU shortfall must come from one specific `(fun, args)` pair where
  `cost_model.builtin_cost(fun, args)` charges a different value than
  upstream's `defaultCostModelParamsForTesting` returns under variant
  A. Yggdrasil's variant-A dispatch in
  `crates/plutus/src/cost_model.rs::from_alonzo_genesis_params_with_variant`
  matches the upstream formula at the type level (e.g. `MultiplyInteger`
  uses `AddedSizes` for variant A, `MultipliedSizes` for B/C — verified
  in source). The remaining surface is either:
  - A specific builtin's `BuiltinCostEntry` is read with the wrong
    parameter slot / sign / minimum, or
  - A specific builtin's runtime semantics (`builtins.rs::evaluate_builtin`)
    consumes one extra step due to a mis-encoded inner CEK term.

### Why the deferred work is queued, not fixed in this round

Two distinct comparisons are still required, both gated on the same
upstream Haskell preview node synced past slot 1,462,057:

- **Cost-array byte-diff (closes candidate (1) value-correctness)** —
  dump yggdrasil's active V2 cost-model array (the 175-entry `Vec<i64>`
  reaching `build_plutus_cost_model_from_protocol_values_for_protocol`
  for tx `7bb40e40…3be5b9`) and byte-diff entry-for-entry against the
  upstream node's same array recovered via
  `cardano-cli query protocol-parameters --testnet-magic 2` against a
  Haskell node at the matching slot.
- **Per-builtin trace diff (closes candidate (3))** — yggdrasil dumps
  every `(fun, args, charged_cost)` triple and diffs against the same
  trace from `db-analyser --repro-mempool-and-forge --target-slot
  1462057 --tx 7bb40e40…3be5b9`.

Both share the same operator-time prerequisite (Haskell preview sync),
so they naturally bundle. Per the plan's per-round approval gate,
queuing them for a focused follow-on round (R266b) keeps the current
round bounded and ships green gates.

### Regression pin

Added unit test
`node/src/genesis/tests.rs::gap_bp_preview_failing_tx_v2_pv7_resolves_variant_a`
which constructs a 175-entry synthetic V2 array, runs it through
`build_plutus_cost_model_from_protocol_values_for_protocol(PlutusVersion::V2,
Some((7, 0)), &v)`, and asserts:

- `model.builtin_semantics_variant == BuiltinSemanticsVariant::A`.
- `model.builtin_costs.get(&MultiplyInteger).cpu` is `CostExpr::AddedSizes`,
  the variant-A shape.

This locks the runtime evidence captured this round so future changes
to `builtin_semantics_variant` cannot silently re-route V2 + PV 7
toward variant B.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 849 passed, 0 failed (+1 vs R265)
```

The new test
`genesis::tests::gap_bp_preview_failing_tx_v2_pv7_resolves_variant_a`
passes cleanly.

### What's functional after this round

Unchanged from R265: preview clean to slot 1,462,057 (Gap BP wall);
preprod clean past slot 607,000; mainnet endurance pending operator-time.

Forensic dumper `YGG_DUMP_PLUTUS_PV` is now available for any future
operator capture without rebuilds.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R263 | shipped | Gap BO closed: Byron-aware TPraos nonce evolution |
| R264 | shipped | Same Byron-prefix epoch-math fix applied to 3 ledger sites (PPUP/MIR/blocks_made) |
| R265 | shipped | Gap BP confirmed live; CEK step-charging path byte-equal to upstream; root-cause search narrowed to 3 candidates |
| **R266** | **this round** | Gap BP step 1: candidate (2) ruled out (variant A correctly selected for V2 + PV (7, 0)); candidate (1) partially ruled out (per-tx cost-model array propagated, but its individual parameter values not yet byte-diffed against upstream). Drift is in either remaining surface: V2 cost-array value correctness, or candidate (3) per-builtin cost charging / runtime builtin semantics. |

### References

- R265 capture: `2026-05-06-round-265-gap-bp-confirmed-fresh-capture.md`
- Forensic dumper: `node/src/plutus_eval.rs::CekPlutusEvaluator::evaluate`
  (gated on `YGG_DUMP_PLUTUS_PV`)
- Variant selector: `node/src/genesis.rs::builtin_semantics_variant`
- Regression test: `node/src/genesis/tests.rs::gap_bp_preview_failing_tx_v2_pv7_resolves_variant_a`
- Upstream variant rule: `PlutusLedgerApi.MachineParameters.machineParametersFor`
  at `.reference-haskell-cardano-node/deps/plutus/plutus-ledger-api/src/PlutusLedgerApi/MachineParameters.hs`
- Captured runtime evidence: `2026-05-06-round-266-gap-bp-variant-a-confirmed.log` (sibling file in this directory)
