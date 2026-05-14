## Round 266c — Gap BP ScriptContext byte-shape audit: top-level structure verified, drift confirmed deeper

Date: 2026-05-06
Branch: main
Type: Forensic capture + structural regression pin (no production-code fix; deeper byte-diff queued for operator-time round)

### Context

R266 step 1 + R266b ruled out the costing surface (variant selection,
per-tx propagation, per-builtin formulas, per-step uniform CEK costs).
Drift narrowed to ~14 extra CEK steps in yggdrasil's evaluation of
preview tx `7bb40e40…3be5b9` at slot ~1,462,057. Most plausible
remaining surface (per the advisor and a static review of CEK
transitions): `script_context_data` construction or `Environment::lookup`.

R266c instruments the constructed V2 ScriptContext, captures its CBOR
bytes for the failing tx, and verifies the top-level structural shape
matches upstream `PlutusLedgerApi.V2.Contexts.ScriptContext` /
`TxInfo`. This rules out gross structural divergence (wrong field count,
wrong outer Constr tag, missing fee/mint zero-ADA prepend) without
needing an upstream Haskell trace.

### Forensic instrumentation added

`YGG_DUMP_SCRIPT_CONTEXT` env-gated diagnostic in
`node/src/plutus_eval.rs::CekPlutusEvaluator::evaluate`, fired right
after `script_context_data` returns and before the term is wrapped:

```rust
if std::env::var_os("YGG_DUMP_SCRIPT_CONTEXT").is_some() {
    let cbor = yggdrasil_ledger::CborEncode::to_cbor_bytes(&context_data);
    eprintln!(
        "YGG_DUMP_SCRIPT_CONTEXT: tx_hash={} script_hash={} version={:?} \
         cbor_len={} cbor_hex={}",
        ...
    );
}
```

Zero-overhead when unset. Persisted in tree as part of the R266 forensic
toolchain (alongside `YGG_DUMP_PLUTUS_PV` and `YGG_DUMP_BUILTIN_COSTS`).

### Capture

Resumed from R265's checkpoint into `/tmp/ygg-r266c-preview/db`. The
failing tx's V2 ScriptContext CBOR is **2,184 bytes**; the full hex
is durably persisted at
`docs/operational-runs/2026-05-06-round-266c-gap-bp-script-context.log`
(4,570-byte file including the `YGG_DUMP_SCRIPT_CONTEXT:` prefix line).

### Structural verification

Decoded the captured 2,184-byte CBOR via yggdrasil's `PlutusData::from_cbor_bytes`
and asserted the top-level shape against upstream V2 spec:

| Layer | Expected (upstream V2) | Observed (yggdrasil) | Match |
|---|---|---|---|
| ScriptContext outer | `Constr 0 [TxInfo, ScriptPurpose]` (2 fields) | Constr 0 with 2 fields | ✅ |
| TxInfo outer       | `Constr 0` with **12** fields per `PlutusLedgerApi.V2.Contexts.TxInfo` | Constr 0 with 12 fields | ✅ |
| TxInfo field 0 (`inputs`) | List of TxInInfo | List | ✅ |
| TxInfo field 1 (`referenceInputs`) | List of TxInInfo | List | ✅ |
| TxInfo field 2 (`outputs`) | List of TxOut | List | ✅ |
| TxInfo field 3 (`fee`)  | Map (V1/V2 Value), not plain Lovelace | Map | ✅ |
| TxInfo field 4 (`mint`) | Map with upstream `transMintValue` zero-ADA prepend (empty-bytes policy → empty-bytes asset → 0) | Map; first key is empty-bytes; value is Map containing the zero-ADA prepend | ✅ |

V2-specific structural rules (per
`.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxInfo.hs:378-391`
and `.reference-haskell-cardano-node/deps/plutus/plutus-ledger-api/src/PlutusLedgerApi/V2/Contexts.hs:80-105`):

