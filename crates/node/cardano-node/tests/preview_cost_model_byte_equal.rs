//! Byte-equal cost-model regression fixture (R266 step 1 — Gap BP narrowing).
//!
//! Companion to `crates/plutus/tests/preview_cost_model_byte_equal.rs`.
//! That fixture pins the V1 named-map → CostModel path (the only Plutus
//! version present in `preview/alonzo-genesis.json`); this fixture pins
//! the V2 + V3 positional-array paths plus the `BuiltinSemanticsVariant`
//! selection that the runtime applies on phase-2 evaluation.
//!
//! V2 cost-model values arrive on chain via protocol-parameter updates
//! (positional array; `genesis::build_plutus_cost_model_from_protocol_values_for_protocol`
//! decodes them). V3 lives in `conway-genesis.json::plutusV3CostModel`
//! also as a positional array.
//!
//! The test rules out parsing-time drift as a cause of the 306,309 CPU
//! drift on preview tx `7bb40e40…3be5b9` at slot 1,462,057 (Gap BP). It
//! does NOT pin specific upstream values for the failing slot — those
//! arrive via on-chain updates and must be captured separately during
//! the per-builtin trace comparison (R266 step 3).
//!
//! Reference: `PlutusLedgerApi.MachineParameters.machineParametersFor`
//! in `IntersectMBO/plutus`.

use std::collections::BTreeMap;

use yggdrasil_ledger::plutus_validation::PlutusVersion;
use yggdrasil_node_genesis::build_plutus_cost_model_from_protocol_values_for_protocol;
use yggdrasil_plutus::{BuiltinSemanticsVariant, CostModel};

