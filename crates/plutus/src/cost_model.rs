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
//! integers are measured in 64-bit words, byte strings in bytes, etc.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model>

use std::collections::BTreeMap;
use std::collections::HashMap;

use thiserror::Error;

use crate::types::{DefaultFun, ExBudget, Value};

// ---------------------------------------------------------------------------
// Step kinds — per-operation costs
// ---------------------------------------------------------------------------

/// CEK machine operation kinds, each charged a distinct step cost.
///
/// Matches upstream `StepKind` from the Haskell CEK machine:
/// <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek/Internal.hs>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepKind {
    Constant,
    Var,
    LamAbs,
    Apply,
    Delay,
    Force,
    Builtin,
    Constr,
    Case,
}

/// Per-step-kind CPU and memory costs.
#[derive(Clone, Debug)]
pub struct StepCosts {
    pub var_cpu: i64,
    pub var_mem: i64,
    pub constant_cpu: i64,
    pub constant_mem: i64,
    pub lam_cpu: i64,
    pub lam_mem: i64,
    pub apply_cpu: i64,
    pub apply_mem: i64,
    pub delay_cpu: i64,
    pub delay_mem: i64,
    pub force_cpu: i64,
    pub force_mem: i64,
    pub builtin_cpu: i64,
    pub builtin_mem: i64,
    pub constr_cpu: i64,
    pub constr_mem: i64,
    pub case_cpu: i64,
    pub case_mem: i64,
}

impl Default for StepCosts {
    fn default() -> Self {
        Self {
            var_cpu: 100, var_mem: 100,
            constant_cpu: 100, constant_mem: 100,
            lam_cpu: 100, lam_mem: 100,
            apply_cpu: 100, apply_mem: 100,
            delay_cpu: 100, delay_mem: 100,
            force_cpu: 100, force_mem: 100,
            builtin_cpu: 100, builtin_mem: 100,
            constr_cpu: 100, constr_mem: 100,
            case_cpu: 100, case_mem: 100,
        }
    }
}

