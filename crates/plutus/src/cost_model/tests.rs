// Tests for `crate::cost_model`. Extracted from the inline `#[cfg(test)]
// mod tests` block in R256 Phase H to keep `cost_model.rs` itself under
// 2 KLOC. `use super::*;` still gives us full access to the parent
// module's items because Rust treats this as the same `tests` submodule
// regardless of where the source file physically lives.

use super::*;

// ── CostModelError Display-content tests ───────────────────────────
//
// Both variants carry operator-facing diagnostic fields (parameter name
// + offending value). Without content tests, a refactor that drops the
// `{name}` or `{value}` placeholder from `#[error(...)]` would strip the
// actionable field from cost-model-construction failures, leaving
// operators to guess which cost-model entry is wrong.

#[test]
fn display_cost_model_missing_parameter_names_parameter() {
    let e = CostModelError::MissingParameter("verifyEd25519Signature-cpu-arguments");
    let s = format!("{e}");
    assert!(s.contains("missing"), "rule name: {s}");
    assert!(
        s.contains("verifyEd25519Signature-cpu-arguments"),
        "must name the missing parameter: {s}",
    );
}

#[test]
fn display_cost_model_negative_parameter_names_field_and_value() {
    let e = CostModelError::NegativeParameter {
        name: "addInteger-cpu-arguments-intercept",
        value: -1_234,
    };
    let s = format!("{e}");
    assert!(s.to_lowercase().contains("negative"), "rule name: {s}");
    assert!(
        s.contains("addInteger-cpu-arguments-intercept"),
        "must name the parameter: {s}",
    );
    assert!(
        s.contains("-1234") || s.contains("-1_234"),
        "must show value: {s}"
    );
}

fn sample_params() -> BTreeMap<String, i64> {
    BTreeMap::from([
        // Machine step costs
        ("cekVarCost-exBudgetCPU".to_owned(), 29_773),
        ("cekConstCost-exBudgetCPU".to_owned(), 29_773),
        ("cekLamCost-exBudgetCPU".to_owned(), 29_773),
        ("cekDelayCost-exBudgetCPU".to_owned(), 29_773),
        ("cekForceCost-exBudgetCPU".to_owned(), 29_773),
        ("cekApplyCost-exBudgetCPU".to_owned(), 29_773),
        ("cekVarCost-exBudgetMemory".to_owned(), 100),
        ("cekConstCost-exBudgetMemory".to_owned(), 100),
        ("cekLamCost-exBudgetMemory".to_owned(), 100),
        ("cekDelayCost-exBudgetMemory".to_owned(), 100),
        ("cekForceCost-exBudgetMemory".to_owned(), 100),
        ("cekApplyCost-exBudgetMemory".to_owned(), 100),
        ("cekBuiltinCost-exBudgetCPU".to_owned(), 29_773),
        ("cekBuiltinCost-exBudgetMemory".to_owned(), 100),
        ("cekStartupCost-exBudgetCPU".to_owned(), 100),
        ("cekStartupCost-exBudgetMemory".to_owned(), 100),
        ("cekConstrCost-exBudgetCPU".to_owned(), 30_001),
        ("cekConstrCost-exBudgetMemory".to_owned(), 101),
        ("cekCaseCost-exBudgetCPU".to_owned(), 30_002),
        ("cekCaseCost-exBudgetMemory".to_owned(), 102),
        // addInteger — MaxSize, slope=0 (effectively constant per arg)
        ("addInteger-cpu-arguments-intercept".to_owned(), 197_209),
        ("addInteger-cpu-arguments-slope".to_owned(), 0),
        ("addInteger-memory-arguments-intercept".to_owned(), 1),
        ("addInteger-memory-arguments-slope".to_owned(), 1),
        // sha2_256 — LinearInX
        ("sha2_256-cpu-arguments-intercept".to_owned(), 2_477_736),
        ("sha2_256-cpu-arguments-slope".to_owned(), 29_175),
        ("sha2_256-memory-arguments".to_owned(), 4),
        // multiplyInteger — MultipliedSizes for CPU, AddedSizes for memory
        ("multiplyInteger-cpu-arguments-intercept".to_owned(), 61_516),
        ("multiplyInteger-cpu-arguments-slope".to_owned(), 11_218),
        ("multiplyInteger-memory-arguments-intercept".to_owned(), 0),
        ("multiplyInteger-memory-arguments-slope".to_owned(), 1),
        // ifThenElse — constant
        ("ifThenElse-cpu-arguments".to_owned(), 1),
        ("ifThenElse-memory-arguments".to_owned(), 1),
        // verifyEd25519Signature — LinearInY
        (
            "verifyEd25519Signature-cpu-arguments-intercept".to_owned(),
            5_000,
        ),
        ("verifyEd25519Signature-cpu-arguments-slope".to_owned(), 10),
        ("verifyEd25519Signature-memory-arguments".to_owned(), 1),
        // verifySchnorrSecp256k1Signature — LinearInY in V3 maps
        (
            "verifySchnorrSecp256k1Signature-cpu-arguments-intercept".to_owned(),
            7_000,
        ),
        (
            "verifySchnorrSecp256k1Signature-cpu-arguments-slope".to_owned(),
            20,
        ),
        (
            "verifySchnorrSecp256k1Signature-memory-arguments".to_owned(),
            10,
        ),
    ])
}

