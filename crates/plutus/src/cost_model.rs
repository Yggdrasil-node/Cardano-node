//! Cost model for UPLC evaluation budget tracking.
//!
//! Provides a simplified cost model that charges fixed per-step and
//! per-builtin costs. A full parameterized cost model matching the
//! Haskell node's `CostModelApplyFun` is a future extension.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model>

use crate::types::{DefaultFun, ExBudget, Value};

/// Cost model used by the CEK machine for budget accounting.
///
/// The initial implementation uses a fixed per-step cost and a
/// per-builtin flat cost. The Cardano mainnet cost model uses
/// parameterized linear/quadratic cost functions keyed by argument
/// sizes — that is deferred to a later milestone.
#[derive(Clone, Debug)]
pub struct CostModel {
    /// CPU cost charged per CEK machine step.
    pub step_cpu: i64,
    /// Memory cost charged per CEK machine step.
    pub step_mem: i64,
    /// Default CPU cost per builtin invocation.
    pub builtin_cpu: i64,
    /// Default memory cost per builtin invocation.
    pub builtin_mem: i64,
}

impl Default for CostModel {
    /// Returns a conservative default cost model.
    ///
    /// These values are intentionally generous so that tests and simple
    /// scripts succeed without tuning. Production use MUST supply the
    /// cost model from the protocol parameters.
    fn default() -> Self {
        Self {
            step_cpu: 100,
            step_mem: 100,
            builtin_cpu: 1_000,
            builtin_mem: 1_000,
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
    /// A full cost model would look up per-builtin cost functions and
    /// evaluate them against argument sizes. This simplified version
    /// charges a flat cost regardless of arguments.
    pub fn builtin_cost(&self, _fun: DefaultFun, _args: &[Value]) -> ExBudget {
        ExBudget::new(self.builtin_cpu, self.builtin_mem)
    }
}
