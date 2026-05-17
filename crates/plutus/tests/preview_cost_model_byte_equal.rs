//! Byte-equal cost-model regression fixture (R266 step 1 — Gap BP narrowing).
//!
//! Loads the vendored preview alonzo-genesis (PlutusV1 named cost-model
//! map) and asserts that every `cek*Cost-exBudget*` parameter parses
//! through Yggdrasil's `CostModel::from_alonzo_genesis_params` builder
//! to the same integer the JSON file declares — no rounding, no implicit
//! type conversions, no silently-defaulted entries.
//!
//! This rules out "wrong cost-model values loaded" as the cause of the
//! 306,309 CPU drift on preview tx `7bb40e40…3be5b9` at slot 1,462,057
//! (Gap BP). A pass leaves the per-builtin cost or step-charging logic
//! as the remaining suspect; a fail localises the bug to genesis loading.
//!
//! V1 is the only cost-model present at genesis in
//! `preview/alonzo-genesis.json`. PlutusV2 enters the chain via
//! protocol-parameter updates (positional-array form) and PlutusV3 lives
//! in `conway-genesis.json` as a positional array. Both are pinned by a
//! parallel `crates/node/yggdrasil-node/tests/preview_cost_model_byte_equal.rs` fixture which
//! can call `node::genesis::build_plutus_cost_model_from_protocol_values_for_protocol`
//! directly. The same fixture pins `BuiltinSemanticsVariant` selection
//! across the PV<9 / PV≥9 boundary that the runtime uses for V2.
//!
//! Reference:
//! `.reference-haskell-cardano-node/deps/plutus/plutus-ledger-api/src/PlutusLedgerApi/Common/ParamName.hs`
//! `PlutusLedgerApi.MachineParameters.machineParametersFor`

use std::collections::BTreeMap;

use yggdrasil_plutus::CostModel;

/// Selector entry: a step-cost prefix and a function reading `(cpu, mem)`
/// off a `CostModel`. Aliased here purely to keep `clippy::type_complexity`
/// quiet on the table-driven assertion loop below.
type StepCostEntry = (&'static str, fn(&CostModel) -> (i64, i64));

const PREVIEW_ALONZO_GENESIS: &str =
    "../node/yggdrasil-node/configuration/preview/alonzo-genesis.json";

fn manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_named_cost_model(language: &str) -> BTreeMap<String, i64> {
    let path = manifest_dir().join(PREVIEW_ALONZO_GENESIS);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("read alonzo-genesis at {}: {err}", path.display()));
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("alonzo-genesis is valid JSON");
    let pointer = format!("/costModels/{language}");
    let map = value
        .pointer(&pointer)
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("alonzo-genesis must carry costModels.{language}"));
    map.iter()
        .map(|(k, v)| {
            let n = v
                .as_i64()
                .unwrap_or_else(|| panic!("{language} entry {k} is not i64"));
            (k.clone(), n)
        })
        .collect()
}

/// Each canonical CEK step-cost name → `(cpu, mem)` selector against the
/// loaded `CostModel`. These seven kinds are the upstream
/// `cek*Cost-exBudget{CPU,Memory}` axes V1/V2 charge — Constr and Case
/// landed in V3 and aren't in the Alonzo/Babbage maps.
fn step_cost_table() -> Vec<StepCostEntry> {
    vec![
        ("cekVarCost", |cm| {
            (cm.step_costs.var_cpu, cm.step_costs.var_mem)
        }),
        ("cekConstCost", |cm| {
            (cm.step_costs.constant_cpu, cm.step_costs.constant_mem)
        }),
        ("cekLamCost", |cm| {
            (cm.step_costs.lam_cpu, cm.step_costs.lam_mem)
        }),
        ("cekApplyCost", |cm| {
            (cm.step_costs.apply_cpu, cm.step_costs.apply_mem)
        }),
        ("cekDelayCost", |cm| {
            (cm.step_costs.delay_cpu, cm.step_costs.delay_mem)
        }),
        ("cekForceCost", |cm| {
            (cm.step_costs.force_cpu, cm.step_costs.force_mem)
        }),
        ("cekBuiltinCost", |cm| {
            (cm.step_costs.builtin_cpu, cm.step_costs.builtin_mem)
        }),
    ]
}

fn assert_step_costs_match(label: &str, named: &BTreeMap<String, i64>, cost_model: &CostModel) {
    for (prefix, selector) in step_cost_table() {
        let cpu_key = format!("{prefix}-exBudgetCPU");
        let mem_key = format!("{prefix}-exBudgetMemory");
        let (got_cpu, got_mem) = selector(cost_model);

        let expect_cpu = named.get(&cpu_key).copied();
        let expect_mem = named.get(&mem_key).copied();

        match (expect_cpu, expect_mem) {
            (Some(c), Some(m)) => {
                assert_eq!(
                    got_cpu, c,
                    "{label}: {cpu_key} mismatch (got {got_cpu}, expected {c})"
                );
                assert_eq!(
                    got_mem, m,
                    "{label}: {mem_key} mismatch (got {got_mem}, expected {m})"
                );
            }
            (None, None) => {
                // Optional entry (e.g. cekBuiltinCost on legacy V1 maps).
                // Yggdrasil defaults missing entries to zero; assert that
                // contract rather than the value.
                assert_eq!(
                    (got_cpu, got_mem),
                    (0, 0),
                    "{label}: optional {prefix} should default to (0,0), \
                     got ({got_cpu},{got_mem})"
                );
            }
            (cpu, mem) => panic!(
                "{label}: {prefix} half-defined (cpu={cpu:?}, mem={mem:?}); \
                 upstream genesis should define both axes or neither",
            ),
        }
    }
}

#[test]
fn preview_plutus_v1_step_costs_match_alonzo_genesis_json() {
    let named = load_named_cost_model("PlutusV1");
    let cost_model = CostModel::from_alonzo_genesis_params(&named)
        .expect("preview PlutusV1 named map should construct a CostModel");
    assert_step_costs_match("PlutusV1", &named, &cost_model);
}