#[test]
fn derives_per_step_kind_costs_from_named_params() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");
    // Var/Const/Lam/Delay/Force/Apply all = 29_773 CPU, 100 MEM in sample
    assert_eq!(model.step_costs.var_cpu, 29_773);
    assert_eq!(model.step_costs.var_mem, 100);
    assert_eq!(model.step_costs.constant_cpu, 29_773);
    assert_eq!(model.step_costs.lam_cpu, 29_773);
    assert_eq!(model.step_costs.apply_cpu, 29_773);
    assert_eq!(model.step_costs.delay_cpu, 29_773);
    assert_eq!(model.step_costs.force_cpu, 29_773);
    // Constr/Case have distinct values in sample
    assert_eq!(model.step_costs.constr_cpu, 30_001);
    assert_eq!(model.step_costs.constr_mem, 101);
    assert_eq!(model.step_costs.case_cpu, 30_002);
    assert_eq!(model.step_costs.case_mem, 102);
    // Backward-compat: machine_step_cost() returns max
    assert_eq!(model.machine_step_cost().cpu, 30_002);
    assert_eq!(model.machine_step_cost().mem, 102);
    // Per-builtin fallback
    assert_eq!(model.builtin_cpu, 29_773);
    assert_eq!(model.builtin_mem, 100);
}

#[test]
fn per_step_kind_costs_differentiated() {
    let mut params = sample_params();
    params.insert("cekApplyCost-exBudgetCPU".to_owned(), 40_000);
    params.insert("cekConstrCost-exBudgetMemory".to_owned(), 111);
    let model = CostModel::from_alonzo_genesis_params(&params).expect("derive cost model");
    assert_eq!(model.step_costs.apply_cpu, 40_000);
    assert_eq!(model.step_costs.constr_mem, 111);
    // Other step kinds unchanged
    assert_eq!(model.step_costs.var_cpu, 29_773);
    // machine_step_cost max should reflect highest
    assert_eq!(model.machine_step_cost().cpu, 40_000);
    assert_eq!(model.machine_step_cost().mem, 111);
}

/// `cekBuiltinCost` is now optional — per-builtin entries replace it.
#[test]
fn rejects_missing_parameter() {
    let mut params = sample_params();
    params.remove("cekBuiltinCost-exBudgetCPU");
    let model = CostModel::from_alonzo_genesis_params(&params);
    assert!(
        model.is_ok(),
        "optional cekBuiltinCost must not fail parsing"
    );
}

#[test]
fn per_builtin_add_integer_parsed() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");
    assert!(
        model.builtin_costs.contains_key(&DefaultFun::AddInteger),
        "AddInteger must have a per-builtin entry after parsing"
    );
}

#[test]
fn per_builtin_sha2_256_linear_cost() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::Sha2_256)
        .expect("Sha2_256 must have a per-builtin entry");

    // Empty input -> intercept + one bytestring word * slope.
    let cost_empty = entry.evaluate(&[Value::Constant(crate::types::Constant::ByteString(vec![]))]);
    assert_eq!(
        cost_empty.cpu,
        2_477_736 + 29_175,
        "empty input: cpu should include the one-word bytestring size"
    );

    // 1-byte input -> intercept + 1 word * slope
    let cost_one = entry.evaluate(&[Value::Constant(crate::types::Constant::ByteString(vec![
        0u8,
    ]))]);
    assert_eq!(
        cost_one.cpu,
        2_477_736 + 29_175,
        "1-byte input: cpu = intercept + slope"
    );
}

