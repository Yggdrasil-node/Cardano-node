//! Cost-expression evaluator for per-builtin costing functions.
//!
//! Mirrors upstream `PlutusCore.Cost.CostingFun` cost-function shapes:
//! `Constant`, `LinearInX/Y/Z`, `LinearForm`, `AddedSizes`, `MaxSize`,
//! `MinSize`, `SubtractedSizes`, `LinearOnDiagonal`, `QuadraticInY`,
//! `QuadraticInZ`, `QuadraticInXAndY`, `LiteralInYOrLinearInZ`.
//!
//! Single public type:
//!
//! - `CostExpr` — algebraic cost expression evaluated against the
//!   per-builtin argument sizes to produce CPU or memory cost.
//!
//! Extracted from `cost_model.rs` in R273h (Phase γ §R273 eighth slice).

/// A single-dimension cost expression (CPU or memory) for a builtin.
///
/// Each variant mirrors one upstream Haskell `CostingFun` shape.
/// Reference: <https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Evaluation/Machine/CostingFun/Core.hs>
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
    LinearForm {
        intercept: i64,
        x: i64,
        y: i64,
        z: i64,
    },
    /// `intercept + slope * (size(arg[0]) + size(arg[1]))`.
    AddedSizes { intercept: i64, slope: i64 },
    /// `intercept + slope * max(size(arg[0]), size(arg[1]))`.
    MaxSize { intercept: i64, slope: i64 },
    /// `intercept + slope * min(size(arg[0]), size(arg[1]))`.
    MinSize { intercept: i64, slope: i64 },
    /// `max(minimum, intercept + slope * max(0, size(arg[0]) - size(arg[1])))`.
    SubtractedSizes {
        intercept: i64,
        slope: i64,
        minimum: i64,
    },
    /// `intercept + slope * max(size(arg[1]), size(arg[2]))`.
    ///
    /// Used by and/or/xorByteString memory costing where the first arg is a
    /// boolean padding flag and the two bytestring operands are args 1 and 2.
    MaxSizeYZ { intercept: i64, slope: i64 },
    /// `c00 + c11 * (y * z) + c12 * (y * z²)` where `y = size(arg[1])`,
    /// `z = size(arg[2])`. If `size(arg[0]) > size(arg[2])`, cost is increased
    /// by 50% (upstream penalty for unreduced base in `expModInteger`).
    ///
    /// Upstream `evaluateExpModCostingFunction` from the Plutus CEK cost model.
    ExpModCost { c00: i64, c11: i64, c12: i64 },
    /// `intercept + slope * (size(arg[0]) * size(arg[1]))`.
    ///
    /// Upstream `ModelTwoArgumentsMultipliedSizes`.
    MultipliedSizes { intercept: i64, slope: i64 },
    /// If `size(arg[0]) == size(arg[1])` then `intercept + slope * size(arg[0])`,
    /// else `constant`.
    ///
    /// Upstream `ModelTwoArgumentsLinearOnDiagonal` / `ModelConstantOrLinear`.
    /// Used by `equalsByteString` and `equalsString`.
    LinearOnDiagonal {
        constant: i64,
        intercept: i64,
        slope: i64,
    },
    /// If `size(arg[0]) < size(arg[1])` then `constant`, else
    /// `inner.evaluate(sizes)`.
    ///
    /// Upstream `ModelTwoArgumentsConstAboveDiagonal`.
    /// Used by division builtins (`divideInteger`, `modInteger`, etc.).
    ConstAboveDiagonal { constant: i64, inner: Box<CostExpr> },
    /// `max(minimum, c00 + c10*x + c01*y + c20*x² + c11*x*y + c02*y²)`
    /// where `x = size(arg[0])`, `y = size(arg[1])`.
    ///
    /// Upstream `TwoVariableQuadraticFunction`.
    TwoVarQuadratic {
        minimum: i64,
        c00: i64,
        c10: i64,
        c01: i64,
        c20: i64,
        c11: i64,
        c02: i64,
    },
    /// `c0 + c1 * size(arg[1]) + c2 * size(arg[1])²`.
    ///
    /// Upstream `ModelTwoArgumentsQuadraticInY` / `ModelThreeArgumentsQuadraticInZ`
    /// (one-variable quadratic on the specified argument index).
    QuadraticInY { c0: i64, c1: i64, c2: i64 },
    /// `c0 + c1 * size(arg[2]) + c2 * size(arg[2])²`.
    ///
    /// Upstream `ModelThreeArgumentsQuadraticInZ`.
    QuadraticInZ { c0: i64, c1: i64, c2: i64 },
    /// If `size(arg[1]) == 0` then `intercept + slope * size(arg[2])`,
    /// else `size(arg[1])` literally.
    ///
    /// Upstream `ModelThreeArgumentsLiteralInYOrLinearInZ`.
    /// Used by `integerToByteString` memory.
    LiteralInYOrLinearInZ { intercept: i64, slope: i64 },
}

