//! Cost model for UPLC evaluation budget tracking.
//!
//! Provides a CEK cost model that charges per-step machine costs **and**
//! per-builtin costs derived from the upstream Cardano named-parameter maps.
//!
//! ## Cost function shapes
//!
//! The Cardano cost model associates each builtin with one of several costing
//! function shapes. This module supports:
//!
//! | Variant           | Formula                                          |
//! |-------------------|--------------------------------------------------|
//! | `Constant`        | `intercept`                                      |
//! | `LinearInX`       | `intercept + slope * size(arg[0])`               |
//! | `LinearInY`       | `intercept + slope * size(arg[1])`               |
//! | `LinearInZ`       | `intercept + slope * size(arg[2])`               |
//! | `LinearForm`      | `intercept + x*size0 + y*size1 + z*size2`        |
//! | `AddedSizes`      | `intercept + slope * (size[0]+size[1])`          |
//! | `MaxSize`         | `intercept + slope * max(size[0],size[1])`       |
//! | `MinSize`         | `intercept + slope * min(size[0],size[1])`       |
//! | `SubtractedSizes` | `max(min, intercept + slope*(size[0]-size[1]))`  |
//!
//! Argument sizes follow the upstream Plutus `ExMemoryUsage` type class:
//! integers and byte strings are measured in 64-bit words; polymorphic lists
//! are measured by spine length.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model>

use std::collections::BTreeMap;
use std::collections::HashMap;

use thiserror::Error;

use crate::types::{DefaultFun, ExBudget, Value};

pub mod costing_fun;
pub mod ex_memory_usage;
pub mod step;

pub use costing_fun::CostExpr;
pub use ex_memory_usage::{bytestring_ex_memory, ex_memory, integer_ex_memory};
pub use step::{StepCosts, StepKind};

/// Errors arising while building or applying a `CostModel`.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CostModelError {
    /// A required named parameter was absent.
    #[error("missing cost-model parameter: {0}")]
    MissingParameter(&'static str),
    /// A required named parameter was present but negative.
    #[error("invalid negative cost-model parameter {name}: {value}")]
    NegativeParameter { name: &'static str, value: i64 },
}

/// Upstream Plutus builtin semantics variant used when interpreting named
/// cost-model parameters.
///
/// Cardano protocol versions select a semantics variant before CEK execution:
/// V1/V2 use variant A before Conway, variant B from Conway until Van Rossem,
/// and variant D after Van Rossem; V3 uses C before Van Rossem and E after.
/// Upstream variants D and E reuse the builtin cost-model files for B and C,
/// respectively, so this Rust mapper only needs the three distinct costing
/// shapes A, B, and C.
///
/// Reference: `PlutusLedgerApi.MachineParameters.machineParametersFor` and
/// `PlutusCore.DataFilePaths` in `IntersectMBO/plutus`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinSemanticsVariant {
    /// Pre-Conway PlutusV1/V2 builtin cost semantics.
    A,
    /// Conway+ PlutusV1/V2 builtin cost semantics; also used by variant D.
    B,
    /// PlutusV3 builtin cost semantics; also used by variant E.
    C,
}

// ---------------------------------------------------------------------------
// BuiltinCostEntry — per-builtin (cpu, mem) cost expressions
// ---------------------------------------------------------------------------

/// Per-builtin costing entry containing CPU and memory `CostExpr` values.
#[derive(Clone, Debug)]
pub struct BuiltinCostEntry {
    pub cpu: CostExpr,
    pub mem: CostExpr,
}

impl BuiltinCostEntry {
    fn constant(cpu: i64, mem: i64) -> Self {
        Self {
            cpu: CostExpr::Constant(cpu),
            mem: CostExpr::Constant(mem),
        }
    }

    /// Evaluate against real argument values, returning an `ExBudget`.
    pub fn evaluate(&self, args: &[Value]) -> ExBudget {
        let sizes: Vec<i64> = args.iter().map(ex_memory).collect();
        ExBudget::new(self.cpu.evaluate(&sizes), self.mem.evaluate(&sizes))
    }
}

// ---------------------------------------------------------------------------
// CostModel
// ---------------------------------------------------------------------------