#[test]
fn per_builtin_if_then_else_constant() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::IfThenElse)
        .expect("IfThenElse must have a per-builtin entry");
    let cost = entry.evaluate(&[]);
    assert_eq!(cost.cpu, 1);
    assert_eq!(cost.mem, 1);
}

#[test]
fn builtin_cost_uses_per_builtin_entry() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");
    // sha2_256 on empty input; per-builtin entry must win over flat fallback.
    let cost = model
        .builtin_cost(
            DefaultFun::Sha2_256,
            &[Value::Constant(crate::types::Constant::ByteString(vec![]))],
        )
        .expect("per-builtin entry present");
    assert_eq!(
        cost.cpu,
        2_477_736 + 29_175,
        "builtin_cost must use per-builtin entry, not flat fallback"
    );
}

#[test]
fn verify_ed25519_cost_tracks_message_length() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");

    let short = model
        .builtin_cost(
            DefaultFun::VerifyEd25519Signature,
            &[
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 32])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 1])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 64])),
            ],
        )
        .expect("per-builtin entry present");
    let long = model
        .builtin_cost(
            DefaultFun::VerifyEd25519Signature,
            &[
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 32])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 9])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 64])),
            ],
        )
        .expect("per-builtin entry present");

    assert_eq!(short.cpu, 5_010);
    assert_eq!(long.cpu, 5_020);
}

#[test]
fn verify_schnorr_cost_parses_v3_linear_form() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");

    let cost = model
        .builtin_cost(
            DefaultFun::VerifySchnorrSecp256k1Signature,
            &[
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 32])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 3])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 64])),
            ],
        )
        .expect("per-builtin entry present");

    assert_eq!(cost.cpu, 7_020);
    assert_eq!(cost.mem, 10);
}

#[test]
fn builtin_cost_falls_back_for_unknown_builtin() {
    // Default (non-strict) model has no per-builtin entries — flat fallback applies.
    let model = CostModel::default();
    assert!(
        !model.strict_builtin_costs,
        "default model must not be strict so tests/non-production paths can use flat fallback"
    );
    let cost = model
        .builtin_cost(DefaultFun::AddInteger, &[])
        .expect("non-strict fallback returns Ok");
    assert_eq!(cost.cpu, model.builtin_cpu);
    assert_eq!(cost.mem, model.builtin_mem);
}

#[test]
fn strict_builtin_cost_errors_on_missing_entry() {
    // Production-derived models reject uncalibrated builtins instead of
    // silently charging the flat fallback. We approximate this by toggling
    // strict mode on a default model and looking up a builtin with no entry.
    let model = CostModel {
        strict_builtin_costs: true,
        ..CostModel::default()
    };
    let err = model
        .builtin_cost(DefaultFun::AddInteger, &[])
        .expect_err("strict mode must reject missing builtin entry");
    assert!(matches!(err, crate::MachineError::MissingBuiltinCost(_)));
}

#[test]
fn integer_ex_memory_zero_is_one() {
    assert_eq!(integer_ex_memory(0), 1);
}

#[test]
fn integer_ex_memory_small_values() {
    assert_eq!(integer_ex_memory(1), 1);
    assert_eq!(integer_ex_memory(u64::MAX as i128), 1); // 64 bits → 1 word
    assert_eq!(integer_ex_memory(u64::MAX as i128 + 1), 2); // 65 bits → 2 words
    assert_eq!(integer_ex_memory(-1), 1); // abs(-1) = 1
    assert_eq!(integer_ex_memory(i64::MIN as i128), 1); // 63 bits → 1 word
}

#[test]
fn integer_ex_memory_arbitrary_precision_values() {
    let n = BigInt::from(1u8) << 130u32;
    assert_eq!(integer_ex_memory(n), 3); // 131 bits -> 3 words
}

