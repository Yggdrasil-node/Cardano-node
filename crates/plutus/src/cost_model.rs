//! Cost model for UPLC evaluation budget tracking.
//!
//! Provides a simplified cost model that charges fixed per-step and
//! per-builtin costs. A full parameterized cost model matching the
//! Haskell node's `CostModelApplyFun` is a future extension.
//!
//! Reference: <https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model>

use std::collections::BTreeMap;

use thiserror::Error;

use crate::types::{DefaultFun, ExBudget, Value};

/// Errors returned while deriving a flat CEK cost model from upstream
/// named cost-model parameters.
#[derive(Debug, Error)]
pub enum CostModelError {
    /// A required named parameter was absent.
    #[error("missing cost-model parameter: {0}")]
    MissingParameter(&'static str),
    /// A required named parameter was present but negative, which cannot be
    /// represented by the current flat cost model.
    #[error("invalid negative cost-model parameter {name}: {value}")]
    NegativeParameter {
        name: &'static str,
        value: i64,
    },
}

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
    /// Derive the current simplified flat cost model from an upstream Alonzo
    /// or later named Plutus cost-model map.
    ///
    /// The real Cardano evaluator uses many per-builtin linear/quadratic cost
    /// functions. This crate currently supports only four flat knobs:
    /// per-step CPU/memory and per-builtin CPU/memory. We therefore map:
    ///
    /// - `step_cpu` / `step_mem` to the maximum of the non-startup CEK step
    ///   costs (`Var`, `Const`, `Lam`, `Delay`, `Force`, `Apply`) so the flat
    ///   model remains conservative when upstream costs diverge.
    /// - `builtin_cpu` / `builtin_mem` to `cekBuiltinCost-*`.
    ///
    /// The one-off `cekStartupCost-*` values are intentionally ignored because
    /// the current CEK machine does not charge a separate startup tick.
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

        let step_cpu = max_named_value(params, &STEP_CPU_KEYS)?;
        let step_mem = max_named_value(params, &STEP_MEM_KEYS)?;
        let builtin_cpu = named_value(params, "cekBuiltinCost-exBudgetCPU")?;
        let builtin_mem = named_value(params, "cekBuiltinCost-exBudgetMemory")?;

        Ok(Self {
            step_cpu,
            step_mem,
            builtin_cpu,
            builtin_mem,
        })
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_params() -> BTreeMap<String, i64> {
        BTreeMap::from([
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
        ])
    }

    #[test]
    fn derives_flat_cost_model_from_named_params() {
        let model = CostModel::from_alonzo_genesis_params(&sample_params())
            .expect("derive cost model");
        assert_eq!(model.step_cpu, 29_773);
        assert_eq!(model.step_mem, 100);
        assert_eq!(model.builtin_cpu, 29_773);
        assert_eq!(model.builtin_mem, 100);
    }

    #[test]
    fn derives_conservative_step_cost_when_keys_diverge() {
        let mut params = sample_params();
        params.insert("cekApplyCost-exBudgetCPU".to_owned(), 40_000);
        let model = CostModel::from_alonzo_genesis_params(&params)
            .expect("derive cost model");
        assert_eq!(model.step_cpu, 40_000);
    }

    #[test]
    fn rejects_missing_parameter() {
        let mut params = sample_params();
        params.remove("cekBuiltinCost-exBudgetCPU");
        let err = CostModel::from_alonzo_genesis_params(&params)
            .expect_err("missing parameter must fail");
        assert!(matches!(err, CostModelError::MissingParameter("cekBuiltinCost-exBudgetCPU")));
    }
}