/// Cost model used by the CEK machine for budget accounting.
///
/// Stores per-step machine costs **and** per-builtin costing entries derived
/// from the upstream Cardano named-parameter maps.
///
/// Use [`CostModel::from_alonzo_genesis_params`] to build from the
/// `costModels.PlutusV1` / `costModels.PlutusV2` maps in `alonzo-genesis.json`.
/// Use [`CostModel::default`] for tests.
#[derive(Clone, Debug)]
pub struct CostModel {
    /// Per-operation step costs matching upstream CEK machine step kinds.
    pub step_costs: StepCosts,
    /// One-time startup cost charged at the beginning of evaluation.
    ///
    /// Upstream: `cekStartupCost-exBudgetCPU` / `cekStartupCost-exBudgetMemory`.
    pub startup_cost: ExBudget,
    /// Flat-fallback CPU cost per builtin (used when no per-builtin entry exists).
    pub builtin_cpu: i64,
    /// Flat-fallback memory cost per builtin.
    pub builtin_mem: i64,
    /// Per-builtin parameterized costing entries.
    ///
    /// When a `DefaultFun` key is present here, its `BuiltinCostEntry` is
    /// evaluated against actual argument sizes. When absent, the flat
    /// `builtin_cpu` / `builtin_mem` fallback is used instead — unless
    /// [`Self::strict_builtin_costs`] is set, in which case
    /// [`builtin_cost`](Self::builtin_cost) returns a structural
    /// `MachineError::MissingBuiltinCost` to surface incomplete cost models
    /// at runtime instead of silently charging fallback costs.
    pub builtin_costs: HashMap<DefaultFun, BuiltinCostEntry>,

    /// Runtime builtin semantics selected by the active protocol version.
    ///
    /// This is not only a costing selector: a small number of builtins changed
    /// behavior across Plutus semantics variants. The CEK builtin dispatcher
    /// must consult this value when executing those builtins.
    pub builtin_semantics_variant: BuiltinSemanticsVariant,

    /// When true, [`Self::builtin_cost`] fails with
    /// `MachineError::MissingBuiltinCost` for any builtin lacking a per-builtin
    /// entry instead of returning the flat fallback. Production cost models
    /// derived from upstream genesis parameters should set this to `true` so
    /// missing builtins are surfaced as a structural error rather than masked
    /// by uncalibrated default costs.
    ///
    /// Reference: upstream `Cardano.Ledger.Alonzo.Plutus.CostModels`
    /// `mkCostModel` requires complete builtin coverage.
    pub strict_builtin_costs: bool,
}

impl Default for CostModel {
    /// Conservative default suitable for unit tests.
    ///
    /// Production nodes MUST supply real cost models from protocol parameters.
    fn default() -> Self {
        Self {
            step_costs: StepCosts::default(),
            startup_cost: ExBudget::new(100, 100),
            builtin_cpu: 1_000,
            builtin_mem: 1_000,
            builtin_costs: HashMap::new(),
            builtin_semantics_variant: BuiltinSemanticsVariant::B,
            strict_builtin_costs: false,
        }
    }
}

impl CostModel {
    /// Derive a cost model from an upstream Alonzo / Babbage named Plutus
    /// cost-model map (`costModels.PlutusV1` or `costModels.PlutusV2` from
    /// `alonzo-genesis.json`).
    ///
    /// **Machine step costs**: per-operation costs matching upstream CEK
    /// step kinds (`Var`, `Const`, `Lam`, `Delay`, `Force`, `Apply`,
    /// `Builtin`, `Constr`, `Case`).
    ///
    /// **Per-builtin costs**: each `DefaultFun` is mapped to a
    /// [`BuiltinCostEntry`] based on the key patterns found in the map.
    /// Unknown or partially-specified entries are silently skipped so that
    /// future cost-model extensions don't break older node versions.
    ///
    /// `cekBuiltinCost-*` is optional when per-builtin entries cover all builtins;
    /// it acts as a flat fallback for any builtin not present in `builtin_costs`.
    pub fn from_alonzo_genesis_params(
        params: &BTreeMap<String, i64>,
    ) -> Result<Self, CostModelError> {
        Self::from_alonzo_genesis_params_with_variant(params, BuiltinSemanticsVariant::B)
    }