#[test]
fn bytestring_ex_memory_counts_64_bit_words() {
    assert_eq!(bytestring_ex_memory(0), 1);
    assert_eq!(bytestring_ex_memory(1), 1);
    assert_eq!(bytestring_ex_memory(8), 1);
    assert_eq!(bytestring_ex_memory(9), 2);
    assert_eq!(bytestring_ex_memory(100), 13);
}

#[test]
fn ex_memory_bytestring_is_word_length() {
    let v = Value::Constant(crate::types::Constant::ByteString(vec![0u8; 100]));
    assert_eq!(ex_memory(&v), 13);
}

#[test]
fn ex_memory_empty_bytestring_is_one() {
    let v = Value::Constant(crate::types::Constant::ByteString(vec![]));
    assert_eq!(ex_memory(&v), 1);
}

#[test]
fn ex_memory_string_counts_characters() {
    let v = Value::Constant(crate::types::Constant::String("aé".to_owned()));
    assert_eq!(ex_memory(&v), 2);
}

#[test]
fn ex_memory_polymorphic_list_is_spine_length() {
    let v = Value::Constant(crate::types::Constant::ProtoList(
        crate::types::Type::ByteString,
        vec![
            crate::types::Constant::ByteString(vec![0; 100]),
            crate::types::Constant::ByteString(vec![0; 100]),
        ],
    ));
    assert_eq!(ex_memory(&v), 2);
}

#[test]
fn ex_memory_polymorphic_pair_is_max_bound() {
    let v = Value::Constant(crate::types::Constant::ProtoPair(
        crate::types::Type::Integer,
        crate::types::Type::ByteString,
        Box::new(crate::types::Constant::integer(1)),
        Box::new(crate::types::Constant::ByteString(vec![0])),
    ));
    assert_eq!(ex_memory(&v), i64::MAX);
}

#[test]
fn data_ex_memory_charges_nodes_and_bytestring_words() {
    let data = yggdrasil_ledger::plutus::PlutusData::List(vec![
        yggdrasil_ledger::plutus::PlutusData::integer(0),
        yggdrasil_ledger::plutus::PlutusData::Bytes(vec![0; 9]),
    ]);
    assert_eq!(data_ex_memory(&data), 15);
}

#[test]
fn ex_memory_bool_is_one() {
    assert_eq!(
        ex_memory(&Value::Constant(crate::types::Constant::Bool(true))),
        1
    );
    assert_eq!(
        ex_memory(&Value::Constant(crate::types::Constant::Bool(false))),
        1
    );
}

#[test]
fn ex_memory_unit_is_one() {
    assert_eq!(ex_memory(&Value::Constant(crate::types::Constant::Unit)), 1);
}

#[test]
fn ex_memory_non_constant_runtime_values_are_one() {
    assert_eq!(
        ex_memory(&Value::Lambda(
            crate::types::Term::Var(1),
            crate::types::Environment::new()
        )),
        1
    );
    assert_eq!(
        ex_memory(&Value::Delay(
            crate::types::Term::Var(1),
            crate::types::Environment::new()
        )),
        1
    );
    assert_eq!(
        ex_memory(&Value::BuiltinApp {
            fun: crate::types::DefaultFun::IfThenElse,
            forces: 0,
            args: Vec::new(),
        }),
        1
    );
    assert_eq!(ex_memory(&Value::Constr(0, Vec::new())), 1);
}

// ---- MaxSizeYZ / ExpModCost ----

#[test]
fn max_size_yz_picks_larger_second_arg() {
    let expr = CostExpr::MaxSizeYZ {
        intercept: 100,
        slope: 5,
    };
    // sizes: [ignored, 10, 20] — arg0 ignored (e.g. boolean padding flag)
    let cost = expr.evaluate(&[0, 10, 20]);
    // max(10, 20) = 20 → 100 + 5 * 20 = 200
    assert_eq!(cost, 200);
}

#[test]
fn max_size_yz_symmetric() {
    let expr = CostExpr::MaxSizeYZ {
        intercept: 0,
        slope: 1,
    };
    assert_eq!(expr.evaluate(&[0, 30, 15]), expr.evaluate(&[0, 15, 30]));
    assert_eq!(expr.evaluate(&[0, 30, 15]), 30);
}