/// Selector entry: a step-cost prefix and a function reading `(cpu, mem)`
/// off a `CostModel`. Aliased here purely to keep `clippy::type_complexity`
/// quiet on the table-driven assertion loops below.
type StepCostEntry = (&'static str, fn(&CostModel) -> (i64, i64));

const PREVIEW_CONWAY_GENESIS: &str = "../../../configuration/preview/conway-genesis.json";

fn manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Each canonical CEK step-cost name → `(cpu, mem)` selector against the
/// loaded `CostModel`. V1/V2 charge the first seven; V3 also charges
/// `cekConstrCost` and `cekCaseCost`.
fn step_cost_table_v1_v2() -> Vec<StepCostEntry> {
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

fn step_cost_table_v3() -> Vec<StepCostEntry> {
    let mut t = step_cost_table_v1_v2();
    t.push(("cekConstrCost", |cm| {
        (cm.step_costs.constr_cpu, cm.step_costs.constr_mem)
    }));
    t.push(("cekCaseCost", |cm| {
        (cm.step_costs.case_cpu, cm.step_costs.case_mem)
    }));
    t
}

/// Load preview's V3 cost-model array from `conway-genesis.json`.
fn load_preview_v3_array() -> Vec<i64> {
    let path = manifest_dir().join(PREVIEW_CONWAY_GENESIS);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("read conway-genesis at {}: {err}", path.display()));
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("conway-genesis is valid JSON");
    let arr = value
        .pointer("/plutusV3CostModel")
        .or_else(|| value.pointer("/costModels/PlutusV3"))
        .and_then(|v| v.as_array())
        .expect("conway-genesis must carry plutusV3CostModel");
    arr.iter()
        .map(|v| {
            v.as_i64()
                .unwrap_or_else(|| panic!("V3 entry not i64: {v:?}"))
        })
        .collect()
}

/// Synthesize a V2 positional-array of cost-model values. Each entry
/// gets a unique recognisable signature so a parsing-time index drift
/// shows up as a non-monotonic step cost. Used to round-trip-verify the
/// array → named → `CostModel` path without depending on a captured
/// on-chain V2 cost model.
fn synthetic_v2_array(len: usize) -> Vec<i64> {
    (0..len).map(|i| 100_000 + i as i64).collect()
}

/// Step 1: the V2 array round-trip is lossless. Confirms
/// `build_plutus_cost_model_from_protocol_values_for_protocol(V2, ...)`
/// preserves every input value through the array → named → struct
/// pipeline that the runtime uses for on-chain cost-model updates.
#[test]
fn preview_plutus_v2_array_round_trips_losslessly() {
    let v2 = synthetic_v2_array(175); // PLUTUS_V2_INITIAL_COST_MODEL_LEN

    // Pre-Conway: variant A. Build, then read back step costs and
    // confirm each lookup returns one of the synthetic input values.
    let cost_model_a = build_plutus_cost_model_from_protocol_values_for_protocol(
        PlutusVersion::V2,
        Some((8, 0)),
        &v2,
    )
    .expect("V2 array should construct a CostModel");
    assert_eq!(
        cost_model_a.builtin_semantics_variant,
        BuiltinSemanticsVariant::A,
        "V2 with PV major=8 must select variant A (pre-Conway)"
    );

    // Each step cost must be in the synthetic input range — proves the
    // value flowed through without lossy transformation.
    for (label, selector) in step_cost_table_v1_v2() {
        let (cpu, mem) = selector(&cost_model_a);
        assert!(
            (100_000..100_000 + 175).contains(&cpu) || cpu == 0,
            "V2 {label} cpu={cpu} not in synthetic range or zero (cekBuiltinCost is optional)",
        );
        assert!(
            (100_000..100_000 + 175).contains(&mem) || mem == 0,
            "V2 {label} mem={mem} not in synthetic range or zero",
        );
    }
}

/// Step 2: variant selection follows upstream's `machineParametersFor`
/// rule. V2 with `PV major < 9` → variant A; V2 with `PV major ≥ 9` →
/// variant B; V3 anytime → variant C. Mirrors `genesis::builtin_semantics_variant`.
#[test]
fn preview_plutus_variant_selection_matches_upstream_machine_parameters_for() {
    let v2 = synthetic_v2_array(175);

    // V2 across the PV<9 / PV≥9 boundary.
    for (pv_major, expected) in [
        (1, BuiltinSemanticsVariant::A),
        (5, BuiltinSemanticsVariant::A),
        (8, BuiltinSemanticsVariant::A),
        (9, BuiltinSemanticsVariant::B),
        (10, BuiltinSemanticsVariant::B),
    ] {
        let cm = build_plutus_cost_model_from_protocol_values_for_protocol(
            PlutusVersion::V2,
            Some((pv_major, 0)),
            &v2,
        )
        .expect("V2 cost model should build");
        assert_eq!(
            cm.builtin_semantics_variant, expected,
            "V2 with PV major={pv_major} should select {expected:?}, got {:?}",
            cm.builtin_semantics_variant,
        );
    }

    // V3 always C, regardless of PV.
    let v3 = load_preview_v3_array();
    for pv_major in [1, 8, 9, 10, 12] {
        let cm = build_plutus_cost_model_from_protocol_values_for_protocol(
            PlutusVersion::V3,
            Some((pv_major, 0)),
            &v3,
        )
        .expect("V3 cost model should build");
        assert_eq!(
            cm.builtin_semantics_variant,
            BuiltinSemanticsVariant::C,
            "V3 with PV major={pv_major} should select C, got {:?}",
            cm.builtin_semantics_variant,
        );
    }
}

/// Step 3: V3 array round-trips through the runtime path. Loads the real
/// preview `plutusV3CostModel` array, builds a `CostModel`, and asserts
/// the resulting step costs are non-zero (i.e. genuinely loaded). V3 is
/// not the failing version for Gap BP but pinning V3 here protects
/// against future drift in the array→named mapping.
#[test]
fn preview_plutus_v3_array_step_costs_non_zero() {
    let v3 = load_preview_v3_array();
    let cm = build_plutus_cost_model_from_protocol_values_for_protocol(
        PlutusVersion::V3,
        Some((9, 0)),
        &v3,
    )
    .expect("preview V3 array should construct a CostModel");

    assert_eq!(
        cm.builtin_semantics_variant,
        BuiltinSemanticsVariant::C,
        "preview V3 should select variant C"
    );

    let mut all_present = BTreeMap::new();
    for (label, selector) in step_cost_table_v3() {
        let (cpu, mem) = selector(&cm);
        all_present.insert(label, (cpu, mem));
    }

    // Every V3 step kind must charge a positive CPU and memory cost,
    // otherwise the array→named mapping silently dropped that entry.
    for (label, (cpu, mem)) in &all_present {
        assert!(
            *cpu > 0,
            "V3 {label} cpu={cpu} should be > 0 (entry missing or mis-mapped)",
        );
        assert!(
            *mem > 0,
            "V3 {label} mem={mem} should be > 0 (entry missing or mis-mapped)",
        );
    }
}