    /// Derive a cost model from named protocol parameters using the specified
    /// upstream builtin semantics variant.
    ///
    /// Use this when replaying transactions because the same named parameter
    /// keys can map to different builtin costing shapes depending on the active
    /// protocol major version.
    pub fn from_alonzo_genesis_params_with_variant(
        params: &BTreeMap<String, i64>,
        builtin_semantics_variant: BuiltinSemanticsVariant,
    ) -> Result<Self, CostModelError> {
        let var_cpu = named_value(params, "cekVarCost-exBudgetCPU")?;
        let var_mem = named_value(params, "cekVarCost-exBudgetMemory")?;
        let constant_cpu = named_value(params, "cekConstCost-exBudgetCPU")?;
        let constant_mem = named_value(params, "cekConstCost-exBudgetMemory")?;
        let lam_cpu = named_value(params, "cekLamCost-exBudgetCPU")?;
        let lam_mem = named_value(params, "cekLamCost-exBudgetMemory")?;
        let apply_cpu = named_value(params, "cekApplyCost-exBudgetCPU")?;
        let apply_mem = named_value(params, "cekApplyCost-exBudgetMemory")?;
        let delay_cpu = named_value(params, "cekDelayCost-exBudgetCPU")?;
        let delay_mem = named_value(params, "cekDelayCost-exBudgetMemory")?;
        let force_cpu = named_value(params, "cekForceCost-exBudgetCPU")?;
        let force_mem = named_value(params, "cekForceCost-exBudgetMemory")?;
        // cekBuiltinCost as the node-level step cost for encountering a Builtin term
        let builtin_step_cpu = params
            .get("cekBuiltinCost-exBudgetCPU")
            .copied()
            .unwrap_or(var_cpu);
        let builtin_step_mem = params
            .get("cekBuiltinCost-exBudgetMemory")
            .copied()
            .unwrap_or(var_mem);
        // Constr/Case are optional (PlutusV3+), default to Apply cost
        let constr_cpu = params
            .get("cekConstrCost-exBudgetCPU")
            .copied()
            .unwrap_or(apply_cpu);
        let constr_mem = params
            .get("cekConstrCost-exBudgetMemory")
            .copied()
            .unwrap_or(apply_mem);
        let case_cpu = params
            .get("cekCaseCost-exBudgetCPU")
            .copied()
            .unwrap_or(apply_cpu);
        let case_mem = params
            .get("cekCaseCost-exBudgetMemory")
            .copied()
            .unwrap_or(apply_mem);

        let step_costs = StepCosts {
            var_cpu,
            var_mem,
            constant_cpu,
            constant_mem,
            lam_cpu,
            lam_mem,
            apply_cpu,
            apply_mem,
            delay_cpu,
            delay_mem,
            force_cpu,
            force_mem,
            builtin_cpu: builtin_step_cpu,
            builtin_mem: builtin_step_mem,
            constr_cpu,
            constr_mem,
            case_cpu,
            case_mem,
        };

        let builtin_cpu = params
            .get("cekBuiltinCost-exBudgetCPU")
            .copied()
            .unwrap_or(1_000);
        let builtin_mem = params
            .get("cekBuiltinCost-exBudgetMemory")
            .copied()
            .unwrap_or(1_000);
        let builtin_costs = build_per_builtin_costs(params, builtin_semantics_variant);

        // Startup cost charged once at the beginning of evaluation.
        let startup_cpu = params
            .get("cekStartupCost-exBudgetCPU")
            .copied()
            .unwrap_or(0);
        let startup_mem = params
            .get("cekStartupCost-exBudgetMemory")
            .copied()
            .unwrap_or(0);
        let startup_cost = ExBudget::new(startup_cpu, startup_mem);

        Ok(Self {
            step_costs,
            startup_cost,
            builtin_cpu,
            builtin_mem,
            builtin_costs,
            builtin_semantics_variant,
            // Production-derived models must surface uncalibrated builtins
            // instead of silently falling back to flat costs.
            strict_builtin_costs: true,
        })
    }

    /// Cost charged for a specific CEK machine step kind.
    pub fn step_cost(&self, kind: StepKind) -> ExBudget {
        self.step_costs.cost(kind)
    }

    /// Cost charged per CEK machine step (maximum across all step kinds).
    ///
    /// Retained for backward compatibility; prefer `step_cost(kind)`.
    pub fn machine_step_cost(&self) -> ExBudget {
        let s = &self.step_costs;
        let max_cpu = [
            s.var_cpu,
            s.constant_cpu,
            s.lam_cpu,
            s.apply_cpu,
            s.delay_cpu,
            s.force_cpu,
            s.builtin_cpu,
            s.constr_cpu,
            s.case_cpu,
        ]
        .into_iter()
        .max()
        .unwrap_or(100);
        let max_mem = [
            s.var_mem,
            s.constant_mem,
            s.lam_mem,
            s.apply_mem,
            s.delay_mem,
            s.force_mem,
            s.builtin_mem,
            s.constr_mem,
            s.case_mem,
        ]
        .into_iter()
        .max()
        .unwrap_or(100);
        ExBudget::new(max_cpu, max_mem)
    }

    /// Cost charged for invoking a saturated builtin.
    ///
    /// Uses the per-builtin [`BuiltinCostEntry`] when available, evaluated
    /// against the actual argument sizes. When [`Self::strict_builtin_costs`]
    /// is `false`, falls back to the flat `builtin_cpu` / `builtin_mem` costs
    /// for any builtin without a per-builtin entry. When strict mode is
    /// enabled, returns [`crate::error::MachineError::MissingBuiltinCost`]
    /// instead so incomplete cost models surface as a structural failure.
    pub fn builtin_cost(
        &self,
        fun: DefaultFun,
        args: &[Value],
    ) -> Result<ExBudget, crate::MachineError> {
        if let Some(entry) = self.builtin_costs.get(&fun) {
            Ok(entry.evaluate(args))
        } else if self.strict_builtin_costs {
            Err(crate::MachineError::MissingBuiltinCost(format!("{fun:?}")))
        } else {
            Ok(ExBudget::new(self.builtin_cpu, self.builtin_mem))
        }
    }
}

// ---------------------------------------------------------------------------
// Per-builtin cost table construction
// ---------------------------------------------------------------------------