#[test]
fn exp_mod_cost_evaluates_polynomial() {
    let expr = CostExpr::ExpModCost {
        c00: 1000,
        c11: 10,
        c12: 2,
    };
    // sizes: [base=5, exp=3, mod=4]
    // cost0 = c00 + c11*exp*mod + c12*exp*mod^2 = 1000 + 10*3*4 + 2*3*16 = 1216
    // base(5) > mod(4) → 50% penalty: 1216 + 1216/2 = 1824
    let cost = expr.evaluate(&[5, 3, 4]);
    assert_eq!(cost, 1824);
}

#[test]
fn exp_mod_cost_no_penalty_when_base_leq_mod() {
    let expr = CostExpr::ExpModCost {
        c00: 1000,
        c11: 10,
        c12: 2,
    };
    // sizes: [base=3, exp=3, mod=4]
    // cost0 = 1000 + 10*3*4 + 2*3*16 = 1216; base(3) <= mod(4) → no penalty
    let cost = expr.evaluate(&[3, 3, 4]);
    assert_eq!(cost, 1216);
}

#[test]
fn exp_mod_cost_zero_exponent() {
    let expr = CostExpr::ExpModCost {
        c00: 500,
        c11: 100,
        c12: 50,
    };
    // y = 0 → all y-dependent terms vanish: 500 + 0 + 0 = 500
    assert_eq!(expr.evaluate(&[5, 0, 10]), 500);
}

#[test]
fn cost_expr_saturates_instead_of_overflow() {
    // ExpModCost with huge sizes must saturate to i64::MAX, not panic.
    let expr = CostExpr::ExpModCost {
        c00: 0,
        c11: i64::MAX,
        c12: i64::MAX,
    };
    let cost = expr.evaluate(&[0, i64::MAX, i64::MAX]);
    assert_eq!(cost, i64::MAX);

    // Linear expressions also saturate.
    let lin = CostExpr::LinearInX {
        intercept: i64::MAX,
        slope: i64::MAX,
    };
    assert_eq!(lin.evaluate(&[i64::MAX]), i64::MAX);
}

// ---- New CostExpr variant tests ----

#[test]
fn multiplied_sizes_basic() {
    let expr = CostExpr::MultipliedSizes {
        intercept: 100,
        slope: 5,
    };
    // 100 + 5 * (3 * 4) = 100 + 60 = 160
    assert_eq!(expr.evaluate(&[3, 4]), 160);
}

#[test]
fn multiplied_sizes_zero_arg() {
    let expr = CostExpr::MultipliedSizes {
        intercept: 50,
        slope: 10,
    };
    assert_eq!(expr.evaluate(&[0, 100]), 50);
    assert_eq!(expr.evaluate(&[100, 0]), 50);
}

#[test]
fn linear_on_diagonal_same_sizes() {
    let expr = CostExpr::LinearOnDiagonal {
        constant: 999,
        intercept: 100,
        slope: 3,
    };
    // sizes equal: intercept + slope * size = 100 + 3*10 = 130
    assert_eq!(expr.evaluate(&[10, 10]), 130);
}

#[test]
fn linear_on_diagonal_different_sizes() {
    let expr = CostExpr::LinearOnDiagonal {
        constant: 999,
        intercept: 100,
        slope: 3,
    };
    // sizes differ: returns constant
    assert_eq!(expr.evaluate(&[10, 20]), 999);
}

#[test]
fn const_above_diagonal_below() {
    let expr = CostExpr::ConstAboveDiagonal {
        constant: 42,
        inner: Box::new(CostExpr::Constant(999)),
    };
    // size0 < size1 → constant
    assert_eq!(expr.evaluate(&[3, 5]), 42);
}

#[test]
fn const_above_diagonal_at_or_above() {
    let inner = CostExpr::TwoVarQuadratic {
        minimum: 0,
        c00: 100,
        c10: 2,
        c01: 3,
        c20: 0,
        c11: 0,
        c02: 0,
    };
    let expr = CostExpr::ConstAboveDiagonal {
        constant: 42,
        inner: Box::new(inner),
    };
    // size0 >= size1 → inner: 100 + 2*5 + 3*3 = 119
    assert_eq!(expr.evaluate(&[5, 3]), 119);
    // size0 == size1 → inner: 100 + 2*4 + 3*4 = 120
    assert_eq!(expr.evaluate(&[4, 4]), 120);
}