impl StepCosts {
    /// Return the CPU and memory cost for a particular step kind.
    pub fn cost(&self, kind: StepKind) -> ExBudget {
        match kind {
            StepKind::Var      => ExBudget::new(self.var_cpu, self.var_mem),
            StepKind::Constant => ExBudget::new(self.constant_cpu, self.constant_mem),
            StepKind::LamAbs   => ExBudget::new(self.lam_cpu, self.lam_mem),
            StepKind::Apply    => ExBudget::new(self.apply_cpu, self.apply_mem),
            StepKind::Delay    => ExBudget::new(self.delay_cpu, self.delay_mem),
            StepKind::Force    => ExBudget::new(self.force_cpu, self.force_mem),
            StepKind::Builtin  => ExBudget::new(self.builtin_cpu, self.builtin_mem),
            StepKind::Constr   => ExBudget::new(self.constr_cpu, self.constr_mem),
            StepKind::Case     => ExBudget::new(self.case_cpu, self.case_mem),
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned while deriving a CEK cost model from upstream named params.
#[derive(Debug, Error)]
pub enum CostModelError {
    /// A required named parameter was absent.
    #[error("missing cost-model parameter: {0}")]
    MissingParameter(&'static str),
    /// A required named parameter was present but negative.
    #[error("invalid negative cost-model parameter {name}: {value}")]
    NegativeParameter {
        name: &'static str,
        value: i64,
    },
}

// ---------------------------------------------------------------------------
// ExMemory — argument size measurement
// ---------------------------------------------------------------------------

/// Compute the ExMemory size of a CEK runtime value.
///
/// Matches the upstream Plutus `ExMemoryUsage` type class:
/// - Integers: 64-bit words needed for the absolute value (minimum 1).
/// - ByteStrings / Strings: byte length.
/// - Bool / Unit: 1.
/// - Pair: 1 + size(fst) + size(snd).
/// - List: 1 + Σ element sizes.
/// - Data: recursive node cost (4 per node).
/// - BLS elements: fixed word counts (6 / 12 / 6 for G1 / G2 / MlResult).
/// - Non-data runtime values (lambda, delay, partial builtin): 0.
pub fn ex_memory(value: &Value) -> i64 {
    match value {
        Value::Constant(c) => constant_ex_memory(c),
        _ => 0,
    }
}

fn constant_ex_memory(c: &crate::types::Constant) -> i64 {
    use crate::types::Constant::*;
    match c {
        Integer(n)                  => integer_ex_memory(*n),
        ByteString(bs)              => bs.len() as i64,
        String(s)                   => s.len() as i64,
        Unit                        => 1,
        Bool(_)                     => 1,
        ProtoList(_, elems)         => 1 + elems.iter().map(constant_ex_memory).sum::<i64>(),
        ProtoPair(_, _, a, b)       => 1 + constant_ex_memory(a) + constant_ex_memory(b),
        Data(d)                     => data_ex_memory(d),
        // G1=48B=6 words, G2=96B=12 words, MlResult=48B=6 words
        Bls12_381_G1_Element(_)     => 6,
        Bls12_381_G2_Element(_)     => 12,
        Bls12_381_MlResult(_)       => 6,
    }
}

/// Compute the ExMemory size of an integer value.
///
/// size = ceil(bit_length(|n|) / 64), minimum 1.
/// Matches Haskell `nWords` in `ExMemoryUsage Integer`.
pub fn integer_ex_memory(n: i128) -> i64 {
    if n == 0 {
        return 1;
    }
    let bits = 128u32 - n.unsigned_abs().leading_zeros();
    ((bits as i64) + 63) / 64
}

/// Compute the ExMemory size of a `PlutusData` value.
///
/// Matches upstream `dataSize` with a base cost of 4 per node.
fn data_ex_memory(d: &yggdrasil_ledger::plutus::PlutusData) -> i64 {
    use yggdrasil_ledger::plutus::PlutusData::*;
    match d {
        Constr(_, fields) => 4 + fields.iter().map(data_ex_memory).sum::<i64>(),
        Map(pairs)        => 4 + pairs.iter()
                                .map(|(k, v)| data_ex_memory(k) + data_ex_memory(v))
                                .sum::<i64>(),
        List(items)       => 4 + items.iter().map(data_ex_memory).sum::<i64>(),
        Integer(n)        => 4 + integer_ex_memory(*n),
        Bytes(bs)         => 4 + bs.len() as i64,
    }
}

// ---------------------------------------------------------------------------
// Cost expression shapes
// ---------------------------------------------------------------------------

/// A single-dimension cost expression (CPU or memory) for a builtin.
///
/// Each variant mirrors one upstream Haskell `CostingFun` shape.
#[derive(Clone, Debug)]
pub enum CostExpr {
    /// Fixed cost regardless of argument sizes: `intercept`.
    Constant(i64),
    /// `intercept + slope * size(arg[0])`.
    LinearInX { intercept: i64, slope: i64 },
    /// `intercept + slope * size(arg[1])`.
    LinearInY { intercept: i64, slope: i64 },
    /// `intercept + slope * size(arg[2])`.
    LinearInZ { intercept: i64, slope: i64 },
    /// `intercept + x*size(arg[0]) + y*size(arg[1]) + z*size(arg[2])`.
    LinearForm { intercept: i64, x: i64, y: i64, z: i64 },
    /// `intercept + slope * (size(arg[0]) + size(arg[1]))`.
    AddedSizes { intercept: i64, slope: i64 },
    /// `intercept + slope * max(size(arg[0]), size(arg[1]))`.
    MaxSize { intercept: i64, slope: i64 },
    /// `intercept + slope * min(size(arg[0]), size(arg[1]))`.
    MinSize { intercept: i64, slope: i64 },
    /// `max(minimum, intercept + slope * max(0, size(arg[0]) - size(arg[1])))`.
    SubtractedSizes { intercept: i64, slope: i64, minimum: i64 },
    /// `intercept + slope * max(size(arg[1]), size(arg[2]))`.
    ///
    /// Used by and/or/xorByteString memory costing where the first arg is a
    /// boolean padding flag and the two bytestring operands are args 1 and 2.
    MaxSizeYZ { intercept: i64, slope: i64 },
    /// `c00 + c11 * (y * z) + c12 * (y * z²)` where y = size(arg[1]),
    /// z = size(arg[2]).
    ///
    /// Upstream `expModInteger` CPU model from the Plutus R benchmarks.
    ExpModCost { c00: i64, c11: i64, c12: i64 },
}

impl CostExpr {
    /// Evaluate the cost expression given the pre-computed per-argument sizes.
    ///
    /// Missing argument sizes default to 1 (conservative). Results are clamped
    /// to 0 from below.
    pub fn evaluate(&self, sizes: &[i64]) -> i64 {
        let sz = |idx: usize| sizes.get(idx).copied().unwrap_or(1).max(0);
        let raw = match self {
            Self::Constant(c)                          => *c,
            Self::LinearInX { intercept, slope }       => intercept.saturating_add(slope.saturating_mul(sz(0))),
            Self::LinearInY { intercept, slope }       => intercept.saturating_add(slope.saturating_mul(sz(1))),
            Self::LinearInZ { intercept, slope }       => intercept.saturating_add(slope.saturating_mul(sz(2))),
            Self::LinearForm { intercept, x, y, z }    => {
                intercept
                    .saturating_add(x.saturating_mul(sz(0)))
                    .saturating_add(y.saturating_mul(sz(1)))
                    .saturating_add(z.saturating_mul(sz(2)))
            }
            Self::AddedSizes { intercept, slope }      => intercept.saturating_add(slope.saturating_mul(sz(0).saturating_add(sz(1)))),
            Self::MaxSize { intercept, slope }         => intercept.saturating_add(slope.saturating_mul(sz(0).max(sz(1)))),
            Self::MinSize { intercept, slope }         => intercept.saturating_add(slope.saturating_mul(sz(0).min(sz(1)))),
            Self::SubtractedSizes { intercept, slope, minimum } => {
                (*minimum).max(intercept.saturating_add(slope.saturating_mul((sz(0) - sz(1)).max(0))))
            }
            Self::MaxSizeYZ { intercept, slope } => intercept.saturating_add(slope.saturating_mul(sz(1).max(sz(2)))),
            Self::ExpModCost { c00, c11, c12 } => {
                let y = sz(1);
                let z = sz(2);
                let yz = y.saturating_mul(z);
                c00.saturating_add(c11.saturating_mul(yz))
                   .saturating_add(c12.saturating_mul(yz.saturating_mul(z)))
            }
        };
        raw.max(0)
    }
}

/// Per-builtin costing entry containing CPU and memory `CostExpr` values.
#[derive(Clone, Debug)]
pub struct BuiltinCostEntry {
    pub cpu: CostExpr,
    pub mem: CostExpr,
}

impl BuiltinCostEntry {
    fn constant(cpu: i64, mem: i64) -> Self {
        Self { cpu: CostExpr::Constant(cpu), mem: CostExpr::Constant(mem) }
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
    /// `builtin_cpu` / `builtin_mem` fallback is used instead.
    pub builtin_costs: HashMap<DefaultFun, BuiltinCostEntry>,
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
        let builtin_step_cpu = params.get("cekBuiltinCost-exBudgetCPU").copied().unwrap_or(var_cpu);
        let builtin_step_mem = params.get("cekBuiltinCost-exBudgetMemory").copied().unwrap_or(var_mem);
        // Constr/Case are optional (PlutusV3+), default to Apply cost
        let constr_cpu = params.get("cekConstrCost-exBudgetCPU").copied().unwrap_or(apply_cpu);
        let constr_mem = params.get("cekConstrCost-exBudgetMemory").copied().unwrap_or(apply_mem);
        let case_cpu = params.get("cekCaseCost-exBudgetCPU").copied().unwrap_or(apply_cpu);
        let case_mem = params.get("cekCaseCost-exBudgetMemory").copied().unwrap_or(apply_mem);

        let step_costs = StepCosts {
            var_cpu, var_mem,
            constant_cpu, constant_mem,
            lam_cpu, lam_mem,
            apply_cpu, apply_mem,
            delay_cpu, delay_mem,
            force_cpu, force_mem,
            builtin_cpu: builtin_step_cpu, builtin_mem: builtin_step_mem,
            constr_cpu, constr_mem,
            case_cpu, case_mem,
        };

        let builtin_cpu = params.get("cekBuiltinCost-exBudgetCPU").copied().unwrap_or(1_000);
        let builtin_mem = params.get("cekBuiltinCost-exBudgetMemory").copied().unwrap_or(1_000);
        let builtin_costs = build_per_builtin_costs(params);

        // Startup cost charged once at the beginning of evaluation.
        let startup_cpu = params.get("cekStartupCost-exBudgetCPU").copied().unwrap_or(0);
        let startup_mem = params.get("cekStartupCost-exBudgetMemory").copied().unwrap_or(0);
        let startup_cost = ExBudget::new(startup_cpu, startup_mem);

        Ok(Self { step_costs, startup_cost, builtin_cpu, builtin_mem, builtin_costs })
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
        let max_cpu = [s.var_cpu, s.constant_cpu, s.lam_cpu, s.apply_cpu,
                       s.delay_cpu, s.force_cpu, s.builtin_cpu, s.constr_cpu,
                       s.case_cpu].into_iter().max().unwrap_or(100);
        let max_mem = [s.var_mem, s.constant_mem, s.lam_mem, s.apply_mem,
                       s.delay_mem, s.force_mem, s.builtin_mem, s.constr_mem,
                       s.case_mem].into_iter().max().unwrap_or(100);
        ExBudget::new(max_cpu, max_mem)
    }

    /// Cost charged for invoking a saturated builtin.
    ///
    /// Uses the per-builtin [`BuiltinCostEntry`] when available, evaluated
    /// against the actual argument sizes. Falls back to the flat
    /// `builtin_cpu` / `builtin_mem` costs for any builtin with no entry.
    pub fn builtin_cost(&self, fun: DefaultFun, args: &[Value]) -> ExBudget {
        if let Some(entry) = self.builtin_costs.get(&fun) {
            entry.evaluate(args)
        } else {
            ExBudget::new(self.builtin_cpu, self.builtin_mem)
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
) -> HashMap<DefaultFun, BuiltinCostEntry> {
    use DefaultFun::*;

    let get = |key: &str| -> Option<i64> { params.get(key).copied() };

    let mut map: HashMap<DefaultFun, BuiltinCostEntry> = HashMap::new();

    // ------------------------------------------------------------------
    // Integer arithmetic
    // ------------------------------------------------------------------

    // addInteger / subtractInteger: MaxSize for both dimensions
    for (fun, prefix) in [(AddInteger, "addInteger"), (SubtractInteger, "subtractInteger")] {
        let ci = get(&format!("{prefix}-cpu-arguments-intercept"));
        let cs = get(&format!("{prefix}-cpu-arguments-slope"));
        let mi = get(&format!("{prefix}-memory-arguments-intercept"));
        let ms = get(&format!("{prefix}-memory-arguments-slope"));
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::MaxSize { intercept: ci, slope: cs },
                mem: CostExpr::MaxSize { intercept: mi, slope: ms },
            });
        }
    }

    // multiplyInteger: AddedSizes for both dimensions
    {
        let ci = get("multiplyInteger-cpu-arguments-intercept");
        let cs = get("multiplyInteger-cpu-arguments-slope");
        let mi = get("multiplyInteger-memory-arguments-intercept");
        let ms = get("multiplyInteger-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(MultiplyInteger, BuiltinCostEntry {
                cpu: CostExpr::AddedSizes { intercept: ci, slope: cs },
                mem: CostExpr::AddedSizes { intercept: mi, slope: ms },
            });
        }
    }

    // divideInteger / modInteger / quotientInteger / remainderInteger:
    // cpu = SubtractedSizes (via model-arguments form); mem = SubtractedSizes
    for (fun, p) in [
        (DivideInteger,    "divideInteger"),
        (ModInteger,       "modInteger"),
        (QuotientInteger,  "quotientInteger"),
        (RemainderInteger, "remainderInteger"),
    ] {
        let cpu = if let (Some(ci), Some(cs)) = (
            get(&format!("{p}-cpu-arguments-model-arguments-intercept")),
            get(&format!("{p}-cpu-arguments-model-arguments-slope")),
        ) {
            Some(CostExpr::SubtractedSizes { intercept: ci, slope: cs, minimum: 0 })
        } else {
            get(&format!("{p}-cpu-arguments-constant")).map(CostExpr::Constant)
        };
        let mem = if let (Some(mi), Some(ms)) = (
            get(&format!("{p}-memory-arguments-intercept")),
            get(&format!("{p}-memory-arguments-slope")),
        ) {
            let minimum = get(&format!("{p}-memory-arguments-minimum")).unwrap_or(0);
            Some(CostExpr::SubtractedSizes { intercept: mi, slope: ms, minimum })
        } else {
            get(&format!("{p}-memory-arguments-intercept")).map(CostExpr::Constant)
        };
        if let (Some(cpu), Some(mem)) = (cpu, mem) {
            map.insert(fun, BuiltinCostEntry { cpu, mem });
        }
    }

    // equalsInteger / lessThanInteger / lessThanEqualsInteger: MinSize / constant-mem
    for (fun, p) in [
        (EqualsInteger,         "equalsInteger"),
        (LessThanInteger,       "lessThanInteger"),
        (LessThanEqualsInteger, "lessThanEqualsInteger"),
    ] {
        let cpu = if let (Some(ci), Some(cs)) = (
            get(&format!("{p}-cpu-arguments-intercept")),
            get(&format!("{p}-cpu-arguments-slope")),
        ) {
            Some(CostExpr::MinSize { intercept: ci, slope: cs })
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
            map.insert(AppendByteString, BuiltinCostEntry {
                cpu: CostExpr::AddedSizes { intercept: ci, slope: cs },
                mem: CostExpr::AddedSizes { intercept: mi, slope: ms },
            });
        }
    }

    // consByteString: LinearInY (output byte-string length = input + 1)
    {
        let ci = get("consByteString-cpu-arguments-intercept");
        let cs = get("consByteString-cpu-arguments-slope");
        let mi = get("consByteString-memory-arguments-intercept");
        let ms = get("consByteString-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(ConsByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInY { intercept: ci, slope: cs },
                mem: CostExpr::LinearInY { intercept: mi, slope: ms },
            });
        }
    }

    // sliceByteString: LinearInZ (slice-length arg)
    {
        let ci = get("sliceByteString-cpu-arguments-intercept");
        let cs = get("sliceByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mi = get("sliceByteString-memory-arguments-intercept").unwrap_or(4);
            let ms = get("sliceByteString-memory-arguments-slope").unwrap_or(0);
            map.insert(SliceByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInZ { intercept: ci, slope: cs },
                mem: CostExpr::LinearInZ { intercept: mi, slope: ms },
            });
        }
    }

    // equalsByteString: LinearInX / constant-mem
    {
        let ci = get("equalsByteString-cpu-arguments-intercept");
        let cs = get("equalsByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("equalsByteString-memory-arguments").unwrap_or(1);
            map.insert(EqualsByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
        }
    }

    // lessThanByteString
    {
        let ci = get("lessThanByteString-cpu-arguments-intercept");
        let cs = get("lessThanByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("lessThanByteString-memory-arguments").unwrap_or(1);
            map.insert(LessThanByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
        }
    }

    // lessThanEqualsByteString
    {
        let ci = get("lessThanEqualsByteString-cpu-arguments-intercept");
        let cs = get("lessThanEqualsByteString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("lessThanEqualsByteString-memory-arguments").unwrap_or(1);
            map.insert(LessThanEqualsByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
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
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
        }
    }

    // blake2b: Alonzo genesis key is "blake2b", later genesis uses "blake2b_256"
    for p in ["blake2b", "blake2b_256"] {
        if map.contains_key(&Blake2b_256) { break; }
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get(&format!("{p}-memory-arguments")).unwrap_or(4);
            map.insert(Blake2b_256, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
        }
    }

    // verifyEd25519Signature: Alonzo key is "verifySignature"
    // cpu = LinearInY (message-length arg), mem = constant
    for p in ["verifyEd25519Signature", "verifySignature"] {
        if map.contains_key(&VerifyEd25519Signature) { break; }
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get(&format!("{p}-memory-arguments")).unwrap_or(1);
            map.insert(VerifyEd25519Signature, BuiltinCostEntry {
                cpu: CostExpr::LinearInY { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
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
            map.insert(AppendString, BuiltinCostEntry {
                cpu: CostExpr::AddedSizes { intercept: ci, slope: cs },
                mem: CostExpr::AddedSizes { intercept: mi, slope: ms },
            });
        }
    }

    // equalsString: LinearInX / constant-mem
    {
        let ci = get("equalsString-cpu-arguments-intercept");
        let cs = get("equalsString-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("equalsString-memory-arguments").unwrap_or(1);
            map.insert(EqualsString, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
        }
    }

    // encodeUtf8 / decodeUtf8: LinearInX for both dimensions
    for (fun, p) in [(EncodeUtf8, "encodeUtf8"), (DecodeUtf8, "decodeUtf8")] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mi = get(&format!("{p}-memory-arguments-intercept")).unwrap_or(0);
            let ms = get(&format!("{p}-memory-arguments-slope")).unwrap_or(1);
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::LinearInX { intercept: mi, slope: ms },
            });
        }
    }

    // ------------------------------------------------------------------
    // Simple constant-cost builtins (single cpu-arguments key each)
    // ------------------------------------------------------------------
    const CONSTANT_BUILTINS: &[(&str, DefaultFun)] = &[
        ("ifThenElse",         IfThenElse),
        ("chooseUnit",         ChooseUnit),
        ("trace",              Trace),
        ("fstPair",            FstPair),
        ("sndPair",            SndPair),
        ("chooseList",         ChooseList),
        ("mkCons",             MkCons),
        ("headList",           HeadList),
        ("tailList",           TailList),
        ("nullList",           NullList),
        ("chooseData",         ChooseData),
        ("constrData",         ConstrData),
        ("mapData",            MapData),
        ("listData",           ListData),
        ("iData",              IData),
        ("bData",              BData),
        ("unConstrData",       UnConstrData),
        ("unMapData",          UnMapData),
        ("unListData",         UnListData),
        ("unIData",            UnIData),
        ("unBData",            UnBData),
        ("mkPairData",         MkPairData),
        ("mkNilData",          MkNilData),
        ("mkNilPairData",      MkNilPairData),
        ("lengthOfByteString", LengthOfByteString),
        ("indexByteString",    IndexByteString),
    ];
    for (prefix, fun) in CONSTANT_BUILTINS {
        if map.contains_key(fun) { continue; }
        if let Some(c) = get(&format!("{prefix}-cpu-arguments")) {
            let m = get(&format!("{prefix}-memory-arguments")).unwrap_or(1);
            map.insert(*fun, BuiltinCostEntry::constant(c, m));
        }
    }

    // ------------------------------------------------------------------
    // Data builtins
    // ------------------------------------------------------------------

    // equalsData: LinearInX / constant-mem
    {
        let ci = get("equalsData-cpu-arguments-intercept");
        let cs = get("equalsData-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get("equalsData-memory-arguments").unwrap_or(1);
            map.insert(EqualsData, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
        }
    }

    // serialiseData: LinearInX for both dimensions
    {
        let ci = get("serialiseData-cpu-arguments-intercept");
        let cs = get("serialiseData-cpu-arguments-slope");
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mi = get("serialiseData-memory-arguments-intercept").unwrap_or(0);
            let ms = get("serialiseData-memory-arguments-slope").unwrap_or(2);
            map.insert(SerialiseData, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::LinearInX { intercept: mi, slope: ms },
            });
        }
    }

    // ------------------------------------------------------------------
    // Secp256k1 signature verification
    // ------------------------------------------------------------------
    for (fun, p) in [
        (VerifyEcdsaSecp256k1Signature,   "verifyEcdsaSecp256k1Signature"),
        (VerifySchnorrSecp256k1Signature, "verifySchnorrSecp256k1Signature"),
    ] {
        if let (Some(ci), Some(cs)) = (
            get(&format!("{p}-cpu-arguments-intercept")),
            get(&format!("{p}-cpu-arguments-slope")),
        ) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(10);
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInY { intercept: ci, slope: cs },
                mem: CostExpr::Constant(m),
            });
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
        (Bls12_381_G1_Add,        "bls12_381_G1_add"),
        (Bls12_381_G1_Neg,        "bls12_381_G1_neg"),
        (Bls12_381_G1_Equal,      "bls12_381_G1_equal"),
        (Bls12_381_G1_Compress,   "bls12_381_G1_compress"),
        (Bls12_381_G1_Uncompress, "bls12_381_G1_uncompress"),
        (Bls12_381_G2_Add,        "bls12_381_G2_add"),
        (Bls12_381_G2_Neg,        "bls12_381_G2_neg"),
        (Bls12_381_G2_Equal,      "bls12_381_G2_equal"),
        (Bls12_381_G2_Compress,   "bls12_381_G2_compress"),
        (Bls12_381_G2_Uncompress, "bls12_381_G2_uncompress"),
        (Bls12_381_MillerLoop,    "bls12_381_millerLoop"),
        (Bls12_381_MulMlResult,   "bls12_381_mulMlResult"),
        (Bls12_381_FinalVerify,   "bls12_381_finalVerify"),
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
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(m),
            });
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
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(m),
            });
        }
    }

    // ------------------------------------------------------------------
    // PlutusV3 additional hashing builtins
    // ------------------------------------------------------------------
    for (fun, p) in [
        (Keccak_256,  "keccak_256"),
        (Blake2b_224, "blake2b_224"),
        (Ripemd_160,  "ripemd_160"),
    ] {
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let m = get(&format!("{p}-memory-arguments")).unwrap_or(4);
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(m),
            });
        }
    }

    // Integer / ByteString conversion (Conway V3 array surface)
    // `integerToByteString` and `byteStringToInteger` expose coefficient-style
    // CPU models in upstream V3 maps. We treat the first coefficient as an
    // intercept term and the remaining coefficients as argument-size weights.
    {
        let c0 = get("integerToByteString-cpu-arguments-c0");
        let c1 = get("integerToByteString-cpu-arguments-c1");
        let c2 = get("integerToByteString-cpu-arguments-c2");
        let mi = get("integerToByteString-memory-arguments-intercept");
        let ms = get("integerToByteString-memory-arguments-slope");
        if let (Some(c0), Some(c1), Some(c2), Some(mi), Some(ms)) = (c0, c1, c2, mi, ms) {
            map.insert(IntegerToByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearForm { intercept: c0, x: 0, y: c1, z: c2 },
                mem: CostExpr::LinearInZ { intercept: mi, slope: ms },
            });
        }
    }

    {
        let c0 = get("byteStringToInteger-cpu-arguments-c0");
        let c1 = get("byteStringToInteger-cpu-arguments-c1");
        let c2 = get("byteStringToInteger-cpu-arguments-c2");
        let mi = get("byteStringToInteger-memory-arguments-intercept");
        let ms = get("byteStringToInteger-memory-arguments-slope");
        if let (Some(c0), Some(c1), Some(c2), Some(mi), Some(ms)) = (c0, c1, c2, mi, ms) {
            map.insert(ByteStringToInteger, BuiltinCostEntry {
                cpu: CostExpr::LinearForm { intercept: c0, x: c1, y: c2, z: 0 },
                mem: CostExpr::LinearInY { intercept: mi, slope: ms },
            });
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
            map.insert(ExpModInteger, BuiltinCostEntry {
                cpu: CostExpr::ExpModCost { c00, c11, c12 },
                mem: CostExpr::LinearInZ { intercept: mi, slope: ms },
            });
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
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearForm { intercept: ci, x: 0, y: cs1, z: cs2 },
                mem: CostExpr::MaxSizeYZ { intercept: mi, slope: ms },
            });
        }
    }

    // complementByteString: LinearInX for both CPU and memory.
    {
        let ci = get("complementByteString-cpu-arguments-intercept");
        let cs = get("complementByteString-cpu-arguments-slope");
        let mi = get("complementByteString-memory-arguments-intercept");
        let ms = get("complementByteString-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(ComplementByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::LinearInX { intercept: mi, slope: ms },
            });
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
            map.insert(WriteBits, BuiltinCostEntry {
                cpu: CostExpr::LinearInY { intercept: ci, slope: cs },
                mem: CostExpr::LinearInX { intercept: mi, slope: ms },
            });
        }
    }

    // replicateByte: LinearInX for both.
    {
        let ci = get("replicateByte-cpu-arguments-intercept");
        let cs = get("replicateByte-cpu-arguments-slope");
        let mi = get("replicateByte-memory-arguments-intercept");
        let ms = get("replicateByte-memory-arguments-slope");
        if let (Some(ci), Some(cs), Some(mi), Some(ms)) = (ci, cs, mi, ms) {
            map.insert(ReplicateByte, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::LinearInX { intercept: mi, slope: ms },
            });
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
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::LinearInX { intercept: mi, slope: ms },
            });
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
            map.insert(fun, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(m.unwrap_or(1)),
            });
        }
    }

    map
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn named_value(
    params: &BTreeMap<String, i64>,
    key: &'static str,
) -> Result<i64, CostModelError> {
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
mod tests {
    use super::*;

    fn sample_params() -> BTreeMap<String, i64> {
        BTreeMap::from([
            // Machine step costs
            ("cekVarCost-exBudgetCPU".to_owned(),      29_773),
            ("cekConstCost-exBudgetCPU".to_owned(),    29_773),
            ("cekLamCost-exBudgetCPU".to_owned(),      29_773),
            ("cekDelayCost-exBudgetCPU".to_owned(),    29_773),
            ("cekForceCost-exBudgetCPU".to_owned(),    29_773),
            ("cekApplyCost-exBudgetCPU".to_owned(),    29_773),
            ("cekVarCost-exBudgetMemory".to_owned(),   100),
            ("cekConstCost-exBudgetMemory".to_owned(), 100),
            ("cekLamCost-exBudgetMemory".to_owned(),   100),
            ("cekDelayCost-exBudgetMemory".to_owned(), 100),
            ("cekForceCost-exBudgetMemory".to_owned(), 100),
            ("cekApplyCost-exBudgetMemory".to_owned(), 100),
            ("cekBuiltinCost-exBudgetCPU".to_owned(),  29_773),
            ("cekBuiltinCost-exBudgetMemory".to_owned(), 100),
            ("cekStartupCost-exBudgetCPU".to_owned(),  100),
            ("cekStartupCost-exBudgetMemory".to_owned(), 100),
            ("cekConstrCost-exBudgetCPU".to_owned(),  30_001),
            ("cekConstrCost-exBudgetMemory".to_owned(), 101),
            ("cekCaseCost-exBudgetCPU".to_owned(),  30_002),
            ("cekCaseCost-exBudgetMemory".to_owned(), 102),
            // addInteger — MaxSize, slope=0 (effectively constant per arg)
            ("addInteger-cpu-arguments-intercept".to_owned(), 197_209),
            ("addInteger-cpu-arguments-slope".to_owned(),     0),
            ("addInteger-memory-arguments-intercept".to_owned(), 1),
            ("addInteger-memory-arguments-slope".to_owned(),     1),
            // sha2_256 — LinearInX
            ("sha2_256-cpu-arguments-intercept".to_owned(), 2_477_736),
            ("sha2_256-cpu-arguments-slope".to_owned(),     29_175),
            ("sha2_256-memory-arguments".to_owned(),        4),
            // multiplyInteger — AddedSizes
            ("multiplyInteger-cpu-arguments-intercept".to_owned(), 61_516),
            ("multiplyInteger-cpu-arguments-slope".to_owned(),     11_218),
            ("multiplyInteger-memory-arguments-intercept".to_owned(), 0),
            ("multiplyInteger-memory-arguments-slope".to_owned(),     1),
            // ifThenElse — constant
            ("ifThenElse-cpu-arguments".to_owned(), 1),
            ("ifThenElse-memory-arguments".to_owned(), 1),
            // verifyEd25519Signature — LinearInY
            ("verifyEd25519Signature-cpu-arguments-intercept".to_owned(), 5_000),
            ("verifyEd25519Signature-cpu-arguments-slope".to_owned(), 10),
            ("verifyEd25519Signature-memory-arguments".to_owned(), 1),
            // verifySchnorrSecp256k1Signature — LinearInY in V3 maps
            ("verifySchnorrSecp256k1Signature-cpu-arguments-intercept".to_owned(), 7_000),
            ("verifySchnorrSecp256k1Signature-cpu-arguments-slope".to_owned(), 20),
            ("verifySchnorrSecp256k1Signature-memory-arguments".to_owned(), 10),
        ])
    }

    #[test]
    fn derives_per_step_kind_costs_from_named_params() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
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
        let model = CostModel::from_alonzo_genesis_params(&params)
            .expect("derive cost model");
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
        assert!(model.is_ok(), "optional cekBuiltinCost must not fail parsing");
    }

    #[test]
    fn per_builtin_add_integer_parsed() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
        assert!(
            model.builtin_costs.contains_key(&DefaultFun::AddInteger),
            "AddInteger must have a per-builtin entry after parsing"
        );
    }

    #[test]
    fn per_builtin_sha2_256_linear_cost() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
        let entry = model.builtin_costs.get(&DefaultFun::Sha2_256)
            .expect("Sha2_256 must have a per-builtin entry");

        // Empty input → intercept only
        let cost_empty = entry.evaluate(&[Value::Constant(
            crate::types::Constant::ByteString(vec![]),
        )]);
        assert_eq!(cost_empty.cpu, 2_477_736, "empty input: cpu should equal intercept");

        // 1-byte input → intercept + 1 * slope
        let cost_one = entry.evaluate(&[Value::Constant(
            crate::types::Constant::ByteString(vec![0u8]),
        )]);
        assert_eq!(cost_one.cpu, 2_477_736 + 29_175, "1-byte input: cpu = intercept + slope");
    }

    #[test]
    fn per_builtin_if_then_else_constant() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
        let entry = model.builtin_costs.get(&DefaultFun::IfThenElse)
            .expect("IfThenElse must have a per-builtin entry");
        let cost = entry.evaluate(&[]);
        assert_eq!(cost.cpu, 1);
        assert_eq!(cost.mem, 1);
    }

    #[test]
    fn builtin_cost_uses_per_builtin_entry() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
        // sha2_256 on empty input — per-builtin entry must win over flat fallback
        let cost = model.builtin_cost(
            DefaultFun::Sha2_256,
            &[Value::Constant(crate::types::Constant::ByteString(vec![]))],
        );
        assert_eq!(cost.cpu, 2_477_736,
            "builtin_cost must use per-builtin entry, not flat fallback");
    }

    #[test]
    fn verify_ed25519_cost_tracks_message_length() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");

        let short = model.builtin_cost(
            DefaultFun::VerifyEd25519Signature,
            &[
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 32])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 1])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 64])),
            ],
        );
        let long = model.builtin_cost(
            DefaultFun::VerifyEd25519Signature,
            &[
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 32])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 9])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 64])),
            ],
        );

        assert_eq!(short.cpu, 5_010);
        assert_eq!(long.cpu, 5_090);
    }

    #[test]
    fn verify_schnorr_cost_parses_v3_linear_form() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");

        let cost = model.builtin_cost(
            DefaultFun::VerifySchnorrSecp256k1Signature,
            &[
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 32])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 3])),
                Value::Constant(crate::types::Constant::ByteString(vec![0u8; 64])),
            ],
        );

        assert_eq!(cost.cpu, 7_060);
        assert_eq!(cost.mem, 10);
    }

    #[test]
    fn builtin_cost_falls_back_for_unknown_builtin() {
        // Default model has no per-builtin entries — flat fallback applies.
        let model = CostModel::default();
        let cost = model.builtin_cost(DefaultFun::AddInteger, &[]);
        assert_eq!(cost.cpu, model.builtin_cpu);
        assert_eq!(cost.mem, model.builtin_mem);
    }

    #[test]
    fn integer_ex_memory_zero_is_one() {
        assert_eq!(integer_ex_memory(0), 1);
    }

    #[test]
    fn integer_ex_memory_small_values() {
        assert_eq!(integer_ex_memory(1), 1);
        assert_eq!(integer_ex_memory(u64::MAX as i128), 1);       // 64 bits → 1 word
        assert_eq!(integer_ex_memory(u64::MAX as i128 + 1), 2);   // 65 bits → 2 words
        assert_eq!(integer_ex_memory(-1), 1);                      // abs(-1) = 1
        assert_eq!(integer_ex_memory(i64::MIN as i128), 1);        // 63 bits → 1 word
    }

    #[test]
    fn ex_memory_bytestring_is_byte_length() {
        let v = Value::Constant(crate::types::Constant::ByteString(vec![0u8; 100]));
        assert_eq!(ex_memory(&v), 100);
    }

    #[test]
    fn ex_memory_empty_bytestring_is_zero() {
        let v = Value::Constant(crate::types::Constant::ByteString(vec![]));
        assert_eq!(ex_memory(&v), 0);
    }

    #[test]
    fn ex_memory_bool_is_one() {
        assert_eq!(ex_memory(&Value::Constant(crate::types::Constant::Bool(true))), 1);
        assert_eq!(ex_memory(&Value::Constant(crate::types::Constant::Bool(false))), 1);
    }

    #[test]
    fn ex_memory_unit_is_one() {
        assert_eq!(ex_memory(&Value::Constant(crate::types::Constant::Unit)), 1);
    }

    // ---- MaxSizeYZ / ExpModCost ----

    #[test]
    fn max_size_yz_picks_larger_second_arg() {
        let expr = CostExpr::MaxSizeYZ { intercept: 100, slope: 5 };
        // sizes: [ignored, 10, 20] — arg0 ignored (e.g. boolean padding flag)
        let cost = expr.evaluate(&[0, 10, 20]);
        // max(10, 20) = 20 → 100 + 5 * 20 = 200
        assert_eq!(cost, 200);
    }

    #[test]
    fn max_size_yz_symmetric() {
        let expr = CostExpr::MaxSizeYZ { intercept: 0, slope: 1 };
        assert_eq!(expr.evaluate(&[0, 30, 15]), expr.evaluate(&[0, 15, 30]));
        assert_eq!(expr.evaluate(&[0, 30, 15]), 30);
    }

    #[test]
    fn exp_mod_cost_evaluates_polynomial() {
        let expr = CostExpr::ExpModCost { c00: 1000, c11: 10, c12: 2 };
        // sizes: [base_ignored, y=3, z=4]
        // c00 + c11 * y * z + c12 * y * z^2 = 1000 + 10*3*4 + 2*3*16 = 1000 + 120 + 96 = 1216
        let cost = expr.evaluate(&[5, 3, 4]);
        assert_eq!(cost, 1216);
    }

    #[test]
    fn exp_mod_cost_zero_exponent() {
        let expr = CostExpr::ExpModCost { c00: 500, c11: 100, c12: 50 };
        // y = 0 → all y-dependent terms vanish: 500 + 0 + 0 = 500
        assert_eq!(expr.evaluate(&[5, 0, 10]), 500);
    }

    #[test]
    fn cost_expr_saturates_instead_of_overflow() {
        // ExpModCost with huge sizes must saturate to i64::MAX, not panic.
        let expr = CostExpr::ExpModCost { c00: 0, c11: i64::MAX, c12: i64::MAX };
        let cost = expr.evaluate(&[0, i64::MAX, i64::MAX]);
        assert_eq!(cost, i64::MAX);

        // Linear expressions also saturate.
        let lin = CostExpr::LinearInX { intercept: i64::MAX, slope: i64::MAX };
        assert_eq!(lin.evaluate(&[i64::MAX]), i64::MAX);
    }
}