/// Build the per-builtin HashMap from a named Alonzo cost-model parameter map.
///
/// Tries each key pattern for each builtin; skips entries that are missing or
/// incomplete. Unknown genesis keys are silently ignored.
fn build_per_builtin_costs(
    params: &BTreeMap<String, i64>,
    builtin_semantics_variant: BuiltinSemanticsVariant,
) -> HashMap<DefaultFun, BuiltinCostEntry> {
    use DefaultFun::*;

    let get = |key: &str| -> Option<i64> { params.get(key).copied() };

    let mut map: HashMap<DefaultFun, BuiltinCostEntry> = HashMap::new();

    // ------------------------------------------------------------------
    // Integer arithmetic
    // ------------------------------------------------------------------

    // addInteger / subtractInteger: MaxSize for both dimensions
    for (fun, prefix) in [
        (AddInteger, "addInteger"),
        (SubtractInteger, "subtractInteger"),
    ] {
        let ci = get(&format!("{prefix}-cpu-arguments-intercept"));
        let cs = get(&format!("{prefix}-cpu-arguments-slope"));
        let mi = get(&format!("{prefix}-memory-arguments-intercept"));
        let ms = get(&format!("{prefix}-memory-arguments-slope"));
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::MaxSize {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::MaxSize {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // multiplyInteger: Variant A uses AddedSizes CPU. Variants B/C use
    // MultipliedSizes CPU. Memory is AddedSizes for all variants.
    {
        let ci = get("multiplyInteger-cpu-arguments-intercept");
        let cs = get("multiplyInteger-cpu-arguments-slope");
        let mi = get("multiplyInteger-memory-arguments-intercept");
        let ms = get("multiplyInteger-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                MultiplyInteger,
                BuiltinCostEntry {
                    cpu: match builtin_semantics_variant {
                        BuiltinSemanticsVariant::A => CostExpr::AddedSizes {
                            intercept: ci,
                            slope: cs,
                        },
                        BuiltinSemanticsVariant::B | BuiltinSemanticsVariant::C => {
                            CostExpr::MultipliedSizes {
                                intercept: ci,
                                slope: cs,
                            }
                        }
                    },
                    mem: CostExpr::AddedSizes {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // divideInteger / modInteger / quotientInteger / remainderInteger:
    //   Variants A/B CPU = ConstAboveDiagonal(MultipliedSizes).
    //   Variant C CPU = ConstAboveDiagonal(TwoVarQuadratic).
    //   divide/quotient memory = SubtractedSizes for all variants.
    //   mod/remainder memory = SubtractedSizes for A/B, LinearInY for C.
    for (fun, p) in [
        (DivideInteger, "divideInteger"),
        (ModInteger, "modInteger"),
        (QuotientInteger, "quotientInteger"),
        (RemainderInteger, "remainderInteger"),
    ] {
        // CPU: try ConstAboveDiagonal(TwoVarQuadratic) keys first, then
        // legacy ConstAboveDiagonal(MultipliedSizes), then constant.
        let cpu = if let Some(c00) = get(&format!("{p}-cpu-arguments-model-arguments-c00")) {
            let constant = get(&format!("{p}-cpu-arguments-constant")).unwrap_or(0);
            let c01 = get(&format!("{p}-cpu-arguments-model-arguments-c01")).unwrap_or(0);
            let c02 = get(&format!("{p}-cpu-arguments-model-arguments-c02")).unwrap_or(0);
            let c10 = get(&format!("{p}-cpu-arguments-model-arguments-c10")).unwrap_or(0);
            let c11 = get(&format!("{p}-cpu-arguments-model-arguments-c11")).unwrap_or(0);
            let c20 = get(&format!("{p}-cpu-arguments-model-arguments-c20")).unwrap_or(0);
            let minimum = get(&format!("{p}-cpu-arguments-model-arguments-minimum")).unwrap_or(0);
            Some(CostExpr::ConstAboveDiagonal {
                constant,
                inner: Box::new(CostExpr::TwoVarQuadratic {
                    minimum,
                    c00,
                    c10,
                    c01,
                    c20,
                    c11,
                    c02,
                }),
            })
        } else if let (Some(ci), Some(cs)) = (
            get(&format!("{p}-cpu-arguments-model-arguments-intercept")),
            get(&format!("{p}-cpu-arguments-model-arguments-slope")),
        ) {
            let constant = get(&format!("{p}-cpu-arguments-constant")).unwrap_or(0);
            Some(CostExpr::ConstAboveDiagonal {
                constant,
                inner: Box::new(CostExpr::MultipliedSizes {
                    intercept: ci,
                    slope: cs,
                }),
            })
        } else {
            get(&format!("{p}-cpu-arguments-constant")).map(CostExpr::Constant)
        };
        // Memory shape depends on the builtin variant.
        let mem = match fun {
            ModInteger | RemainderInteger
                if builtin_semantics_variant == BuiltinSemanticsVariant::C =>
            {
                if let (Some(mi), Some(ms)) = (
                    get(&format!("{p}-memory-arguments-intercept")),
                    get(&format!("{p}-memory-arguments-slope")),
                ) {
                    Some(CostExpr::LinearInY {
                        intercept: mi,
                        slope: ms,
                    })
                } else {
                    get(&format!("{p}-memory-arguments-intercept")).map(CostExpr::Constant)
                }
            }
            _ => {
                if let (Some(mi), Some(ms)) = (
                    get(&format!("{p}-memory-arguments-intercept")),
                    get(&format!("{p}-memory-arguments-slope")),
                ) {
                    let minimum = get(&format!("{p}-memory-arguments-minimum")).unwrap_or(0);
                    Some(CostExpr::SubtractedSizes {
                        intercept: mi,
                        slope: ms,
                        minimum,
                    })
                } else {
                    get(&format!("{p}-memory-arguments-intercept")).map(CostExpr::Constant)
                }
            }
        };
        if let (Some(cpu), Some(mem)) = (cpu, mem) {
            map.insert(fun, BuiltinCostEntry { cpu, mem });
        }
    }

    // equalsInteger / lessThanInteger / lessThanEqualsInteger: MinSize / constant-mem
    for (fun, p) in [
        (EqualsInteger, "equalsInteger"),
        (LessThanInteger, "lessThanInteger"),
        (LessThanEqualsInteger, "lessThanEqualsInteger"),
    ] {
        let cpu = if let (Some(ci), Some(cs)) = (
            get(&format!("{p}-cpu-arguments-intercept")),
            get(&format!("{p}-cpu-arguments-slope")),
        ) {
            Some(CostExpr::MinSize {
                intercept: ci,
                slope: cs,
            })
        } else {
            get(&format!("{p}-cpu-arguments")).map(CostExpr::Constant)
        };
        let mem = get(&format!("{p}-memory-arguments"))
            .map(CostExpr::Constant)
            .unwrap_or(CostExpr::Constant(1));
        if let Some(cpu) = cpu {
            map.insert(fun, BuiltinCostEntry { cpu, mem });
        }
    }

    // ------------------------------------------------------------------
    // ByteString operations
    // ------------------------------------------------------------------

    // appendByteString: AddedSizes
    {
        let ci = get("appendByteString-cpu-arguments-intercept");
        let cs = get("appendByteString-cpu-arguments-slope");
        let mi = get("appendByteString-memory-arguments-intercept");
        let ms = get("appendByteString-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                AppendByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::AddedSizes {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::AddedSizes {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // consByteString: CPU = LinearInY (upstream: linear_in_y),
    // memory = AddedSizes (upstream: added_sizes).
    {
        let ci = get("consByteString-cpu-arguments-intercept");
        let cs = get("consByteString-cpu-arguments-slope");
        let mi = get("consByteString-memory-arguments-intercept");
        let ms = get("consByteString-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                ConsByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInY {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::AddedSizes {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // sliceByteString: LinearInZ (slice-length arg)
    {
        let ci = get("sliceByteString-cpu-arguments-intercept");
        let cs = get("sliceByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mi = get("sliceByteString-memory-arguments-intercept").unwrap_or(4);
            let ms = get("sliceByteString-memory-arguments-slope").unwrap_or(0);
            map.insert(
                SliceByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInZ {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInZ {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // equalsByteString: CPU = LinearOnDiagonal (upstream: linear_on_diagonal),
    // memory = constant.
    {
        let ci = get("equalsByteString-cpu-arguments-intercept");
        let cs = get("equalsByteString-cpu-arguments-slope");
        let cc = get("equalsByteString-cpu-arguments-constant");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let constant = cc.unwrap_or(ci);
            let mem = get("equalsByteString-memory-arguments").unwrap_or(1);
            map.insert(
                EqualsByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearOnDiagonal {
                        constant,
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // lessThanByteString: CPU = MinSize (upstream: min_size), memory = constant.
    {
        let ci = get("lessThanByteString-cpu-arguments-intercept");
        let cs = get("lessThanByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("lessThanByteString-memory-arguments").unwrap_or(1);
            map.insert(
                LessThanByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::MinSize {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // lessThanEqualsByteString: CPU = MinSize (upstream: min_size), memory = constant.
    {
        let ci = get("lessThanEqualsByteString-cpu-arguments-intercept");
        let cs = get("lessThanEqualsByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("lessThanEqualsByteString-memory-arguments").unwrap_or(1);
            map.insert(
                LessThanEqualsByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::MinSize {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        } else if let Some(entry) = map.get(&LessThanByteString) {
            // Fall back to LessThanByteString costs if LessThanEqualsByteString absent.
            map.insert(LessThanEqualsByteString, entry.clone());
        }
    }

    // ------------------------------------------------------------------
    // Cryptographic hashing — LinearInX / constant-mem
    // ------------------------------------------------------------------

    for (fun, p, mem_key) in [
        (Sha2_256, "sha2_256", "sha2_256-memory-arguments"),
        (Sha3_256, "sha3_256", "sha3_256-memory-arguments"),
    ] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get(mem_key).unwrap_or(4);
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // blake2b: Alonzo genesis key is "blake2b", later genesis uses "blake2b_256"
    for p in ["blake2b", "blake2b_256"] {
        if map.contains_key(&Blake2b_256) {
            break;
        }
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get(&format!("{p}-memory-arguments")).unwrap_or(4);
            map.insert(
                Blake2b_256,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // verifyEd25519Signature: Alonzo key is "verifySignature"
    // cpu = LinearInY (message-length arg), mem = constant
    for p in ["verifyEd25519Signature", "verifySignature"] {
        if map.contains_key(&VerifyEd25519Signature) {
            break;
        }
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get(&format!("{p}-memory-arguments")).unwrap_or(1);
            map.insert(
                VerifyEd25519Signature,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInY {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // ------------------------------------------------------------------
    // String operations
    // ------------------------------------------------------------------

    // appendString: AddedSizes
    {
        let ci = get("appendString-cpu-arguments-intercept");
        let cs = get("appendString-cpu-arguments-slope");
        let mi = get("appendString-memory-arguments-intercept");
        let ms = get("appendString-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                AppendString,
                BuiltinCostEntry {
                    cpu: CostExpr::AddedSizes {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::AddedSizes {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // equalsString: CPU = LinearOnDiagonal (upstream: linear_on_diagonal),
    // memory = constant.
    {
        let ci = get("equalsString-cpu-arguments-intercept");
        let cs = get("equalsString-cpu-arguments-slope");
        let cc = get("equalsString-cpu-arguments-constant");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let constant = cc.unwrap_or(ci);
            let mem = get("equalsString-memory-arguments").unwrap_or(1);
            map.insert(
                EqualsString,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearOnDiagonal {
                        constant,
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // encodeUtf8 / decodeUtf8: LinearInX for both dimensions
    for (fun, p) in [(EncodeUtf8, "encodeUtf8"), (DecodeUtf8, "decodeUtf8")] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mi = get(&format!("{p}-memory-arguments-intercept")).unwrap_or(0);
            let ms = get(&format!("{p}-memory-arguments-slope")).unwrap_or(1);
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInX {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // ------------------------------------------------------------------
    // Simple constant-cost builtins (single cpu-arguments key each)
    // ------------------------------------------------------------------
    const CONSTANT_BUILTINS: &[(&str, DefaultFun)] = &[
        ("ifThenElse", IfThenElse),
        ("chooseUnit", ChooseUnit),
        ("trace", Trace),
        ("fstPair", FstPair),
        ("sndPair", SndPair),
        ("chooseList", ChooseList),
        ("mkCons", MkCons),
        ("headList", HeadList),
        ("tailList", TailList),
        ("nullList", NullList),
        ("chooseData", ChooseData),
        ("constrData", ConstrData),
        ("mapData", MapData),
        ("listData", ListData),
        ("iData", IData),
        ("bData", BData),
        ("unConstrData", UnConstrData),
        ("unMapData", UnMapData),
        ("unListData", UnListData),
        ("unIData", UnIData),
        ("unBData", UnBData),
        ("mkPairData", MkPairData),
        ("mkNilData", MkNilData),
        ("mkNilPairData", MkNilPairData),
        ("lengthOfByteString", LengthOfByteString),
        ("indexByteString", IndexByteString),
    ];
    for (prefix, fun) in CONSTANT_BUILTINS {
        if map.contains_key(fun) {
            continue;
        }
        if let Some(c) = get(&format!("{prefix}-cpu-arguments")) {
            let m = get(&format!("{prefix}-memory-arguments")).unwrap_or(1);
            map.insert(*fun, BuiltinCostEntry::constant(c, m));
        }
    }

    // ------------------------------------------------------------------
    // Data builtins
    // ------------------------------------------------------------------

    // equalsData: CPU = MinSize (upstream: min_size), memory = constant.
    {
        let ci = get("equalsData-cpu-arguments-intercept");
        let cs = get("equalsData-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("equalsData-memory-arguments").unwrap_or(1);
            map.insert(
                EqualsData,
                BuiltinCostEntry {
                    cpu: CostExpr::MinSize {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(mem),
                },
            );
        }
    }

    // serialiseData: LinearInX for both dimensions
    {
        let ci = get("serialiseData-cpu-arguments-intercept");
        let cs = get("serialiseData-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mi = get("serialiseData-memory-arguments-intercept").unwrap_or(0);
            let ms = get("serialiseData-memory-arguments-slope").unwrap_or(2);
            map.insert(
                SerialiseData,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInX {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // ------------------------------------------------------------------
    // Secp256k1 signature verification
    // ------------------------------------------------------------------
    for (fun, p) in [
        (
            VerifyEcdsaSecp256k1Signature,
            "verifyEcdsaSecp256k1Signature",
        ),
        (
            VerifySchnorrSecp256k1Signature,
            "verifySchnorrSecp256k1Signature",
        ),
    ] {
        if let (Some(ci), Some(cs)) = (
            get(&format!("{p}-cpu-arguments-intercept")),
            get(&format!("{p}-cpu-arguments-slope")),
        ) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(10);
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInY {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(m),
                },
            );
        } else if let Some(c) = get(&format!("{p}-cpu-arguments")) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(10);
            map.insert(fun, BuiltinCostEntry::constant(c, m));
        }
    }

    // ------------------------------------------------------------------
    // BLS12-381 builtins (PlutusV3)
    // ------------------------------------------------------------------

    // Constant-cost BLS group operations
    for (fun, p) in [
        (Bls12_381_G1_Add, "bls12_381_G1_add"),
        (Bls12_381_G1_Neg, "bls12_381_G1_neg"),
        (Bls12_381_G1_Equal, "bls12_381_G1_equal"),
        (Bls12_381_G1_Compress, "bls12_381_G1_compress"),
        (Bls12_381_G1_Uncompress, "bls12_381_G1_uncompress"),
        (Bls12_381_G2_Add, "bls12_381_G2_add"),
        (Bls12_381_G2_Neg, "bls12_381_G2_neg"),
        (Bls12_381_G2_Equal, "bls12_381_G2_equal"),
        (Bls12_381_G2_Compress, "bls12_381_G2_compress"),
        (Bls12_381_G2_Uncompress, "bls12_381_G2_uncompress"),
        (Bls12_381_MillerLoop, "bls12_381_millerLoop"),
        (Bls12_381_MulMlResult, "bls12_381_mulMlResult"),
        (Bls12_381_FinalVerify, "bls12_381_finalVerify"),
    ] {
        if let Some(c) = get(&format!("{p}-cpu-arguments")) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(6);
            map.insert(fun, BuiltinCostEntry::constant(c, m));
        }
    }

    // scalarMul: LinearInX (scalar word-size)
    for (fun, p) in [
        (Bls12_381_G1_ScalarMul, "bls12_381_G1_scalarMul"),
        (Bls12_381_G2_ScalarMul, "bls12_381_G2_scalarMul"),
    ] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(6);
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(m),
                },
            );
        }
    }

    // hashToGroup: LinearInX (message byte-length)
    for (fun, p) in [
        (Bls12_381_G1_HashToGroup, "bls12_381_G1_hashToGroup"),
        (Bls12_381_G2_HashToGroup, "bls12_381_G2_hashToGroup"),
    ] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(6);
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(m),
                },
            );
        }
    }

    // ------------------------------------------------------------------
    // PlutusV3 additional hashing builtins
    // ------------------------------------------------------------------
    for (fun, p) in [
        (Keccak_256, "keccak_256"),
        (Blake2b_224, "blake2b_224"),
        (Ripemd_160, "ripemd_160"),
    ] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(4);
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(m),
                },
            );
        }
    }

    // integerToByteString: CPU = QuadraticInZ (upstream: quadratic_in_z, c0+c1*z+c2*z²),
    // memory = LiteralInYOrLinearInZ (upstream: literal_in_y_or_linear_in_z).
    {
        let c0 = get("integerToByteString-cpu-arguments-c0");
        let c1 = get("integerToByteString-cpu-arguments-c1");
        let c2 = get("integerToByteString-cpu-arguments-c2");
        let mi = get("integerToByteString-memory-arguments-intercept");
        let ms = get("integerToByteString-memory-arguments-slope");
        if let (Some(c0), Some(c1), Some(c2), Some(mi), Some(ms)) = (c0, c1, c2, mi, ms) {
            map.insert(
                IntegerToByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::QuadraticInZ { c0, c1, c2 },
                    mem: CostExpr::LiteralInYOrLinearInZ {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // byteStringToInteger: CPU = QuadraticInY (upstream: quadratic_in_y, c0+c1*y+c2*y²),
    // memory = LinearInY.
    {
        let c0 = get("byteStringToInteger-cpu-arguments-c0");
        let c1 = get("byteStringToInteger-cpu-arguments-c1");
        let c2 = get("byteStringToInteger-cpu-arguments-c2");
        let mi = get("byteStringToInteger-memory-arguments-intercept");
        let ms = get("byteStringToInteger-memory-arguments-slope");
        if let (Some(c0), Some(c1), Some(c2), Some(mi), Some(ms)) = (c0, c1, c2, mi, ms) {
            map.insert(
                ByteStringToInteger,
                BuiltinCostEntry {
                    cpu: CostExpr::QuadraticInY { c0, c1, c2 },
                    mem: CostExpr::LinearInY {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // expModInteger: polynomial CPU (`c00 + c11*(y*z) + c12*(y*z²)`),
    // linear-in-z memory (`intercept + slope * size(modulus)`).
    {
        let c00 = get("expModInteger-cpu-arguments-coefficient00");
        let c11 = get("expModInteger-cpu-arguments-coefficient11");
        let c12 = get("expModInteger-cpu-arguments-coefficient12");
        let mi = get("expModInteger-memory-arguments-intercept");
        let ms = get("expModInteger-memory-arguments-slope");
        if let (Some(c00), Some(c11), Some(c12), Some(mi), Some(ms)) = (c00, c11, c12, mi, ms) {
            map.insert(
                ExpModInteger,
                BuiltinCostEntry {
                    cpu: CostExpr::ExpModCost { c00, c11, c12 },
                    mem: CostExpr::LinearInZ {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        } else if let Some(c) = get("expModInteger-cpu-arguments") {
            // Flat fallback for legacy/incomplete parameter maps.
            let m = get("expModInteger-memory-arguments").unwrap_or(1);
            map.insert(ExpModInteger, BuiltinCostEntry::constant(c, m));
        }
    }

    // ------------------------------------------------------------------
    // Bitwise builtins (PlutusV3 / CIP-0058, CIP-0123)
    // ------------------------------------------------------------------

    // andByteString, orByteString, xorByteString:
    // CPU = intercept + slope1*size(arg[1]) + slope2*size(arg[2])
    // Memory = intercept + slope*max(size(arg[1]), size(arg[2]))
    for (fun, prefix) in [
        (AndByteString, "andByteString"),
        (OrByteString, "orByteString"),
        (XorByteString, "xorByteString"),
    ] {
        let ci = get(&format!("{prefix}-cpu-arguments-intercept"));
        let cs1 = get(&format!("{prefix}-cpu-arguments-slope1"));
        let cs2 = get(&format!("{prefix}-cpu-arguments-slope2"));
        let mi = get(&format!("{prefix}-memory-arguments-intercept"));
        let ms = get(&format!("{prefix}-memory-arguments-slope"));
        if let (Some(ci), Some(cs1), Some(cs2), Some(mi), Some(ms)) = (ci, cs1, cs2, mi, ms) {
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearForm {
                        intercept: ci,
                        x: 0,
                        y: cs1,
                        z: cs2,
                    },
                    mem: CostExpr::MaxSizeYZ {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // complementByteString: LinearInX for both CPU and memory.
    {
        let ci = get("complementByteString-cpu-arguments-intercept");
        let cs = get("complementByteString-cpu-arguments-slope");
        let mi = get("complementByteString-memory-arguments-intercept");
        let ms = get("complementByteString-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                ComplementByteString,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInX {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // readBit: constant for both.
    {
        let c = get("readBit-cpu-arguments");
        let m = get("readBit-memory-arguments");
        if let (Some(c), Some(m)) = (c, m) {
            map.insert(ReadBit, BuiltinCostEntry::constant(c, m));
        }
    }

    // writeBits: CPU linear in Y (list length), memory linear in X (bytestring size).
    {
        let ci = get("writeBits-cpu-arguments-intercept");
        let cs = get("writeBits-cpu-arguments-slope");
        let mi = get("writeBits-memory-arguments-intercept");
        let ms = get("writeBits-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                WriteBits,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInY {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInX {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // replicateByte: LinearInX for both.
    {
        let ci = get("replicateByte-cpu-arguments-intercept");
        let cs = get("replicateByte-cpu-arguments-slope");
        let mi = get("replicateByte-memory-arguments-intercept");
        let ms = get("replicateByte-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                ReplicateByte,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInX {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // shiftByteString, rotateByteString: LinearInX for both.
    for (fun, prefix) in [
        (ShiftByteString, "shiftByteString"),
        (RotateByteString, "rotateByteString"),
    ] {
        let ci = get(&format!("{prefix}-cpu-arguments-intercept"));
        let cs = get(&format!("{prefix}-cpu-arguments-slope"));
        let mi = get(&format!("{prefix}-memory-arguments-intercept"));
        let ms = get(&format!("{prefix}-memory-arguments-slope"));
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::LinearInX {
                        intercept: mi,
                        slope: ms,
                    },
                },
            );
        }
    }

    // countSetBits, findFirstSetBit: CPU LinearInX, memory constant.
    for (fun, prefix) in [
        (CountSetBits, "countSetBits"),
        (FindFirstSetBit, "findFirstSetBit"),
    ] {
        let ci = get(&format!("{prefix}-cpu-arguments-intercept"));
        let cs = get(&format!("{prefix}-cpu-arguments-slope"));
        let m = get(&format!("{prefix}-memory-arguments"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            map.insert(
                fun,
                BuiltinCostEntry {
                    cpu: CostExpr::LinearInX {
                        intercept: ci,
                        slope: cs,
                    },
                    mem: CostExpr::Constant(m.unwrap_or(1)),
                },
            );
        }
    }

    map
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn named_value(params: &BTreeMap<String, i64>, key: &'static str) -> Result<i64, CostModelError> {
    let value = *params
        .get(key)
        .ok_or(CostModelError::MissingParameter(key))?;
    if value < 0 {
        return Err(CostModelError::NegativeParameter { name: key, value });
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