#[test]
fn two_var_quadratic_with_minimum() {
    let expr = CostExpr::TwoVarQuadratic {
        minimum: 1000,
        c00: 10,
        c10: 1,
        c01: 1,
        c20: 0,
        c11: 0,
        c02: 0,
    };
    // 10 + 1*2 + 1*3 = 15, but minimum is 1000
    assert_eq!(expr.evaluate(&[2, 3]), 1000);
}

#[test]
fn two_var_quadratic_all_terms() {
    let expr = CostExpr::TwoVarQuadratic {
        minimum: 0,
        c00: 100,
        c10: 2,
        c01: 3,
        c20: 4,
        c11: 5,
        c02: 6,
    };
    // 100 + 2*10 + 3*20 + 4*100 + 5*200 + 6*400 = 100+20+60+400+1000+2400 = 3980
    assert_eq!(expr.evaluate(&[10, 20]), 3980);
}

#[test]
fn two_var_quadratic_negative_coefficient() {
    // Upstream divideInteger uses c02 = -900
    let expr = CostExpr::TwoVarQuadratic {
        minimum: 85848,
        c00: 123203,
        c10: 1716,
        c01: 7305,
        c20: 57,
        c11: 549,
        c02: -900,
    };
    // x=5, y=5: 123203 + 1716*5 + 7305*5 + 57*25 + 549*25 + (-900)*25
    //         = 123203 + 8580 + 36525 + 1425 + 13725 - 22500 = 160958
    assert_eq!(expr.evaluate(&[5, 5]), 160958);
}

#[test]
fn quadratic_in_y_basic() {
    let expr = CostExpr::QuadraticInY {
        c0: 1000,
        c1: 50,
        c2: 3,
    };
    // c0 + c1*y + c2*y^2 = 1000 + 50*10 + 3*100 = 1000 + 500 + 300 = 1800
    assert_eq!(expr.evaluate(&[99, 10]), 1800);
}

#[test]
fn quadratic_in_z_basic() {
    let expr = CostExpr::QuadraticInZ {
        c0: 1000,
        c1: 50,
        c2: 3,
    };
    // c0 + c1*z + c2*z^2 = 1000 + 50*10 + 3*100 = 1800
    assert_eq!(expr.evaluate(&[99, 99, 10]), 1800);
}

#[test]
fn literal_in_y_or_linear_in_z_when_y_nonzero() {
    let expr = CostExpr::LiteralInYOrLinearInZ {
        intercept: 100,
        slope: 5,
    };
    // y != 0 → returns y as literal
    assert_eq!(expr.evaluate(&[0, 42, 999]), 42);
}

#[test]
fn literal_in_y_or_linear_in_z_when_y_zero() {
    let expr = CostExpr::LiteralInYOrLinearInZ {
        intercept: 100,
        slope: 5,
    };
    // y == 0 → linear in z: 100 + 5*20 = 200
    assert_eq!(expr.evaluate(&[0, 0, 20]), 200);
}

#[test]
fn division_builtins_use_quadratic_cpu_model() {
    let mut params = sample_params();
    // Add divideInteger params matching upstream structure.
    params.insert("divideInteger-cpu-arguments-constant".to_owned(), 85848);
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-c00".to_owned(),
        123203,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-c01".to_owned(),
        7305,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-c02".to_owned(),
        -900,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-c10".to_owned(),
        1716,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-c11".to_owned(),
        549,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-c20".to_owned(),
        57,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-minimum".to_owned(),
        85848,
    );
    params.insert("divideInteger-memory-arguments-intercept".to_owned(), 0);
    params.insert("divideInteger-memory-arguments-slope".to_owned(), 1);
    params.insert("divideInteger-memory-arguments-minimum".to_owned(), 1);

    let model = CostModel::from_alonzo_genesis_params(&params).expect("parse");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::DivideInteger)
        .expect("DivideInteger must have entry");

    // size0=10, size1=5: size0 >= size1 → use TwoVarQuadratic inner
    match &entry.cpu {
        CostExpr::ConstAboveDiagonal { constant, inner } => {
            assert_eq!(*constant, 85848);
            // Evaluate inner at (10, 5)
            let val = inner.evaluate(&[10, 5]);
            let expected = 123203 + 1716 * 10 + 7305 * 5 + 57 * 100 + 549 * 50 + (-900) * 25;
            assert_eq!(val, expected.max(85848));
        }
        other => panic!("Expected ConstAboveDiagonal, got {:?}", other),
    }
}