impl CostExpr {
    /// Evaluate the cost expression given the pre-computed per-argument sizes.
    ///
    /// Missing argument sizes default to 1 (conservative). Results are clamped
    /// to 0 from below.
    pub fn evaluate(&self, sizes: &[i64]) -> i64 {
        let sz = |idx: usize| sizes.get(idx).copied().unwrap_or(1).max(0);
        let raw = match self {
            Self::Constant(c) => *c,
            Self::LinearInX { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(0)))
            }
            Self::LinearInY { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(1)))
            }
            Self::LinearInZ { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(2)))
            }
            Self::LinearForm { intercept, x, y, z } => intercept
                .saturating_add(x.saturating_mul(sz(0)))
                .saturating_add(y.saturating_mul(sz(1)))
                .saturating_add(z.saturating_mul(sz(2))),
            Self::AddedSizes { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(0).saturating_add(sz(1))))
            }
            Self::MaxSize { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(0).max(sz(1))))
            }
            Self::MinSize { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(0).min(sz(1))))
            }
            Self::SubtractedSizes {
                intercept,
                slope,
                minimum,
            } => (*minimum)
                .max(intercept.saturating_add(slope.saturating_mul((sz(0) - sz(1)).max(0)))),
            Self::MaxSizeYZ { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(1).max(sz(2))))
            }
            Self::ExpModCost { c00, c11, c12 } => {
                let aa = sz(0);
                let ee = sz(1);
                let mm = sz(2);
                let em = ee.saturating_mul(mm);
                let cost0 = c00
                    .saturating_add(c11.saturating_mul(em))
                    .saturating_add(c12.saturating_mul(em.saturating_mul(mm)));
                // Upstream penalty: if base size > modulus size, increase by 50%.
                if aa > mm {
                    cost0.saturating_add(cost0 / 2)
                } else {
                    cost0
                }
            }
            Self::MultipliedSizes { intercept, slope } => {
                intercept.saturating_add(slope.saturating_mul(sz(0).saturating_mul(sz(1))))
            }
            Self::LinearOnDiagonal {
                constant,
                intercept,
                slope,
            } => {
                if sz(0) == sz(1) {
                    intercept.saturating_add(slope.saturating_mul(sz(0)))
                } else {
                    *constant
                }
            }
            Self::ConstAboveDiagonal { constant, inner } => {
                if sz(0) < sz(1) {
                    *constant
                } else {
                    inner.evaluate(sizes)
                }
            }
            Self::TwoVarQuadratic {
                minimum,
                c00,
                c10,
                c01,
                c20,
                c11,
                c02,
            } => {
                let x = sz(0);
                let y = sz(1);
                let val = c00
                    .saturating_add(c10.saturating_mul(x))
                    .saturating_add(c01.saturating_mul(y))
                    .saturating_add(c20.saturating_mul(x.saturating_mul(x)))
                    .saturating_add(c11.saturating_mul(x.saturating_mul(y)))
                    .saturating_add(c02.saturating_mul(y.saturating_mul(y)));
                val.max(*minimum)
            }
            Self::QuadraticInY { c0, c1, c2 } => {
                let y = sz(1);
                c0.saturating_add(c1.saturating_mul(y))
                    .saturating_add(c2.saturating_mul(y.saturating_mul(y)))
            }
            Self::QuadraticInZ { c0, c1, c2 } => {
                let z = sz(2);
                c0.saturating_add(c1.saturating_mul(z))
                    .saturating_add(c2.saturating_mul(z.saturating_mul(z)))
            }
            Self::LiteralInYOrLinearInZ { intercept, slope } => {
                let y = sz(1);
                if y == 0 {
                    intercept.saturating_add(slope.saturating_mul(sz(2)))
                } else {
                    y
                }
            }
        };
        raw.max(0)
    }
}
