//! CEK machine step kinds + per-operation step costs.
//!
//! Mirrors upstream `UntypedPlutusCore.Evaluation.Machine.Cek.Internal::StepKind`
//! and `PlutusCore.Evaluation.Machine.MachineParameters::CostingPart` per-step
//! cost wiring.
//!
//! Two public types:
//!
//! - `StepKind` — CEK machine operation kinds (Constant, Var, LamAbs,
//!   Apply, Delay, Force, Builtin, Constr, Case, plus Compute/Return/StartUp).
//! - `StepCosts` — per-step CPU + memory charges, indexed by `StepKind`.
//!
//! Extracted from `cost_model.rs` in R273h (Phase γ §R273 eighth slice).

use crate::types::ExBudget;

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

impl StepKind {
    /// Number of CEK step kinds with distinct budget costs.
    pub const COUNT: usize = 9;

    /// Stable order used when accumulating batched CEK step counts.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Constant,
        Self::Var,
        Self::LamAbs,
        Self::Apply,
        Self::Delay,
        Self::Force,
        Self::Builtin,
        Self::Constr,
        Self::Case,
    ];

    /// Index into [`Self::ALL`].
    pub const fn index(self) -> usize {
        match self {
            Self::Constant => 0,
            Self::Var => 1,
            Self::LamAbs => 2,
            Self::Apply => 3,
            Self::Delay => 4,
            Self::Force => 5,
            Self::Builtin => 6,
            Self::Constr => 7,
            Self::Case => 8,
        }
    }
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
            var_cpu: 100,
            var_mem: 100,
            constant_cpu: 100,
            constant_mem: 100,
            lam_cpu: 100,
            lam_mem: 100,
            apply_cpu: 100,
            apply_mem: 100,
            delay_cpu: 100,
            delay_mem: 100,
            force_cpu: 100,
            force_mem: 100,
            builtin_cpu: 100,
            builtin_mem: 100,
            constr_cpu: 100,
            constr_mem: 100,
            case_cpu: 100,
            case_mem: 100,
        }
    }
}

impl StepCosts {
    /// Return the CPU and memory cost for a particular step kind.
    pub fn cost(&self, kind: StepKind) -> ExBudget {
        match kind {
            StepKind::Var => ExBudget::new(self.var_cpu, self.var_mem),
            StepKind::Constant => ExBudget::new(self.constant_cpu, self.constant_mem),
            StepKind::LamAbs => ExBudget::new(self.lam_cpu, self.lam_mem),
            StepKind::Apply => ExBudget::new(self.apply_cpu, self.apply_mem),
            StepKind::Delay => ExBudget::new(self.delay_cpu, self.delay_mem),
            StepKind::Force => ExBudget::new(self.force_cpu, self.force_mem),
            StepKind::Builtin => ExBudget::new(self.builtin_cpu, self.builtin_mem),
            StepKind::Constr => ExBudget::new(self.constr_cpu, self.constr_mem),
            StepKind::Case => ExBudget::new(self.case_cpu, self.case_mem),
        }
    }
}