#[test]
fn multiply_integer_uses_multiplied_sizes_cpu() {
    let model = CostModel::from_alonzo_genesis_params(&sample_params()).expect("derive cost model");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::MultiplyInteger)
        .expect("MultiplyInteger must have entry");
    match &entry.cpu {
        CostExpr::MultipliedSizes { intercept, slope } => {
            assert_eq!(*intercept, 61_516);
            assert_eq!(*slope, 11_218);
        }
        other => panic!("Expected MultipliedSizes, got {:?}", other),
    }
}

#[test]
fn variant_a_multiply_integer_uses_added_sizes_cpu() {
    let model = CostModel::from_alonzo_genesis_params_with_variant(
        &sample_params(),
        BuiltinSemanticsVariant::A,
    )
    .expect("derive variant A cost model");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::MultiplyInteger)
        .expect("MultiplyInteger must have entry");
    match &entry.cpu {
        CostExpr::AddedSizes { intercept, slope } => {
            assert_eq!(*intercept, 61_516);
            assert_eq!(*slope, 11_218);
        }
        other => panic!("Expected AddedSizes, got {:?}", other),
    }
}

#[test]
fn legacy_division_cpu_uses_const_above_diagonal_multiplied_sizes() {
    let mut params = sample_params();
    params.insert("divideInteger-cpu-arguments-constant".to_owned(), 196_500);
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-intercept".to_owned(),
        453_240,
    );
    params.insert(
        "divideInteger-cpu-arguments-model-arguments-slope".to_owned(),
        220,
    );
    params.insert("divideInteger-memory-arguments-intercept".to_owned(), 0);
    params.insert("divideInteger-memory-arguments-slope".to_owned(), 1);
    params.insert("divideInteger-memory-arguments-minimum".to_owned(), 1);

    let model =
        CostModel::from_alonzo_genesis_params_with_variant(&params, BuiltinSemanticsVariant::A)
            .expect("derive variant A cost model");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::DivideInteger)
        .expect("DivideInteger must have entry");

    match &entry.cpu {
        CostExpr::ConstAboveDiagonal { constant, inner } => {
            assert_eq!(*constant, 196_500);
            assert_eq!(entry.cpu.evaluate(&[1, 2]), 196_500);
            match inner.as_ref() {
                CostExpr::MultipliedSizes { intercept, slope } => {
                    assert_eq!(*intercept, 453_240);
                    assert_eq!(*slope, 220);
                }
                other => panic!("Expected MultipliedSizes inner, got {:?}", other),
            }
        }
        other => panic!("Expected ConstAboveDiagonal, got {:?}", other),
    }
}

#[test]
fn variant_a_mod_remainder_memory_uses_subtracted_sizes() {
    let mut params = sample_params();
    for prefix in ["modInteger", "remainderInteger"] {
        params.insert(format!("{prefix}-cpu-arguments-constant"), 196_500);
        params.insert(
            format!("{prefix}-cpu-arguments-model-arguments-intercept"),
            453_240,
        );
        params.insert(format!("{prefix}-cpu-arguments-model-arguments-slope"), 220);
        params.insert(format!("{prefix}-memory-arguments-intercept"), 0);
        params.insert(format!("{prefix}-memory-arguments-slope"), 1);
        params.insert(format!("{prefix}-memory-arguments-minimum"), 1);
    }

    let model =
        CostModel::from_alonzo_genesis_params_with_variant(&params, BuiltinSemanticsVariant::A)
            .expect("derive variant A cost model");
    for fun in [DefaultFun::ModInteger, DefaultFun::RemainderInteger] {
        let entry = model.builtin_costs.get(&fun).expect("entry present");
        match &entry.mem {
            CostExpr::SubtractedSizes {
                intercept,
                slope,
                minimum,
            } => {
                assert_eq!((*intercept, *slope, *minimum), (0, 1, 1));
            }
            other => panic!("Expected SubtractedSizes for {fun:?}, got {:?}", other),
        }
    }
}

