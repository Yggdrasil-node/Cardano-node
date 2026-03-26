//! Parameterized cost model for UPLC evaluation budget tracking.
//!
//! Implements per-builtin cost functions that scale with argument sizes,
//! matching the upstream Plutus cost model structure from
//! `plutus-core/cost-model`.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model>
//! Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/ExBudgetingDefaults.hs>

use std::collections::BTreeMap;

use thiserror::Error;

use crate::types::{Constant, DefaultFun, ExBudget, Value};

/// Errors returned while building a cost model from upstream parameters.
#[derive(Debug, Error)]
pub enum CostModelError {
    /// A required named parameter was absent.
    #[error("missing cost-model parameter: {0}")]
    MissingParameter(&'static str),
    /// A required named parameter was present but the value was invalid.
    #[error("invalid cost-model parameter {name}: {value}")]
    InvalidParameter { name: &'static str, value: i64 },
}

// ---------------------------------------------------------------------------
// Cost function types
// ---------------------------------------------------------------------------

/// A cost function over N argument sizes.
///
/// Argument sizes are measured in "words" (64-bit units) for integers and
/// bytes for bytestrings/strings. Each `CostFun` variant encodes a different
/// scaling shape matching the upstream `ModelOneArgument` / `ModelTwoArguments`
/// / `ModelThreeArguments` types from the Haskell evaluator.
///
/// Argument slots:
/// - `x` = first argument size (index 0)
/// - `y` = second argument size (index 1)
/// - `z` = third argument size (index 2)
#[derive(Clone, Debug)]
pub enum CostFun {
    /// Fixed cost, no argument-size scaling.
    Constant(i64),

    /// Linear in the first argument size: `intercept + slope * x`.
    LinearInX { intercept: i64, slope: i64 },

    /// Linear in the second argument size: `intercept + slope * y`.
    LinearInY { intercept: i64, slope: i64 },

    /// Linear in the third argument size: `intercept + slope * z`.
    LinearInZ { intercept: i64, slope: i64 },

    /// Linear in x + y: `intercept + slope * (x + y)`.
    LinearInXAndY { intercept: i64, slope: i64 },

    /// Linear in max(x, y): `intercept + slope * max(x, y)`.
    LinearInMaxXY { intercept: i64, slope: i64 },

    /// Linear in min(x, y): `intercept + slope * min(x, y)`.
    LinearInMinXY { intercept: i64, slope: i64 },

    /// Linear in x * y: `intercept + slope * x * y`.
    MultipliedSizes { intercept: i64, slope: i64 },

    /// Linear in y + z: `intercept + slope * (y + z)`.
    LinearInYAndZ { intercept: i64, slope: i64 },

    /// Constant below the diagonal, linear in (x − y) above.
    ///
    /// - When `x <= y`: cost = `constant`
    /// - When `x > y`: cost = `model_intercept + model_slope * (x − y)`
    ///
    /// Used for integer division / modulo CPU costs.
    ConstAboveDiagonal {
        constant: i64,
        model_intercept: i64,
        model_slope: i64,
    },

    /// Minimum-floored subtracted sizes.
    ///
    /// - When `x <= y`: cost = `minimum`
    /// - When `x > y`: cost = `max(intercept + slope * (x − y), minimum)`
    ///
    /// Used for integer division / modulo memory costs.
    SubtractedSizesWithMin {
        intercept: i64,
        slope: i64,
        minimum: i64,
    },

    /// Constant above the diagonal (equality-like operations).
    ///
    /// - When `x != y`: cost = `constant`
    /// - When `x == y`: cost = `model_intercept + model_slope * x`
    ///
    /// Used for `equalsByteString` and `equalsString` CPU costs.
    ConstOffDiagonalLinearOnDiagonal {
        constant: i64,
        model_intercept: i64,
        model_slope: i64,
    },
}

impl CostFun {
    /// Evaluate the cost function given a slice of argument sizes.
    ///
    /// Missing argument sizes default to 1.
    pub fn eval(&self, sizes: &[i64]) -> i64 {
        let x = sizes.first().copied().unwrap_or(1);
        let y = sizes.get(1).copied().unwrap_or(1);
        let z = sizes.get(2).copied().unwrap_or(1);

        match self {
            Self::Constant(c) => *c,
            Self::LinearInX { intercept, slope } => intercept + slope * x,
            Self::LinearInY { intercept, slope } => intercept + slope * y,
            Self::LinearInZ { intercept, slope } => intercept + slope * z,
            Self::LinearInXAndY { intercept, slope } => intercept + slope * (x + y),
            Self::LinearInMaxXY { intercept, slope } => intercept + slope * x.max(y),
            Self::LinearInMinXY { intercept, slope } => intercept + slope * x.min(y),
            Self::MultipliedSizes { intercept, slope } => intercept + slope * x * y,
            Self::LinearInYAndZ { intercept, slope } => intercept + slope * (y + z),
            Self::ConstAboveDiagonal {
                constant,
                model_intercept,
                model_slope,
            } => {
                if x <= y {
                    *constant
                } else {
                    model_intercept + model_slope * (x - y)
                }
            }
            Self::SubtractedSizesWithMin {
                intercept,
                slope,
                minimum,
            } => {
                if x <= y {
                    *minimum
                } else {
                    (intercept + slope * (x - y)).max(*minimum)
                }
            }
            Self::ConstOffDiagonalLinearOnDiagonal {
                constant,
                model_intercept,
                model_slope,
            } => {
                if x != y {
                    *constant
                } else {
                    model_intercept + model_slope * x
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-builtin cost entry
// ---------------------------------------------------------------------------

/// CPU and memory cost functions for one builtin function.
#[derive(Clone, Debug)]
pub struct BuiltinCostEntry {
    /// CPU cost function evaluated against argument sizes.
    pub cpu: CostFun,
    /// Memory cost function evaluated against argument sizes.
    pub mem: CostFun,
}

impl BuiltinCostEntry {
    fn new(cpu: CostFun, mem: CostFun) -> Self {
        Self { cpu, mem }
    }
}

// ---------------------------------------------------------------------------
// Argument size measurement
// ---------------------------------------------------------------------------

/// Compute the "size" of a `Value` in cost-model units.
///
/// Follows the upstream Plutus size semantics:
/// - Integers: number of 64-bit words needed to represent the absolute value (min 1)
/// - ByteStrings: length in bytes (min 1)
/// - Strings: UTF-8 byte length (min 1)
/// - Other: 1 (unit size)
fn arg_size(val: &Value) -> i64 {
    match val {
        Value::Constant(c) => const_size(c),
        _ => 1,
    }
}

fn const_size(c: &Constant) -> i64 {
    match c {
        Constant::Integer(n) => integer_size(*n),
        // Upstream `memoryUsage` for ByteString is the raw byte length with no minimum.
        Constant::ByteString(bs) => bs.len() as i64,
        Constant::String(s) => s.len() as i64,
        Constant::Unit => 1,
        Constant::Bool(_) => 1,
        Constant::Data(_) => 32, // conservative estimate; full recursive sizing is expensive
        Constant::ProtoList(_, items) => items.len().max(1) as i64,
        Constant::ProtoPair(..) => 1,
        // BLS elements have fixed sizes matching upstream
        Constant::Bls12_381_G1_Element(_) => 12, // 96 bytes compressed = 12 words
        Constant::Bls12_381_G2_Element(_) => 24, // 192 bytes compressed = 24 words
        Constant::Bls12_381_MlResult(_) => 72,   // 576 bytes = 72 words
    }
}

/// Number of 64-bit words required to represent the absolute value of `n`.
///
/// Matches the upstream `memoryUsage` for `Integer`:
/// `ceiling(bitLength(n) / 64)`, minimum 1.
fn integer_size(n: i128) -> i64 {
    if n == 0 {
        return 1;
    }
    let bits = 128u32.saturating_sub(n.unsigned_abs().leading_zeros());
    ((bits + 63) / 64).max(1) as i64
}

// ---------------------------------------------------------------------------
// Cost model
// ---------------------------------------------------------------------------

/// Full parameterized cost model for the CEK machine.
///
/// Carries both machine-step costs (charged per CEK reduction step) and
/// per-builtin cost functions (charged when a saturated builtin is applied).
///
/// Matches the upstream `EvaluationContext` + `CostingFun` structure from
/// `PlutusCore.Evaluation.Machine.ExBudgetingDefaults`.
#[derive(Clone, Debug)]
pub struct CostModel {
    /// CPU units charged per CEK machine step (max across all step types).
    step_cpu: i64,
    /// Memory units charged per CEK machine step (max across all step types).
    step_mem: i64,
    /// CPU fallback for builtins not present in the per-builtin table.
    default_builtin_cpu: i64,
    /// Memory fallback for builtins not present in the per-builtin table.
    default_builtin_mem: i64,
    /// Per-builtin parameterized cost entries.
    builtins: BTreeMap<DefaultFun, BuiltinCostEntry>,
}

impl Default for CostModel {
    /// Conservative defaults suitable for tests and simple scripts.
    ///
    /// Production use MUST supply the cost model from the protocol parameters
    /// (via [`CostModel::from_alonzo_genesis_params`]).
    fn default() -> Self {
        Self {
            step_cpu: 29_773,
            step_mem: 100,
            default_builtin_cpu: 29_773,
            default_builtin_mem: 100,
            builtins: BTreeMap::new(),
        }
    }
}

impl CostModel {
    /// Cost charged per CEK machine step.
    pub fn machine_step_cost(&self) -> ExBudget {
        ExBudget::new(self.step_cpu, self.step_mem)
    }

    /// Cost charged for invoking a saturated builtin.
    ///
    /// Looks up the per-builtin parameterized cost function and evaluates it
    /// against the actual argument sizes. Falls back to the default flat cost
    /// for builtins not present in the table.
    pub fn builtin_cost(&self, fun: DefaultFun, args: &[Value]) -> ExBudget {
        if let Some(entry) = self.builtins.get(&fun) {
            let sizes: Vec<i64> = args.iter().map(arg_size).collect();
            let cpu = entry.cpu.eval(&sizes).max(0);
            let mem = entry.mem.eval(&sizes).max(0);
            ExBudget::new(cpu, mem)
        } else {
            ExBudget::new(self.default_builtin_cpu, self.default_builtin_mem)
        }
    }

    /// Derive a cost model from an upstream Alonzo/Babbage named cost-model map.
    ///
    /// The map uses string keys in the format `{builtin}-{cpu|memory}-arguments[-{suffix}]`
    /// as found in `alonzo-genesis.json` (PlutusV1 section) and Babbage protocol-parameter
    /// updates (PlutusV2 section).
    ///
    /// Machine step costs are extracted from `cek*Cost-exBudget{CPU,Memory}` keys.
    /// Per-builtin costs are parsed using hardcoded cost-function shapes that match
    /// the upstream `DefaultBuiltinCostModel` in `ExBudgetingDefaults.hs`.
    pub fn from_alonzo_genesis_params(
        params: &BTreeMap<String, i64>,
    ) -> Result<Self, CostModelError> {
        // --- Machine step costs ---
        const STEP_CPU_KEYS: [&str; 6] = [
            "cekVarCost-exBudgetCPU",
            "cekConstCost-exBudgetCPU",
            "cekLamCost-exBudgetCPU",
            "cekDelayCost-exBudgetCPU",
            "cekForceCost-exBudgetCPU",
            "cekApplyCost-exBudgetCPU",
        ];
        const STEP_MEM_KEYS: [&str; 6] = [
            "cekVarCost-exBudgetMemory",
            "cekConstCost-exBudgetMemory",
            "cekLamCost-exBudgetMemory",
            "cekDelayCost-exBudgetMemory",
            "cekForceCost-exBudgetMemory",
            "cekApplyCost-exBudgetMemory",
        ];

        let step_cpu = max_named(params, &STEP_CPU_KEYS)?;
        let step_mem = max_named(params, &STEP_MEM_KEYS)?;
        let default_builtin_cpu = get_named(params, "cekBuiltinCost-exBudgetCPU")?;
        let default_builtin_mem = get_named(params, "cekBuiltinCost-exBudgetMemory")?;

        // --- Per-builtin costs ---
        let builtins = parse_builtin_costs(params);

        Ok(Self {
            step_cpu,
            step_mem,
            default_builtin_cpu,
            default_builtin_mem,
            builtins,
        })
    }
}

// ---------------------------------------------------------------------------
// Named-parameter helpers
// ---------------------------------------------------------------------------

fn get_named(params: &BTreeMap<String, i64>, key: &'static str) -> Result<i64, CostModelError> {
    params
        .get(key)
        .copied()
        .ok_or(CostModelError::MissingParameter(key))
}

fn get_named_opt(params: &BTreeMap<String, i64>, key: &str) -> Option<i64> {
    params.get(key).copied()
}

fn max_named(
    params: &BTreeMap<String, i64>,
    keys: &[&'static str],
) -> Result<i64, CostModelError> {
    let mut max = 0i64;
    for key in keys {
        max = max.max(get_named(params, key)?);
    }
    Ok(max)
}

// ---------------------------------------------------------------------------
// Per-builtin cost parsing
// ---------------------------------------------------------------------------

/// Parse all known builtin cost entries from the named parameter map.
///
/// Each builtin has a fixed cost-function shape (constant, linear, etc.)
/// determined by the upstream `DefaultBuiltinCostModel` record. The shape is
/// not encoded in the key names; it is hardcoded here to match upstream.
///
/// Missing keys result in the builtin being absent from the returned map;
/// the caller's `default_builtin_*` fallback then applies.
fn parse_builtin_costs(p: &BTreeMap<String, i64>) -> BTreeMap<DefaultFun, BuiltinCostEntry> {
    use DefaultFun::*;

    let mut m: BTreeMap<DefaultFun, BuiltinCostEntry> = BTreeMap::new();

    // Helper closures
    let constant = |key: &str| -> Option<i64> { get_named_opt(p, key) };

    let linear = |prefix: &str, resource: &str| -> Option<(i64, i64)> {
        let i = constant(&format!("{prefix}-{resource}-arguments-intercept"))?;
        let s = constant(&format!("{prefix}-{resource}-arguments-slope"))?;
        Some((i, s))
    };

    let const_entry = |cpu_key: &str, mem_key: &str| -> Option<BuiltinCostEntry> {
        let cpu = constant(cpu_key)?;
        let mem = constant(mem_key)?;
        Some(BuiltinCostEntry::new(CostFun::Constant(cpu), CostFun::Constant(mem)))
    };

    // Macro-like helper: insert only when all params are present
    macro_rules! ins {
        ($fun:expr, $entry:expr) => {
            if let Some(e) = $entry {
                m.insert($fun, e);
            }
        };
    }

    // -- Integer arithmetic --------------------------------------------------

    // addInteger / subtractInteger: cpu=LinearInMaxXY, mem=LinearInMaxXY
    for (fun, prefix) in [
        (AddInteger, "addInteger"),
        (SubtractInteger, "subtractInteger"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let (mi, ms) = linear(prefix, "memory")?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInMaxXY { intercept: ci, slope: cs },
                CostFun::LinearInMaxXY { intercept: mi, slope: ms },
            ))
        })());
    }

    // multiplyInteger: cpu=LinearInXAndY (intercept + slope*(x+y)), mem=LinearInXAndY
    ins!(MultiplyInteger, (|| {
        let (ci, cs) = linear("multiplyInteger", "cpu")?;
        let (mi, ms) = linear("multiplyInteger", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInXAndY { intercept: ci, slope: cs },
            CostFun::LinearInXAndY { intercept: mi, slope: ms },
        ))
    })());

    // divideInteger / quotientInteger / remainderInteger / modInteger:
    // cpu=ConstAboveDiagonal, mem=SubtractedSizesWithMin
    for (fun, prefix) in [
        (DivideInteger, "divideInteger"),
        (QuotientInteger, "quotientInteger"),
        (RemainderInteger, "remainderInteger"),
        (ModInteger, "modInteger"),
    ] {
        ins!(fun, (|| {
            let cc = constant(&format!("{prefix}-cpu-arguments-constant"))?;
            let mi_key = format!("{prefix}-cpu-arguments-model-arguments-intercept");
            let ms_key = format!("{prefix}-cpu-arguments-model-arguments-slope");
            let ci = get_named_opt(p, &mi_key)?;
            let cs = get_named_opt(p, &ms_key)?;
            let mem_i = constant(&format!("{prefix}-memory-arguments-intercept"))?;
            let mem_min = constant(&format!("{prefix}-memory-arguments-minimum"))?;
            let mem_s = constant(&format!("{prefix}-memory-arguments-slope"))?;
            Some(BuiltinCostEntry::new(
                CostFun::ConstAboveDiagonal {
                    constant: cc,
                    model_intercept: ci,
                    model_slope: cs,
                },
                CostFun::SubtractedSizesWithMin {
                    intercept: mem_i,
                    slope: mem_s,
                    minimum: mem_min,
                },
            ))
        })());
    }

    // equalsInteger / lessThanInteger / lessThanEqualsInteger: cpu=LinearInMinXY, mem=Constant
    for (fun, prefix) in [
        (EqualsInteger, "equalsInteger"),
        (LessThanInteger, "lessThanInteger"),
        (LessThanEqualsInteger, "lessThanEqualsInteger"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let mem = constant(&format!("{prefix}-memory-arguments"))?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInMinXY { intercept: ci, slope: cs },
                CostFun::Constant(mem),
            ))
        })());
    }

    // -- ByteString ----------------------------------------------------------

    // appendByteString: cpu=LinearInXAndY, mem=LinearInXAndY
    ins!(AppendByteString, (|| {
        let (ci, cs) = linear("appendByteString", "cpu")?;
        let (mi, ms) = linear("appendByteString", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInXAndY { intercept: ci, slope: cs },
            CostFun::LinearInXAndY { intercept: mi, slope: ms },
        ))
    })());

    // consByteString: cpu=LinearInY (slope on content), mem=LinearInY
    ins!(ConsByteString, (|| {
        let (ci, cs) = linear("consByteString", "cpu")?;
        let (mi, ms) = linear("consByteString", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInY { intercept: ci, slope: cs },
            CostFun::LinearInY { intercept: mi, slope: ms },
        ))
    })());

    // sliceByteString: cpu=LinearInZ (z = length to slice), mem=LinearInZ
    ins!(SliceByteString, (|| {
        let (ci, cs) = linear("sliceByteString", "cpu")?;
        let (mi, ms) = linear("sliceByteString", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInZ { intercept: ci, slope: cs },
            CostFun::LinearInZ { intercept: mi, slope: ms },
        ))
    })());

    // lengthOfByteString: cpu=Constant, mem=Constant
    ins!(LengthOfByteString, const_entry(
        "lengthOfByteString-cpu-arguments",
        "lengthOfByteString-memory-arguments",
    ));

    // indexByteString: cpu=Constant, mem=Constant
    ins!(IndexByteString, const_entry(
        "indexByteString-cpu-arguments",
        "indexByteString-memory-arguments",
    ));

    // equalsByteString: cpu=ConstOffDiagonalLinearOnDiagonal, mem=Constant
    ins!(EqualsByteString, (|| {
        let cc = constant("equalsByteString-cpu-arguments-constant")?;
        let ci = constant("equalsByteString-cpu-arguments-intercept")?;
        let cs = constant("equalsByteString-cpu-arguments-slope")?;
        let mem = constant("equalsByteString-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::ConstOffDiagonalLinearOnDiagonal {
                constant: cc,
                model_intercept: ci,
                model_slope: cs,
            },
            CostFun::Constant(mem),
        ))
    })());

    // lessThanByteString / lessThanEqualsByteString: cpu=LinearInX, mem=Constant
    // Note: LessThanEqualsByteString was removed from UPLC and is not in DefaultFun;
    // the genesis key "lessThanEqualsByteString" is ignored.
    ins!(LessThanByteString, (|| {
        let (ci, cs) = linear("lessThanByteString", "cpu")?;
        let mem = constant("lessThanByteString-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    // -- Hashing -------------------------------------------------------------

    // sha2_256 / sha3_256: cpu=LinearInX, mem=Constant
    // Genesis key uses "sha2_256" for sha2_256 and "sha3_256" for sha3_256.
    ins!(Sha2_256, (|| {
        let (ci, cs) = linear("sha2_256", "cpu")?;
        let mem = constant("sha2_256-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    ins!(Sha3_256, (|| {
        let (ci, cs) = linear("sha3_256", "cpu")?;
        let mem = constant("sha3_256-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    // blake2b_256: genesis key "blake2b" (old name in V1), also try "blake2b_256"
    ins!(Blake2b_256, (|| {
        // Try new name first, fall back to old genesis name
        let prefix = if p.contains_key("blake2b_256-cpu-arguments-intercept") {
            "blake2b_256"
        } else {
            "blake2b"
        };
        let (ci, cs) = linear(prefix, "cpu")?;
        let mem = constant(&format!("{prefix}-memory-arguments"))?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    // keccak_256 / blake2b_224 / ripemd_160 (PlutusV3): cpu=LinearInX, mem=Constant
    for (fun, prefix) in [
        (Keccak_256, "keccak_256"),
        (Blake2b_224, "blake2b_224"),
        (Ripemd_160, "ripemd_160"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let mem = constant(&format!("{prefix}-memory-arguments"))?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInX { intercept: ci, slope: cs },
                CostFun::Constant(mem),
            ))
        })());
    }

    // -- Signature verification ----------------------------------------------

    // verifyEd25519Signature (genesis: "verifySignature"): cpu=LinearInY (message size), mem=Constant
    ins!(VerifyEd25519Signature, (|| {
        // Try V2 name first, then V1 name
        let prefix = if p.contains_key("verifyEd25519Signature-cpu-arguments-intercept") {
            "verifyEd25519Signature"
        } else {
            "verifySignature"
        };
        let (ci, cs) = linear(prefix, "cpu")?;
        let mem = constant(&format!("{prefix}-memory-arguments"))?;
        // Message is the second argument (Y)
        Some(BuiltinCostEntry::new(
            CostFun::LinearInY { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    // verifyEcdsaSecp256k1Signature: cpu=Constant, mem=Constant (PlutusV2)
    ins!(VerifyEcdsaSecp256k1Signature, const_entry(
        "verifyEcdsaSecp256k1Signature-cpu-arguments",
        "verifyEcdsaSecp256k1Signature-memory-arguments",
    ));

    // verifySchnorrSecp256k1Signature: cpu=LinearInY (message), mem=Constant (PlutusV2)
    ins!(VerifySchnorrSecp256k1Signature, (|| {
        let (ci, cs) = linear("verifySchnorrSecp256k1Signature", "cpu")?;
        let mem = constant("verifySchnorrSecp256k1Signature-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInY { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    // -- String --------------------------------------------------------------

    // appendString: cpu=LinearInXAndY, mem=LinearInXAndY
    ins!(AppendString, (|| {
        let (ci, cs) = linear("appendString", "cpu")?;
        let (mi, ms) = linear("appendString", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInXAndY { intercept: ci, slope: cs },
            CostFun::LinearInXAndY { intercept: mi, slope: ms },
        ))
    })());

    // equalsString: cpu=ConstOffDiagonalLinearOnDiagonal, mem=Constant
    ins!(EqualsString, (|| {
        let cc = constant("equalsString-cpu-arguments-constant")?;
        let ci = constant("equalsString-cpu-arguments-intercept")?;
        let cs = constant("equalsString-cpu-arguments-slope")?;
        let mem = constant("equalsString-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::ConstOffDiagonalLinearOnDiagonal {
                constant: cc,
                model_intercept: ci,
                model_slope: cs,
            },
            CostFun::Constant(mem),
        ))
    })());

    // encodeUtf8 / decodeUtf8: cpu=LinearInX, mem=LinearInX
    for (fun, prefix) in [
        (EncodeUtf8, "encodeUtf8"),
        (DecodeUtf8, "decodeUtf8"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let (mi, ms) = linear(prefix, "memory")?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInX { intercept: ci, slope: cs },
                CostFun::LinearInX { intercept: mi, slope: ms },
            ))
        })());
    }

    // -- Constant builtins (all Constant cost) --------------------------------

    for (fun, cpu_key, mem_key) in [
        (IfThenElse,    "ifThenElse-cpu-arguments",    "ifThenElse-memory-arguments"),
        (ChooseUnit,    "chooseUnit-cpu-arguments",    "chooseUnit-memory-arguments"),
        (Trace,         "trace-cpu-arguments",         "trace-memory-arguments"),
        (FstPair,       "fstPair-cpu-arguments",       "fstPair-memory-arguments"),
        (SndPair,       "sndPair-cpu-arguments",       "sndPair-memory-arguments"),
        (ChooseList,    "chooseList-cpu-arguments",    "chooseList-memory-arguments"),
        (MkCons,        "mkCons-cpu-arguments",        "mkCons-memory-arguments"),
        (HeadList,      "headList-cpu-arguments",      "headList-memory-arguments"),
        (TailList,      "tailList-cpu-arguments",      "tailList-memory-arguments"),
        (NullList,      "nullList-cpu-arguments",      "nullList-memory-arguments"),
        (ChooseData,    "chooseData-cpu-arguments",    "chooseData-memory-arguments"),
        (ConstrData,    "constrData-cpu-arguments",    "constrData-memory-arguments"),
        (MapData,       "mapData-cpu-arguments",       "mapData-memory-arguments"),
        (ListData,      "listData-cpu-arguments",      "listData-memory-arguments"),
        (IData,         "iData-cpu-arguments",         "iData-memory-arguments"),
        (BData,         "bData-cpu-arguments",         "bData-memory-arguments"),
        (UnConstrData,  "unConstrData-cpu-arguments",  "unConstrData-memory-arguments"),
        (UnMapData,     "unMapData-cpu-arguments",     "unMapData-memory-arguments"),
        (UnListData,    "unListData-cpu-arguments",    "unListData-memory-arguments"),
        (UnIData,       "unIData-cpu-arguments",       "unIData-memory-arguments"),
        (UnBData,       "unBData-cpu-arguments",       "unBData-memory-arguments"),
        (MkPairData,    "mkPairData-cpu-arguments",    "mkPairData-memory-arguments"),
        (MkNilData,     "mkNilData-cpu-arguments",     "mkNilData-memory-arguments"),
        (MkNilPairData, "mkNilPairData-cpu-arguments", "mkNilPairData-memory-arguments"),
        (IndexByteString, "indexByteString-cpu-arguments", "indexByteString-memory-arguments"),
        (LengthOfByteString, "lengthOfByteString-cpu-arguments", "lengthOfByteString-memory-arguments"),
    ] {
        // Only insert if not already present from a more specific parse above
        if !m.contains_key(&fun) {
            ins!(fun, const_entry(cpu_key, mem_key));
        }
    }

    // equalsData: cpu=LinearInMinXY (scales with smaller data), mem=Constant
    ins!(EqualsData, (|| {
        let (ci, cs) = linear("equalsData", "cpu")?;
        let mem = constant("equalsData-memory-arguments")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInMinXY { intercept: ci, slope: cs },
            CostFun::Constant(mem),
        ))
    })());

    // serialiseData (PlutusV2+): cpu=LinearInX, mem=LinearInX
    ins!(SerialiseData, (|| {
        let (ci, cs) = linear("serialiseData", "cpu")?;
        let (mi, ms) = linear("serialiseData", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::LinearInX { intercept: mi, slope: ms },
        ))
    })());

    // -- PlutusV3: integer/bytestring conversions ----------------------------

    // integerToByteString (3 args: signed flag, output_size, input): cpu=LinearInY (output_size), mem=LinearInY
    ins!(IntegerToByteString, (|| {
        let (ci, cs) = linear("integerToByteString", "cpu")?;
        let (mi, ms) = linear("integerToByteString", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInY { intercept: ci, slope: cs },
            CostFun::LinearInY { intercept: mi, slope: ms },
        ))
    })());

    // byteStringToInteger (2 args: signed flag, input): cpu=LinearInY (input size), mem=LinearInY
    ins!(ByteStringToInteger, (|| {
        let (ci, cs) = linear("byteStringToInteger", "cpu")?;
        let (mi, ms) = linear("byteStringToInteger", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInY { intercept: ci, slope: cs },
            CostFun::LinearInY { intercept: mi, slope: ms },
        ))
    })());

    // -- PlutusV3: bitwise operations ----------------------------------------

    // andByteString / orByteString / xorByteString (3 args: lsb_first?, bs1, bs2)
    // cpu=LinearInYAndZ (bs sizes), mem=LinearInYAndZ
    for (fun, prefix) in [
        (AndByteString, "andByteString"),
        (OrByteString, "orByteString"),
        (XorByteString, "xorByteString"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let (mi, ms) = linear(prefix, "memory")?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInYAndZ { intercept: ci, slope: cs },
                CostFun::LinearInYAndZ { intercept: mi, slope: ms },
            ))
        })());
    }

    // complementByteString: cpu=LinearInX, mem=LinearInX
    ins!(ComplementByteString, (|| {
        let (ci, cs) = linear("complementByteString", "cpu")?;
        let (mi, ms) = linear("complementByteString", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::LinearInX { intercept: mi, slope: ms },
        ))
    })());

    // readBit (2 args: bs, index): cpu=Constant, mem=Constant
    ins!(ReadBit, const_entry(
        "readBit-cpu-arguments",
        "readBit-memory-arguments",
    ));

    // writeBits (3 args: bs, indices_list, bits_list): cpu=LinearInX, mem=LinearInX
    ins!(WriteBits, (|| {
        let (ci, cs) = linear("writeBits", "cpu")?;
        let (mi, ms) = linear("writeBits", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::LinearInX { intercept: mi, slope: ms },
        ))
    })());

    // replicateByte (2 args: count, byte): cpu=LinearInX, mem=LinearInX
    ins!(ReplicateByte, (|| {
        let (ci, cs) = linear("replicateByte", "cpu")?;
        let (mi, ms) = linear("replicateByte", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInX { intercept: ci, slope: cs },
            CostFun::LinearInX { intercept: mi, slope: ms },
        ))
    })());

    // shiftByteString / rotateByteString (2 args: bs, shift): cpu=LinearInX, mem=LinearInX
    for (fun, prefix) in [
        (ShiftByteString, "shiftByteString"),
        (RotateByteString, "rotateByteString"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let (mi, ms) = linear(prefix, "memory")?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInX { intercept: ci, slope: cs },
                CostFun::LinearInX { intercept: mi, slope: ms },
            ))
        })());
    }

    // countSetBits / findFirstSetBit: cpu=LinearInX, mem=Constant
    for (fun, prefix) in [
        (CountSetBits, "countSetBits"),
        (FindFirstSetBit, "findFirstSetBit"),
    ] {
        ins!(fun, (|| {
            let (ci, cs) = linear(prefix, "cpu")?;
            let mem = constant(&format!("{prefix}-memory-arguments"))?;
            Some(BuiltinCostEntry::new(
                CostFun::LinearInX { intercept: ci, slope: cs },
                CostFun::Constant(mem),
            ))
        })());
    }

    // expModInteger (3 args: base, exp, mod): cpu=LinearInZ (mod size drives cost), mem=LinearInZ
    ins!(ExpModInteger, (|| {
        let (ci, cs) = linear("expModInteger", "cpu")?;
        let (mi, ms) = linear("expModInteger", "memory")?;
        Some(BuiltinCostEntry::new(
            CostFun::LinearInZ { intercept: ci, slope: cs },
            CostFun::LinearInZ { intercept: mi, slope: ms },
        ))
    })());

    // -- PlutusV3: BLS12-381 -------------------------------------------------
    // All BLS operations use constant costs (elliptic curve ops are fixed-size)

    for (fun, cpu_key, mem_key) in [
        (Bls12_381_G1_Add,          "bls12_381_G1_add-cpu-arguments",           "bls12_381_G1_add-memory-arguments"),
        (Bls12_381_G1_Neg,          "bls12_381_G1_neg-cpu-arguments",           "bls12_381_G1_neg-memory-arguments"),
        (Bls12_381_G1_ScalarMul,    "bls12_381_G1_scalarMul-cpu-arguments",     "bls12_381_G1_scalarMul-memory-arguments"),
        (Bls12_381_G1_Equal,        "bls12_381_G1_equal-cpu-arguments",         "bls12_381_G1_equal-memory-arguments"),
        (Bls12_381_G1_HashToGroup,  "bls12_381_G1_hashToGroup-cpu-arguments",   "bls12_381_G1_hashToGroup-memory-arguments"),
        (Bls12_381_G1_Compress,     "bls12_381_G1_compress-cpu-arguments",      "bls12_381_G1_compress-memory-arguments"),
        (Bls12_381_G1_Uncompress,   "bls12_381_G1_uncompress-cpu-arguments",    "bls12_381_G1_uncompress-memory-arguments"),
        (Bls12_381_G2_Add,          "bls12_381_G2_add-cpu-arguments",           "bls12_381_G2_add-memory-arguments"),
        (Bls12_381_G2_Neg,          "bls12_381_G2_neg-cpu-arguments",           "bls12_381_G2_neg-memory-arguments"),
        (Bls12_381_G2_ScalarMul,    "bls12_381_G2_scalarMul-cpu-arguments",     "bls12_381_G2_scalarMul-memory-arguments"),
        (Bls12_381_G2_Equal,        "bls12_381_G2_equal-cpu-arguments",         "bls12_381_G2_equal-memory-arguments"),
        (Bls12_381_G2_HashToGroup,  "bls12_381_G2_hashToGroup-cpu-arguments",   "bls12_381_G2_hashToGroup-memory-arguments"),
        (Bls12_381_G2_Compress,     "bls12_381_G2_compress-cpu-arguments",      "bls12_381_G2_compress-memory-arguments"),
        (Bls12_381_G2_Uncompress,   "bls12_381_G2_uncompress-cpu-arguments",    "bls12_381_G2_uncompress-memory-arguments"),
        (Bls12_381_MillerLoop,      "bls12_381_millerLoop-cpu-arguments",       "bls12_381_millerLoop-memory-arguments"),
        (Bls12_381_MulMlResult,     "bls12_381_mulMlResult-cpu-arguments",      "bls12_381_mulMlResult-memory-arguments"),
        (Bls12_381_FinalVerify,     "bls12_381_finalVerify-cpu-arguments",      "bls12_381_finalVerify-memory-arguments"),
    ] {
        if !m.contains_key(&fun) {
            // BLS costs can be either single constant or linear-in-X (for hash-to-group)
            if let (Some(cpu), Some(mem)) = (constant(cpu_key), constant(mem_key)) {
                m.insert(fun, BuiltinCostEntry::new(CostFun::Constant(cpu), CostFun::Constant(mem)));
            } else {
                // Try linear form (for hashToGroup which scales with the DST size)
                if let (Some((ci, cs)), Some(mem)) = (
                    // hashToGroup uses "-cpu-arguments-intercept" / "-slope"
                    (|| {
                        let pfx = cpu_key.strip_suffix("-cpu-arguments")?;
                        let ci = constant(&format!("{pfx}-cpu-arguments-intercept"))?;
                        let cs = constant(&format!("{pfx}-cpu-arguments-slope"))?;
                        Some((ci, cs))
                    })(),
                    constant(mem_key),
                ) {
                    m.insert(fun, BuiltinCostEntry::new(
                        CostFun::LinearInX { intercept: ci, slope: cs },
                        CostFun::Constant(mem),
                    ));
                }
            }
        }
    }

    m
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn v1_params() -> BTreeMap<String, i64> {
        BTreeMap::from([
            // Machine steps
            ("cekVarCost-exBudgetCPU".into(),     29_773),
            ("cekConstCost-exBudgetCPU".into(),   29_773),
            ("cekLamCost-exBudgetCPU".into(),     29_773),
            ("cekDelayCost-exBudgetCPU".into(),   29_773),
            ("cekForceCost-exBudgetCPU".into(),   29_773),
            ("cekApplyCost-exBudgetCPU".into(),   29_773),
            ("cekVarCost-exBudgetMemory".into(),  100),
            ("cekConstCost-exBudgetMemory".into(), 100),
            ("cekLamCost-exBudgetMemory".into(),  100),
            ("cekDelayCost-exBudgetMemory".into(), 100),
            ("cekForceCost-exBudgetMemory".into(), 100),
            ("cekApplyCost-exBudgetMemory".into(), 100),
            ("cekBuiltinCost-exBudgetCPU".into(), 29_773),
            ("cekBuiltinCost-exBudgetMemory".into(), 100),
            ("cekStartupCost-exBudgetCPU".into(), 100),
            ("cekStartupCost-exBudgetMemory".into(), 100),
            // addInteger
            ("addInteger-cpu-arguments-intercept".into(), 197_209),
            ("addInteger-cpu-arguments-slope".into(),     0),
            ("addInteger-memory-arguments-intercept".into(), 1),
            ("addInteger-memory-arguments-slope".into(),     1),
            // sha2_256
            ("sha2_256-cpu-arguments-intercept".into(), 2_477_736),
            ("sha2_256-cpu-arguments-slope".into(),     29_175),
            ("sha2_256-memory-arguments".into(),        4),
            // blake2b
            ("blake2b-cpu-arguments-intercept".into(), 2_477_736),
            ("blake2b-cpu-arguments-slope".into(),     29_175),
            ("blake2b-memory-arguments".into(),        4),
            // divideInteger
            ("divideInteger-cpu-arguments-constant".into(),                   148_000),
            ("divideInteger-cpu-arguments-model-arguments-intercept".into(),  425_507),
            ("divideInteger-cpu-arguments-model-arguments-slope".into(),      118),
            ("divideInteger-memory-arguments-intercept".into(), 0),
            ("divideInteger-memory-arguments-minimum".into(),   1),
            ("divideInteger-memory-arguments-slope".into(),     1),
            // equalsData
            ("equalsData-cpu-arguments-intercept".into(), 150_000),
            ("equalsData-cpu-arguments-slope".into(),     10_000),
            ("equalsData-memory-arguments".into(),        1),
            // chooseData
            ("chooseData-cpu-arguments".into(),    150_000),
            ("chooseData-memory-arguments".into(), 32),
            // equalsByteString
            ("equalsByteString-cpu-arguments-constant".into(),   150_000),
            ("equalsByteString-cpu-arguments-intercept".into(),  112_536),
            ("equalsByteString-cpu-arguments-slope".into(),      247),
            ("equalsByteString-memory-arguments".into(),         1),
            // verifySignature (old V1 name for VerifyEd25519Signature)
            ("verifySignature-cpu-arguments-intercept".into(), 3_345_831),
            ("verifySignature-cpu-arguments-slope".into(),     1),
            ("verifySignature-memory-arguments".into(),        1),
        ])
    }

    #[test]
    fn parses_machine_step_costs() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        assert_eq!(model.machine_step_cost().cpu, 29_773);
        assert_eq!(model.machine_step_cost().mem, 100);
    }

    #[test]
    fn add_integer_charges_max_of_arg_sizes() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        // intercept=197209, slope=0 → always 197209 regardless of sizes
        let args = [
            Value::Constant(Constant::Integer(1)),
            Value::Constant(Constant::Integer(i128::MAX)),
        ];
        let cost = model.builtin_cost(DefaultFun::AddInteger, &args);
        // With slope=0: cost = intercept + 0 * max(1, 2) = 197209
        assert_eq!(cost.cpu, 197_209);
    }

    #[test]
    fn sha2_256_scales_with_input_size() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        // Empty bytestring: size=0 → cpu = intercept + slope * 0 = 2_477_736
        let empty = [Value::Constant(Constant::ByteString(vec![]))];
        let small = [Value::Constant(Constant::ByteString(vec![0u8; 32]))];
        let large = [Value::Constant(Constant::ByteString(vec![0u8; 1024]))];
        let cost_empty = model.builtin_cost(DefaultFun::Sha2_256, &empty);
        let cost_small = model.builtin_cost(DefaultFun::Sha2_256, &small);
        let cost_large = model.builtin_cost(DefaultFun::Sha2_256, &large);
        // cpu = 2_477_736 + 29_175 * len_bytes
        assert_eq!(cost_empty.cpu, 2_477_736);
        assert_eq!(cost_small.cpu, 2_477_736 + 29_175 * 32);
        assert_eq!(cost_large.cpu, 2_477_736 + 29_175 * 1024);
        assert!(cost_large.cpu > cost_small.cpu, "larger input should cost more");
    }

    #[test]
    fn divide_integer_constant_below_diagonal() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        // dividend < divisor → below diagonal → constant = 148000
        let args = [
            Value::Constant(Constant::Integer(3)),   // |3| = 1 word
            Value::Constant(Constant::Integer(1_000_000_000_000_000_000i128)), // large
        ];
        let cost = model.builtin_cost(DefaultFun::DivideInteger, &args);
        // x=1 word, y=1 word (both fit in i64 → 1 word); x <= y → constant
        assert_eq!(cost.cpu, 148_000);
    }

    #[test]
    fn equals_bytestring_constant_off_diagonal() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        // Different lengths → off diagonal → constant cost
        let args = [
            Value::Constant(Constant::ByteString(vec![0u8; 10])),
            Value::Constant(Constant::ByteString(vec![0u8; 20])),
        ];
        let cost = model.builtin_cost(DefaultFun::EqualsByteString, &args);
        assert_eq!(cost.cpu, 150_000); // constant (off diagonal)
    }

    #[test]
    fn equals_bytestring_linear_on_diagonal() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        // Same lengths → on diagonal → linear in x = 32
        let args = [
            Value::Constant(Constant::ByteString(vec![0u8; 32])),
            Value::Constant(Constant::ByteString(vec![0u8; 32])),
        ];
        let cost = model.builtin_cost(DefaultFun::EqualsByteString, &args);
        // model_intercept=112536, model_slope=247, x=y=32
        assert_eq!(cost.cpu, 112_536 + 247 * 32);
    }

    #[test]
    fn choosedata_is_constant() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        let args: Vec<Value> = (0..6)
            .map(|_| Value::Constant(Constant::Unit))
            .collect();
        let cost = model.builtin_cost(DefaultFun::ChooseData, &args);
        assert_eq!(cost.cpu, 150_000);
        assert_eq!(cost.mem, 32);
    }

    #[test]
    fn verify_ed25519_scales_with_message_size() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        let vk  = Value::Constant(Constant::ByteString(vec![0u8; 32]));
        let sig = Value::Constant(Constant::ByteString(vec![0u8; 64]));
        let small_msg = Value::Constant(Constant::ByteString(vec![0u8; 32]));
        let large_msg = Value::Constant(Constant::ByteString(vec![0u8; 512]));
        let cost_small = model.builtin_cost(
            DefaultFun::VerifyEd25519Signature, &[vk.clone(), small_msg, sig.clone()]);
        let cost_large = model.builtin_cost(
            DefaultFun::VerifyEd25519Signature, &[vk, large_msg, sig]);
        // intercept=3345831, slope=1; scales with Y (message, arg index 1)
        assert_eq!(cost_small.cpu, 3_345_831 + 1 * 32);
        assert_eq!(cost_large.cpu, 3_345_831 + 1 * 512);
        assert!(cost_large.cpu > cost_small.cpu);
    }

    #[test]
    fn unknown_builtin_falls_back_to_default() {
        let model = CostModel::from_alonzo_genesis_params(&v1_params())
            .expect("should parse");
        // ExpModInteger not in our sample params → falls back to default
        let args = [
            Value::Constant(Constant::Integer(2)),
            Value::Constant(Constant::Integer(10)),
            Value::Constant(Constant::Integer(1000)),
        ];
        let cost = model.builtin_cost(DefaultFun::ExpModInteger, &args);
        assert_eq!(cost.cpu, 29_773);
        assert_eq!(cost.mem, 100);
    }

    #[test]
    fn integer_size_measurement() {
        assert_eq!(integer_size(0), 1);
        assert_eq!(integer_size(1), 1);
        // 2^63 = 9223372036854775808 still fits in one 64-bit word
        assert_eq!(integer_size((1i128 << 63) - 1), 1);
        assert_eq!(integer_size(1i128 << 63), 1);
        // 2^64 requires two 64-bit words
        assert_eq!(integer_size(1i128 << 64), 2);
        assert_eq!(integer_size(-1), 1);
        assert_eq!(integer_size(i128::MIN), 2);
        assert_eq!(integer_size(i128::MAX), 2);
    }

    #[test]
    fn backwards_compatible_from_old_genesis_params() {
        // The original 4-key test from the previous flat implementation
        let mut params = v1_params();
        // Add a diverging step cost to verify max is taken
        params.insert("cekApplyCost-exBudgetCPU".into(), 40_000);
        let model = CostModel::from_alonzo_genesis_params(&params)
            .expect("should parse");
        assert_eq!(model.machine_step_cost().cpu, 40_000);
        assert_eq!(model.machine_step_cost().mem, 100);
    }
}