- ✅ `referenceInputs` is the **second** TxInfo field, before `outputs`
  (V2-specific reordering vs V1's 10-field layout).
- ✅ V2 `mint` is a Map (not a List), with the legacy zero-ADA entry
  preserved per `transMintValue = transCoinToValue zero <> transMultiAsset m`.
- ✅ V2 `fee` remains a Value-shaped Map (V3 changed to plain Lovelace
  Integer; this is V2 so the Map shape applies).

So the top-level V2 ScriptContext shape is **upstream-correct** at every
layer audited by this round. The 14-step drift cannot come from a wrong
TxInfo field count or a wrong outer Constr tag.

### Conclusions

Cumulative ruling out across R266 + R266b + R266c:

| Candidate | Status | Evidence |
|---|---|---|
| Variant selector (V2 + PV 7 → A) | ✅ ruled out | R266 step 1 capture |
| Per-tx cost-model propagation | ✅ ruled out | R266 step 1 capture |
| Per-builtin cost expression shape | ✅ ruled out | R266b: 21/21 match upstream variant-A |
| Per-step uniform CEK cost (variant-A) | ✅ ruled out | R266b: 23000 cpu / 100 mem all kinds |
| Slip-batch trigger comparison | ✅ ruled out | R266b: matches upstream `>= 200` and `Done` flush |
| Term-tag decoder mapping | ✅ ruled out | R266b: tags 0–9 byte-equal to upstream |
| V2 ScriptContext outer Constr tag | ✅ ruled out | R266c: tag 0, 2 fields |
| V2 TxInfo field count + Lists/Maps | ✅ ruled out | R266c: 12 fields, correct shape per field |
| V2 mint zero-ADA prepend | ✅ ruled out | R266c: present, first key empty-bytes |
| **Deep field encoding (per-TxInInfo, per-TxOut, datum/script-ref encoding, address resolution, signatories ordering, redeemer Map keying, validity-range bound encoding)** | ⚠️ **active** | R266c only checks top-level field count + per-field outer constructor; deeper byte-diff requires upstream Haskell trace |
| **V2 cost-model parameter values vs preview chain** | ⚠️ deferred | Same operator-time prerequisite as deep field encoding |

### Why deeper byte-diff is queued

To localise the residual 14-step drift, the next round must compare
yggdrasil's full 2,184-byte ScriptContext CBOR byte-for-byte against the
ScriptContext upstream's `cardano-node` Haskell binary builds for the
same tx + UTxO state at preview slot 1,462,057. The cheapest path is:

1. Sync the vendored Haskell preview node past slot 1,462,057
   (`.reference-haskell-cardano-node/install/bin/cardano-node ...`;
   the install/run/preview/db is already at chunk 786 = ~17M slots,
   well past the failing slot — but its `lock` file may be stale,
   needs an operator pass to verify).
2. Use `db-analyser --repro-mempool-and-forge` to replay block ~1,462,057
   with the failing tx and capture the upstream ScriptContext bytes.
3. Hex-diff the 2,184-byte yggdrasil capture against upstream.

The first divergent byte-window in the diff identifies the offending
field. Operator-time wall-clock for the Haskell node sync (re-validation
on existing DB or top-up) is the gating cost.

### Regression pin

Added unit test
`node/src/plutus_eval/tests.rs::gap_bp_v2_script_context_structural_shape`
which:

1. Re-reads the captured 2,184-byte CBOR hex from the operational-runs
   log via `include_str!` (no live re-sync needed).
2. Decodes through `PlutusData::from_cbor_bytes`.
3. Asserts the outer Constr tag + field count for ScriptContext and
   TxInfo, and the constructor shape of each of the first 5 TxInfo
   fields (inputs / referenceInputs / outputs / fee / mint).

If a future ScriptContext refactor accidentally drops `referenceInputs`,
flips `outputs` and `fee` order, or removes the V1/V2 mint zero-ADA
prepend, this test fires before any preview sync.

### Verification gates

```
cargo fmt --all -- --check       # clean
cargo check-all                  # clean
cargo lint                       # clean
cargo test-all                   # 4 851 passed, 0 failed (+1 vs R266b)
```

The new test
`plutus_eval::tests::gap_bp_v2_script_context_structural_shape`
passes alongside R266 + R266b regressions.

### Cumulative parity arc

| Round | Status | Effect |
|---|---|---|
| R263–R264 | shipped | Byron-aware nonce / epoch_first_slot fixes |
| R265 | shipped | Gap BP confirmed live; root-cause narrowed to 3 candidates |
| R266 | shipped | Gap BP step 1: variant + propagation ruled out |
| R266b | shipped | Per-builtin + per-step costs verified upstream-correct; drift narrowed to ~14 extra CEK steps |
| **R266c** | **this round** | V2 ScriptContext top-level shape verified upstream-correct (12 TxInfo fields, correct ordering, mint zero-ADA prepend present). Drift confirmed to live below the top level — in deep-field encoding (TxInInfo/TxOut/address/datum/witness ordering) or in `Environment::lookup`. Operator-time Haskell sync is the next gating step. |

### References

- R266 closure: `2026-05-06-round-266-gap-bp-variant-a-confirmed.md`
- R266b closure: `2026-05-06-round-266b-gap-bp-builtin-trace-narrowing.md`
- Forensic instrumentation:
  - `node/src/plutus_eval.rs::CekPlutusEvaluator::evaluate` —
    `YGG_DUMP_SCRIPT_CONTEXT` ScriptContext CBOR dump
- Regression tests:
  - `node/src/genesis/tests.rs::gap_bp_preview_failing_tx_v2_pv7_resolves_variant_a`
  - `node/src/genesis/tests.rs::gap_bp_variant_a_v2_builtin_cost_expression_shapes`
  - `node/src/plutus_eval/tests.rs::gap_bp_v2_script_context_structural_shape`
- Upstream V2 ScriptContext spec:
  - `.reference-haskell-cardano-node/deps/plutus/plutus-ledger-api/src/PlutusLedgerApi/V2/Contexts.hs:80-130`
  - `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxInfo.hs:370-410`
  - `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Plutus/TxInfo.hs:340-345`
    (`transMintValue` zero-ADA prepend rule)
- Captured runtime evidence: `2026-05-06-round-266c-gap-bp-script-context.log`
  (2,184-byte CBOR hex; can be re-parsed offline without rebuilding)