#[test]
fn variant_c_mod_remainder_memory_uses_linear_in_y() {
    let mut params = sample_params();
    for prefix in ["modInteger", "remainderInteger"] {
        params.insert(format!("{prefix}-cpu-arguments-constant"), 85_848);
        params.insert(
            format!("{prefix}-cpu-arguments-model-arguments-c00"),
            123_203,
        );
        params.insert(format!("{prefix}-cpu-arguments-model-arguments-c01"), 7305);
        params.insert(format!("{prefix}-cpu-arguments-model-arguments-c02"), -900);
        params.insert(format!("{prefix}-cpu-arguments-model-arguments-c10"), 1716);
        params.insert(format!("{prefix}-cpu-arguments-model-arguments-c11"), 549);
        params.insert(format!("{prefix}-cpu-arguments-model-arguments-c20"), 57);
        params.insert(
            format!("{prefix}-cpu-arguments-model-arguments-minimum"),
            85_848,
        );
        params.insert(format!("{prefix}-memory-arguments-intercept"), 0);
        params.insert(format!("{prefix}-memory-arguments-slope"), 1);
    }

    let model =
        CostModel::from_alonzo_genesis_params_with_variant(&params, BuiltinSemanticsVariant::C)
            .expect("derive variant C cost model");
    for fun in [DefaultFun::ModInteger, DefaultFun::RemainderInteger] {
        let entry = model.builtin_costs.get(&fun).expect("entry present");
        match &entry.mem {
            CostExpr::LinearInY { intercept, slope } => {
                assert_eq!((*intercept, *slope), (0, 1));
            }
            other => panic!("Expected LinearInY for {fun:?}, got {:?}", other),
        }
    }
}

#[test]
fn equals_byte_string_uses_linear_on_diagonal() {
    let mut params = sample_params();
    params.insert("equalsByteString-cpu-arguments-intercept".to_owned(), 29498);
    params.insert("equalsByteString-cpu-arguments-slope".to_owned(), 38);
    params.insert("equalsByteString-cpu-arguments-constant".to_owned(), 24548);
    params.insert("equalsByteString-memory-arguments".to_owned(), 1);

    let model = CostModel::from_alonzo_genesis_params(&params).expect("parse");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::EqualsByteString)
        .expect("EqualsByteString entry");
    match &entry.cpu {
        CostExpr::LinearOnDiagonal {
            constant,
            intercept,
            slope,
        } => {
            assert_eq!(*constant, 24548);
            assert_eq!(*intercept, 29498);
            assert_eq!(*slope, 38);
        }
        other => panic!("Expected LinearOnDiagonal, got {:?}", other),
    }
}

#[test]
fn less_than_byte_string_uses_min_size() {
    let mut params = sample_params();
    params.insert(
        "lessThanByteString-cpu-arguments-intercept".to_owned(),
        28999,
    );
    params.insert("lessThanByteString-cpu-arguments-slope".to_owned(), 74);
    params.insert("lessThanByteString-memory-arguments".to_owned(), 1);

    let model = CostModel::from_alonzo_genesis_params(&params).expect("parse");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::LessThanByteString)
        .expect("LessThanByteString entry");
    match &entry.cpu {
        CostExpr::MinSize { intercept, slope } => {
            assert_eq!(*intercept, 28999);
            assert_eq!(*slope, 74);
        }
        other => panic!("Expected MinSize, got {:?}", other),
    }
}

#[test]
fn equals_data_uses_min_size() {
    let mut params = sample_params();
    params.insert("equalsData-cpu-arguments-intercept".to_owned(), 1060367);
    params.insert("equalsData-cpu-arguments-slope".to_owned(), 12586);
    params.insert("equalsData-memory-arguments".to_owned(), 1);

    let model = CostModel::from_alonzo_genesis_params(&params).expect("parse");
    let entry = model
        .builtin_costs
        .get(&DefaultFun::EqualsData)
        .expect("EqualsData entry");
    match &entry.cpu {
        CostExpr::MinSize { intercept, slope } => {
            assert_eq!(*intercept, 1060367);
            assert_eq!(*slope, 12586);
        }
        other => panic!("Expected MinSize, got {:?}", other),
    }
}
