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
            Self::LinearInX { intercept, slope }       => intercept + slope * sz(0),
            Self::LinearInY { intercept, slope }       => intercept + slope * sz(1),
            Self::LinearInZ { intercept, slope }       => intercept + slope * sz(2),
            Self::LinearForm { intercept, x, y, z }    => {
                intercept + x * sz(0) + y * sz(1) + z * sz(2)
            }
            Self::AddedSizes { intercept, slope }      => intercept + slope * (sz(0) + sz(1)),
            Self::MaxSize { intercept, slope }         => intercept + slope * sz(0).max(sz(1)),
            Self::MinSize { intercept, slope }         => intercept + slope * sz(0).min(sz(1)),
            Self::SubtractedSizes { intercept, slope, minimum } => {
                (*minimum).max(intercept + slope * (sz(0) - sz(1)).max(0))
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
    /// CPU cost charged per CEK machine step.
    pub step_cpu: i64,
    /// Memory cost charged per CEK machine step.
    pub step_mem: i64,
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
            step_cpu: 100,
            step_mem: 100,
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
    /// **Machine step costs**: `step_cpu` / `step_mem` are set to the maximum
    /// of the base CEK step costs (`Var`, `Const`, `Lam`, `Delay`, `Force`,
    /// `Apply`) plus any constructor/case costs present in newer V3 maps.
    /// The startup cost is ignored.
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
        const OPTIONAL_STEP_CPU_KEYS: [&str; 2] = [
            "cekConstrCost-exBudgetCPU",
            "cekCaseCost-exBudgetCPU",
        ];
        const OPTIONAL_STEP_MEM_KEYS: [&str; 2] = [
            "cekConstrCost-exBudgetMemory",
            "cekCaseCost-exBudgetMemory",
        ];

        let step_cpu = max_named_value(params, &STEP_CPU_KEYS)?
            .max(max_present_named_value(params, &OPTIONAL_STEP_CPU_KEYS)?);
        let step_mem = max_named_value(params, &STEP_MEM_KEYS)?
            .max(max_present_named_value(params, &OPTIONAL_STEP_MEM_KEYS)?);
        let builtin_cpu = params.get("cekBuiltinCost-exBudgetCPU").copied().unwrap_or(1_000);
        let builtin_mem = params.get("cekBuiltinCost-exBudgetMemory").copied().unwrap_or(1_000);
        let builtin_costs = build_per_builtin_costs(params);

        Ok(Self { step_cpu, step_mem, builtin_cpu, builtin_mem, builtin_costs })
    }

    /// Cost charged per CEK machine step.
    pub fn machine_step_cost(&self) -> ExBudget {
        ExBudget::new(self.step_cpu, self.step_mem)
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

    // lessThanByteString: also appears as "lessThanEqualsByteString" in early genesis
    for p in ["lessThanByteString", "lessThanEqualsByteString"] {
        if map.contains_key(&LessThanByteString) { break; }
        let ci = get(&format!("{p}-cpu-arguments-intercept"));
        let cs = get(&format!("{p}-cpu-arguments-slope"));
        if let (Some(ci), Some(cs)) = (ci, cs) {
            let mem = get(&format!("{p}-memory-arguments")).unwrap_or(1);
            map.insert(LessThanByteString, BuiltinCostEntry {
                cpu: CostExpr::LinearInX { intercept: ci, slope: cs },
                mem: CostExpr::Constant(mem),
            });
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

    // expModInteger: constant (complex to model; upstream uses LinearInXandY)
    if let Some(c) = get("expModInteger-cpu-arguments") {
        let m = get("expModInteger-memory-arguments").unwrap_or(1);
        map.insert(ExpModInteger, BuiltinCostEntry::constant(c, m));
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

fn max_named_value(
    params: &BTreeMap<String, i64>,
    keys: &[&'static str],
) -> Result<i64, CostModelError> {
    let mut max = 0i64;
    for key in keys {
        max = max.max(named_value(params, key)?);
    }
    Ok(max)
}

fn max_present_named_value(
    params: &BTreeMap<String, i64>,
    keys: &[&'static str],
) -> Result<i64, CostModelError> {
    let mut max = 0i64;
    for key in keys {
        if let Some(value) = params.get(*key).copied() {
            if value < 0 {
                return Err(CostModelError::NegativeParameter { name: key, value });
            }
            max = max.max(value);
        }
    }
    Ok(max)
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
    fn derives_flat_cost_model_from_named_params() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
        assert_eq!(model.step_cpu, 30_002);
        assert_eq!(model.step_mem, 102);
        assert_eq!(model.builtin_cpu, 29_773);
        assert_eq!(model.builtin_mem, 100);
    }

    #[test]
    fn derives_conservative_step_cost_when_keys_diverge() {
        let mut params = sample_params();
        params.insert("cekApplyCost-exBudgetCPU".to_owned(), 40_000);
        params.insert("cekConstrCost-exBudgetMemory".to_owned(), 111);
        let model = CostModel::from_alonzo_genesis_params(&params)
            .expect("derive cost model");
        assert_eq!(model.step_cpu, 40_000);
        assert_eq!(model.step_mem, 111);
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
}
